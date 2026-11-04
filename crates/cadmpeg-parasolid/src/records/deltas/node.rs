// SPDX-License-Identifier: Apache-2.0
//! By-name field access to the node instances of a Deltas stream.

use super::{DeltasRecords, EventContent, Instance};
use crate::records::fields::NULL_REAL;
use crate::records::instance::FieldValue;
use crate::records::DataType;
use cadmpeg_core::decode::View;
use std::ops::Range;

impl DeltasRecords<'_> {
    /// The framed node instances in stream order.
    pub fn nodes(&self) -> impl Iterator<Item = DeltasNode<'_>> {
        self.events()
            .iter()
            .filter_map(|event| match event.content() {
                EventContent::Instance(instance) => Some(DeltasNode {
                    source: self.source(),
                    range: event.range(),
                    instance,
                }),
                _ => None,
            })
    }
}

/// One Deltas node instance with field access by schema name. A pointer field
/// element is a target node index and a status byte.
#[derive(Debug, Clone)]
pub struct DeltasNode<'r> {
    source: &'r [u8],
    range: Range<usize>,
    instance: &'r Instance,
}

impl<'r> DeltasNode<'r> {
    /// Node type.
    #[must_use]
    pub fn kind(&self) -> u16 {
        self.instance.definition().plan().kind
    }

    /// Encoded node index.
    #[must_use]
    pub fn index(&self) -> u32 {
        u32::from(self.instance.xmt())
    }

    /// Byte range of the instance, from its type tag.
    #[must_use]
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }

    /// Byte range from the node index to the end of the instance. A first-use
    /// type declaration precedes it.
    #[must_use]
    pub fn data_range(&self) -> Range<usize> {
        self.instance.identity_at()..self.range.end
    }

    /// Byte range of the named field.
    #[must_use]
    pub fn field_range(&self, name: &str) -> Option<Range<usize>> {
        let index = self.position(name)?;
        self.instance.fields().get(index).cloned()
    }

    /// Every element of the named field, decoded by its schema type.
    #[must_use]
    pub fn values(&self, name: &str) -> Option<Vec<FieldValue>> {
        let index = self.position(name)?;
        let data_type = self
            .instance
            .definition()
            .plan()
            .fields
            .get(index)?
            .data_type;
        let range = self.instance.fields().get(index)?.clone();
        let mut view = View::over_retained(self.source.get(range)?);
        let mut out = Vec::new();
        while view.remaining() > 0 {
            out.push(FieldValue::read(data_type, &mut view)?);
        }
        Some(out)
    }

    /// The target and status byte of a scalar pointer field.
    #[must_use]
    pub fn pointer_status(&self, name: &str) -> Option<(u32, bool)> {
        match self.scalar(name, DataType::Pointer)? {
            FieldValue::Pointer { target, status } => Some((target.get(), status)),
            _ => None,
        }
    }

    /// The target of a scalar pointer field.
    #[must_use]
    pub fn pointer(&self, name: &str) -> Option<u32> {
        self.pointer_status(name).map(|(target, _)| target)
    }

    /// The targets of the named scalar pointer fields, in order.
    #[must_use]
    pub fn pointers<const N: usize>(&self, names: [&str; N]) -> Option<[u32; N]> {
        let mut out = [0; N];
        for (value, name) in out.iter_mut().zip(names) {
            *value = self.pointer(name)?;
        }
        Some(out)
    }

    /// The targets of a variable pointer field.
    #[must_use]
    pub fn pointer_list(&self, name: &str) -> Option<Vec<u32>> {
        self.variable(name, DataType::Pointer)?;
        self.values(name)?
            .into_iter()
            .map(|value| match value {
                FieldValue::Pointer { target, .. } => Some(target.get()),
                _ => None,
            })
            .collect()
    }

    /// The value of a scalar integer field.
    #[must_use]
    pub fn integer(&self, name: &str) -> Option<u32> {
        match self.scalar(name, DataType::Integer)? {
            FieldValue::Integer(value) => Some(value),
            _ => None,
        }
    }

    /// The value of a scalar short field.
    #[must_use]
    pub fn short(&self, name: &str) -> Option<u16> {
        match self.scalar(name, DataType::Short)? {
            FieldValue::Short(value) => Some(value),
            _ => None,
        }
    }

    /// The value of a scalar byte field.
    #[must_use]
    pub fn byte(&self, name: &str) -> Option<u8> {
        match self.scalar(name, DataType::Byte)? {
            FieldValue::Byte(value) => Some(value),
            _ => None,
        }
    }

    /// The value of a scalar character field.
    #[must_use]
    pub fn char(&self, name: &str) -> Option<u8> {
        match self.scalar(name, DataType::Char)? {
            FieldValue::Char(value) => Some(value),
            _ => None,
        }
    }

    /// The value of a scalar logical field.
    #[must_use]
    pub fn logical(&self, name: &str) -> Option<bool> {
        match self.scalar(name, DataType::Logical)? {
            FieldValue::Logical(value) => Some(value),
            _ => None,
        }
    }

    /// The value of a scalar real field.
    #[must_use]
    pub fn real(&self, name: &str) -> Option<f64> {
        let mut view = self.scalar_view(name, DataType::Real)?;
        Some(f64::from_bits(view.u64_be()?))
    }

    /// Whether a scalar real field holds the null real.
    #[must_use]
    pub fn is_null(&self, name: &str) -> Option<bool> {
        let mut view = self.scalar_view(name, DataType::Real)?;
        Some(view.take(NULL_REAL.len())? == NULL_REAL)
    }

    /// The value of a scalar vector field.
    #[must_use]
    pub fn vector(&self, name: &str) -> Option<[f64; 3]> {
        let mut view = self.scalar_view(name, DataType::Vector)?;
        let mut out = [0.0; 3];
        for value in &mut out {
            *value = f64::from_bits(view.u64_be()?);
        }
        Some(out)
    }

    /// The values of a variable real field.
    #[must_use]
    pub fn reals(&self, name: &str) -> Option<Vec<f64>> {
        let mut view = self.variable(name, DataType::Real)?;
        let mut out = Vec::new();
        while view.remaining() > 0 {
            out.push(f64::from_bits(view.u64_be()?));
        }
        Some(out)
    }

    /// A view over the named field when it is a variable field of `data_type`.
    fn variable(&self, name: &str, data_type: DataType) -> Option<View<'r>> {
        let index = self.position(name)?;
        let field = self.instance.definition().plan().fields.get(index)?;
        (field.data_type == data_type && field.n_elements == 1).then_some(())?;
        let range = self.instance.fields().get(index)?.clone();
        Some(View::over_retained(self.source.get(range)?))
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.instance
            .definition()
            .plan()
            .fields
            .iter()
            .position(|field| field.name.as_str() == name)
    }

    /// The single element of a scalar field of `data_type`.
    fn scalar(&self, name: &str, data_type: DataType) -> Option<FieldValue> {
        let mut view = self.scalar_view(name, data_type)?;
        let value = FieldValue::read(data_type, &mut view)?;
        (view.remaining() == 0).then_some(value)
    }

    fn scalar_view(&self, name: &str, data_type: DataType) -> Option<View<'r>> {
        let index = self.position(name)?;
        let field = self.instance.definition().plan().fields.get(index)?;
        (field.data_type == data_type && field.n_elements == 0 && field.transmit).then_some(())?;
        let range = self.instance.fields().get(index)?.clone();
        Some(View::over_retained(self.source.get(range)?))
    }
}

#[cfg(test)]
mod tests;
