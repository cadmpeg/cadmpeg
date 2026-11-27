// SPDX-License-Identifier: Apache-2.0
//! Field access by schema name over one framed node.

use super::{has_base_layout, PrimaryEnd, PrimaryRecord, PrimaryRecords};
use crate::records::{BaseVersion, DataType, SchemaPlan};
use cadmpeg_core::decode::View;
use std::ops::Range;

use crate::records::fields::NULL_REAL;

/// One framed node of a node sequence, with its fields addressed by name.
#[derive(Debug, Clone, Copy)]
pub struct Node<'r> {
    source: &'r [u8],
    record: &'r PrimaryRecord,
    version: BaseVersion,
}

impl<'a> PrimaryRecords<'a> {
    /// Framed nodes in stream order.
    pub fn nodes(&self) -> impl Iterator<Item = Node<'_>> {
        self.records.iter().map(|record| Node {
            source: self.source,
            record,
            version: self.version,
        })
    }

    /// `record`, read by [`PrimaryRecords::read_at`], as a node of this
    /// sequence.
    #[must_use]
    pub fn node<'r>(&'r self, record: &'r PrimaryRecord) -> Node<'r> {
        Node {
            source: self.source,
            record,
            version: self.version,
        }
    }

    /// Whether the node sequence ends at its terminator.
    #[must_use]
    pub const fn is_terminated(&self) -> bool {
        matches!(self.end, PrimaryEnd::Terminated { .. })
    }

    /// The bytes before the first node and the bytes from the terminator to
    /// the end of the stream, when the node sequence reaches its terminator.
    pub(crate) fn frame(&self) -> Option<(&'a [u8], &'a [u8])> {
        let PrimaryEnd::Terminated { terminator, .. } = &self.end else {
            return None;
        };
        let start = self
            .records
            .first()
            .map_or(terminator.start, |record| record.range.start);
        Some((
            self.source.get(..start)?,
            self.source.get(terminator.start..)?,
        ))
    }
}

impl<'r> Node<'r> {
    /// The node type.
    #[must_use]
    pub fn kind(&self) -> u16 {
        self.record.definition.kind
    }

    /// The node's encoded index, which pointer fields of other nodes name.
    #[must_use]
    pub fn index(&self) -> u32 {
        u32::from(self.record.xmt)
    }

    /// The node's byte range in the stream, from its type to its last field.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.record.range.clone()
    }

    /// The node's effective field list.
    pub(crate) fn plan(&self) -> &'r SchemaPlan {
        &self.record.definition
    }

    /// The bytes between the node's type and its variable count or identity:
    /// its type's declaration when the node is the first of its type in the
    /// stream, else empty.
    pub(crate) fn declaration(&self) -> Option<&'r [u8]> {
        self.source
            .get(self.record.range.start.checked_add(2)?..self.encoding_start()?)
    }

    /// The node's variable count, identity, and fields.
    pub(crate) fn encoding(&self) -> Option<&'r [u8]> {
        self.source
            .get(self.encoding_start()?..self.record.range.end)
    }

    fn encoding_start(&self) -> Option<usize> {
        let count = if self.record.variable_length.is_some() {
            4
        } else {
            0
        };
        self.record.identity_at.checked_sub(count)
    }

    /// Whether each base field of the node's type has its base name,
    /// encoding, and position at the start of the node's fields.
    #[must_use]
    pub fn has_base_layout(&self) -> bool {
        has_base_layout(self.version, self.record)
    }

    /// Where the node's first field starts, after its header.
    #[must_use]
    pub fn fields_start(&self) -> Option<usize> {
        self.record.fields.first().map(|field| field.start)
    }

    /// The byte range of the named field in the stream.
    #[must_use]
    pub fn field_range(&self, name: &str) -> Option<Range<usize>> {
        let index = self
            .record
            .definition
            .fields
            .iter()
            .position(|field| field.name.as_str() == name)?;
        self.record.fields.get(index).cloned()
    }

    /// A view over the named field when it is a transmitted scalar of
    /// `data_type`; an array, a variable field, or another type gives `None`.
    fn scalar(&self, name: &str, data_type: DataType) -> Option<View<'r>> {
        let fields = &self.record.definition.fields;
        let index = fields
            .iter()
            .position(|field| field.name.as_str() == name)?;
        let field = fields.get(index)?;
        (field.data_type == data_type && field.n_elements == 0).then_some(())?;
        let range = self.record.fields.get(index)?.clone();
        Some(View::over_retained(self.source.get(range)?))
    }

    /// The nonzero encoded index a scalar pointer field names.
    #[must_use]
    pub fn pointer(&self, name: &str) -> Option<u32> {
        crate::pointer::read(&mut self.scalar(name, DataType::Pointer)?).filter(|value| *value != 0)
    }

    /// The encoded indices of the named scalar pointer fields, in order.
    #[must_use]
    pub fn pointers<const N: usize>(&self, names: [&str; N]) -> Option<[u32; N]> {
        let mut out = [0; N];
        for (value, name) in out.iter_mut().zip(names) {
            *value = self.pointer(name)?;
        }
        Some(out)
    }

    /// The value of a scalar short field.
    #[must_use]
    pub fn short(&self, name: &str) -> Option<u16> {
        self.scalar(name, DataType::Short)?.u16_be()
    }

    /// The value of a scalar logical field.
    #[must_use]
    pub fn logical(&self, name: &str) -> Option<bool> {
        match self.scalar(name, DataType::Logical)?.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// A view over the named variable field when it holds `data_type`, and
    /// its element count.
    fn variable(&self, name: &str, data_type: DataType) -> Option<(View<'r>, usize)> {
        let count = usize::try_from(self.record.variable_length?).ok()?;
        let range = self.variable_range(name, data_type)?;
        Some((View::over_retained(self.source.get(range)?), count))
    }

    /// The byte range of the named field when it is a transmitted variable
    /// field of `data_type`.
    #[must_use]
    pub fn variable_range(&self, name: &str, data_type: DataType) -> Option<Range<usize>> {
        let fields = &self.record.definition.fields;
        let index = fields
            .iter()
            .position(|field| field.name.as_str() == name)?;
        let field = fields.get(index)?;
        (field.data_type == data_type && field.n_elements == 1 && field.transmit).then_some(())?;
        self.record.fields.get(index).cloned()
    }

    /// A view over the named field when it is a fixed array of `N` elements
    /// of `data_type`.
    fn fixed<const N: usize>(&self, name: &str, data_type: DataType) -> Option<View<'r>> {
        let fields = &self.record.definition.fields;
        let index = fields
            .iter()
            .position(|field| field.name.as_str() == name)?;
        let field = fields.get(index)?;
        let extent = usize::try_from(field.n_elements).ok()?;
        (field.data_type == data_type && extent == N && N > 1).then_some(())?;
        let range = self.record.fields.get(index)?.clone();
        Some(View::over_retained(self.source.get(range)?))
    }

    /// The nonzero encoded indices of a fixed pointer array of `N` elements.
    #[must_use]
    pub fn pointer_array<const N: usize>(&self, name: &str) -> Option<[u32; N]> {
        let mut view = self.fixed::<N>(name, DataType::Pointer)?;
        let mut out = [0; N];
        for value in &mut out {
            *value = crate::pointer::read(&mut view).filter(|value| *value != 0)?;
        }
        Some(out)
    }

    /// The values of a fixed byte array of `N` elements.
    #[must_use]
    pub fn byte_array<const N: usize>(&self, name: &str) -> Option<[u8; N]> {
        let mut view = self.fixed::<N>(name, DataType::Byte)?;
        view.take(N)?.try_into().ok()
    }

    /// The values of a fixed logical array of the extent its schema
    /// declares; `None` when an element is outside `0` and `1`.
    #[must_use]
    pub fn logicals(&self, name: &str) -> Option<Vec<bool>> {
        let fields = &self.record.definition.fields;
        let index = fields
            .iter()
            .position(|field| field.name.as_str() == name)?;
        let field = fields.get(index)?;
        (field.data_type == DataType::Logical && field.n_elements > 1).then_some(())?;
        let range = self.record.fields.get(index)?.clone();
        self.source
            .get(range)?
            .iter()
            .map(|value| match value {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            })
            .collect()
    }

    /// The values of a fixed real array of `N` elements.
    #[must_use]
    pub fn real_array<const N: usize>(&self, name: &str) -> Option<[f64; N]> {
        let mut view = self.fixed::<N>(name, DataType::Real)?;
        let mut out = [0.0; N];
        for value in &mut out {
            *value = f64::from_bits(view.u64_be()?);
        }
        Some(out)
    }

    /// The values of a variable real field.
    #[must_use]
    pub fn reals(&self, name: &str) -> Option<Vec<f64>> {
        let (mut view, count) = self.variable(name, DataType::Real)?;
        (0..count)
            .map(|_| view.u64_be().map(f64::from_bits))
            .collect()
    }

    /// The rows of a variable Hvec field, each a point of three reals.
    #[must_use]
    pub fn hvec_points(&self, name: &str) -> Option<Vec<[f64; 3]>> {
        let (mut view, count) = self.variable(name, DataType::Hvec)?;
        (0..count)
            .map(|_| {
                let mut point = [0.0; 3];
                for value in &mut point {
                    *value = f64::from_bits(view.u64_be()?);
                }
                Some(point)
            })
            .collect()
    }

    /// The values of a variable character field.
    #[must_use]
    pub fn chars(&self, name: &str) -> Option<Vec<u8>> {
        let (mut view, count) = self.variable(name, DataType::Char)?;
        view.take(count).map(<[u8]>::to_vec)
    }

    /// The values of a variable byte field.
    #[must_use]
    pub fn bytes(&self, name: &str) -> Option<Vec<u8>> {
        let (mut view, count) = self.variable(name, DataType::Byte)?;
        view.take(count).map(<[u8]>::to_vec)
    }

    /// The values of a variable integer field.
    #[must_use]
    pub fn integers(&self, name: &str) -> Option<Vec<u32>> {
        let (mut view, count) = self.variable(name, DataType::Integer)?;
        (0..count).map(|_| view.u32_be()).collect()
    }

    /// The encoded indices of a variable pointer field; the null index is `1`.
    #[must_use]
    pub fn pointer_list(&self, name: &str) -> Option<Vec<u32>> {
        let (mut view, count) = self.variable(name, DataType::Pointer)?;
        (0..count)
            .map(|_| crate::pointer::read(&mut view).filter(|value| *value != 0))
            .collect()
    }

    /// The values of a variable short field.
    #[must_use]
    pub fn shorts(&self, name: &str) -> Option<Vec<u16>> {
        let (mut view, count) = self.variable(name, DataType::Short)?;
        (0..count).map(|_| view.u16_be()).collect()
    }

    /// The encoded index of every transmitted pointer element, in field
    /// order; the null index is `1`.
    #[must_use]
    pub fn pointer_targets(&self) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        for (field, range) in self
            .record
            .definition
            .fields
            .iter()
            .zip(&self.record.fields)
        {
            if field.data_type != DataType::Pointer {
                continue;
            }
            let mut view = View::over_retained(self.source.get(range.clone())?);
            while view.remaining() > 0 {
                out.push(crate::pointer::read(&mut view).filter(|value| *value != 0)?);
            }
        }
        Some(out)
    }

    /// The value of a scalar integer field.
    #[must_use]
    pub fn integer(&self, name: &str) -> Option<u32> {
        self.scalar(name, DataType::Integer)?.u32_be()
    }

    /// The value of a scalar byte field.
    #[must_use]
    pub fn byte(&self, name: &str) -> Option<u8> {
        self.scalar(name, DataType::Byte)?.u8()
    }

    /// The value of a scalar character field.
    #[must_use]
    pub fn char(&self, name: &str) -> Option<u8> {
        self.scalar(name, DataType::Char)?.u8()
    }

    /// The value of a scalar real field.
    #[must_use]
    pub fn real(&self, name: &str) -> Option<f64> {
        Some(f64::from_bits(self.scalar(name, DataType::Real)?.u64_be()?))
    }

    /// Whether a scalar real field holds the null real.
    #[must_use]
    pub fn is_null(&self, name: &str) -> Option<bool> {
        let mut view = self.scalar(name, DataType::Real)?;
        Some(view.take(NULL_REAL.len())? == NULL_REAL)
    }

    /// The value of a scalar vector field.
    #[must_use]
    pub fn vector(&self, name: &str) -> Option<[f64; 3]> {
        let mut view = self.scalar(name, DataType::Vector)?;
        let mut out = [0.0; 3];
        for value in &mut out {
            *value = f64::from_bits(view.u64_be()?);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests;
