// SPDX-License-Identifier: Apache-2.0
//! Common `PmDc` scalar, reference, content-header, and typed-list grammar.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

#[cfg(test)]
std::thread_local! {
    pub(crate) static PMDC_LIST_CLONE_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct PmDcListCloneProbe;

#[cfg(test)]
impl Clone for PmDcListCloneProbe {
    fn clone(&self) -> Self {
        PMDC_LIST_CLONE_COUNT.with(|count| count.set(count.get() + 1));
        Self
    }
}

/// Lowercase hexadecimal digits, the only characters a hex rendering holds.
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Render a fixed byte array as lowercase hexadecimal digit pairs.
pub(crate) fn fixed_hex<const N: usize>(
    ctx: &DecodeContext<'_>,
    bytes: &[u8; N],
    operation: &'static str,
) -> Result<String, CodecError> {
    let length = N
        .checked_mul(2)
        .ok_or_else(|| ctx.refuse_codec_limit(operation, u64::MAX, u64::MAX))?;
    let mut text = ctx.retained_string(length, operation)?;
    for byte in bytes {
        text.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
    Ok(text)
}

/// Builds an Inventor type identifier from the `time_low` field of its GUID.
pub(crate) const fn inventor_id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc,
        0x06, 0x63, 0xdc, 0x09,
    ]
}

pub(crate) fn type_id_string(
    ctx: &DecodeContext<'_>,
    value: [u8; 16],
    operation: &'static str,
) -> Result<String, CodecError> {
    fixed_hex(ctx, &value, operation)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcReference {
    index: ReferenceIndex,
    qualified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
struct ReferenceIndex(u32);

impl From<ReferenceIndex> for u32 {
    fn from(value: ReferenceIndex) -> Self {
        value.0
    }
}

impl TryFrom<u32> for ReferenceIndex {
    type Error = &'static str;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value <= 0x7fff_ffff {
            Ok(Self(value))
        } else {
            Err("reference index exceeds 31 bits")
        }
    }
}

impl PmDcReference {
    pub(crate) fn new(index: u32, qualified: bool) -> Option<Self> {
        Some(Self {
            index: ReferenceIndex::try_from(index).ok()?,
            qualified,
        })
    }

    pub(crate) const fn from_packed(value: u32) -> Self {
        Self {
            index: ReferenceIndex(value & 0x7fff_ffff),
            qualified: value & 0x8000_0000 != 0,
        }
    }

    pub(crate) const fn index(self) -> u32 {
        self.index.0
    }

    pub(crate) const fn qualified(self) -> bool {
        self.qualified
    }

    pub(crate) fn zip(
        ctx: &DecodeContext<'_>,
        indices: &[u32],
        qualifiers: &[bool],
    ) -> Result<Vec<Self>, CodecError> {
        if indices.len() != qualifiers.len() {
            return Err(CodecError::malformed(format_args!(
                "reference count {} differs from qualifier count {}",
                indices.len(),
                qualifiers.len()
            )));
        }
        ctx.try_collect_vec(
            indices
                .iter()
                .copied()
                .zip(qualifiers.iter().copied())
                .map(|(index, qualified)| {
                    Self::new(index, qualified)
                        .ok_or_else(|| CodecError::malformed("reference index exceeds 31 bits"))
                }),
            "collect Inventor PmDc references",
        )
    }

    /// The zero-based record ordinal this reference names.
    ///
    /// A `PmDc` reference is one-based. Index 0 is the null reference: it names
    /// no record, and it is not the record at ordinal 0.
    pub(crate) fn record_ordinal(self) -> Option<u32> {
        self.index().checked_sub(1)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcContentHeader {
    pub(crate) header_value: u32,
    pub(crate) header_id: u16,
    pub(crate) next: PmDcReference,
    pub(crate) flags: u32,
    pub(crate) context: PmDcReference,
    pub(crate) source_index: u32,
}

/// A reference list with metadata, carrying the marker its format prefixes it with.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PmDcReferenceListWire")]
pub(crate) struct PmDcReferenceList {
    #[cfg(test)]
    clone_probe: PmDcListCloneProbe,
    marker: u16,
    items: Option<(PmDcListMetadata, Vec<PmDcReference>)>,
}

#[derive(Serialize)]
struct PmDcReferenceListRef<'a> {
    marker: u16,
    metadata: Option<&'a PmDcListMetadata>,
    references: &'a [PmDcReference],
}

impl Serialize for PmDcReferenceList {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (metadata, references) = match self.items.as_ref() {
            None => (None, &[][..]),
            Some((metadata, references)) => (Some(metadata), references.as_slice()),
        };
        PmDcReferenceListRef {
            marker: self.marker,
            metadata,
            references,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct PmDcReferenceListWire {
    marker: u16,
    metadata: Option<PmDcListMetadata>,
    references: Vec<PmDcReference>,
}

impl PmDcReferenceList {
    pub(crate) fn new(
        marker: u16,
        metadata: Option<PmDcListMetadata>,
        references: Vec<PmDcReference>,
    ) -> Option<Self> {
        if metadata
            .as_ref()
            .is_some_and(|value| !value.matches_marker(marker))
        {
            return None;
        }
        let items = match paired_items(metadata, references) {
            PairedItems::Empty => None,
            PairedItems::Complete(metadata, references) => Some((metadata, references)),
            PairedItems::Mismatch => return None,
        };
        Some(Self {
            #[cfg(test)]
            clone_probe: PmDcListCloneProbe,
            marker,
            items,
        })
    }

    pub(crate) fn references(&self) -> &[PmDcReference] {
        self.items
            .as_ref()
            .map_or(&[], |(_, references)| references.as_slice())
    }

    pub(crate) fn into_parts(self) -> (u16, Option<PmDcListMetadata>, Vec<PmDcReference>) {
        match self.items {
            None => (self.marker, None, Vec::new()),
            Some((metadata, references)) => (self.marker, Some(metadata), references),
        }
    }
}

impl From<PmDcReferenceList> for PmDcReferenceListWire {
    fn from(value: PmDcReferenceList) -> Self {
        let (marker, metadata, references) = value.into_parts();
        Self {
            marker,
            metadata,
            references,
        }
    }
}

impl TryFrom<PmDcReferenceListWire> for PmDcReferenceList {
    type Error = String;

    fn try_from(wire: PmDcReferenceListWire) -> Result<Self, Self::Error> {
        Self::new(wire.marker, wire.metadata, wire.references)
            .ok_or_else(|| "PmDc reference list metadata disagrees with length".to_owned())
    }
}

/// A reference list with metadata whose format prefixes it with no marker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PmDcPairedReferenceList<M> {
    #[cfg(test)]
    clone_probe: PmDcListCloneProbe,
    items: Option<(M, Vec<PmDcReference>)>,
}

impl<M> PmDcPairedReferenceList<M> {
    pub(crate) fn new(metadata: Option<M>, references: Vec<PmDcReference>) -> Option<Self> {
        let items = match paired_items(metadata, references) {
            PairedItems::Empty => None,
            PairedItems::Complete(metadata, references) => Some((metadata, references)),
            PairedItems::Mismatch => return None,
        };
        Some(Self {
            #[cfg(test)]
            clone_probe: PmDcListCloneProbe,
            items,
        })
    }

    pub(crate) fn metadata(&self) -> Option<&M> {
        self.items.as_ref().map(|(metadata, _)| metadata)
    }

    pub(crate) fn references(&self) -> &[PmDcReference] {
        self.items
            .as_ref()
            .map_or(&[], |(_, references)| references.as_slice())
    }

    pub(crate) fn try_clone_for_decode(
        &self,
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<Self, CodecError>
    where
        M: Copy,
    {
        let items = match self.items.as_ref() {
            None => None,
            Some((metadata, references)) => {
                Some((*metadata, ctx.copy_slice(references, operation)?))
            }
        };
        Ok(Self {
            #[cfg(test)]
            clone_probe: PmDcListCloneProbe,
            items,
        })
    }

    pub(crate) fn into_references(self) -> Vec<PmDcReference> {
        self.items
            .map(|(_, references)| references)
            .unwrap_or_default()
    }
}

impl<M> Default for PmDcPairedReferenceList<M> {
    fn default() -> Self {
        Self {
            #[cfg(test)]
            clone_probe: PmDcListCloneProbe,
            items: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PmDcU32ListWire")]
pub(crate) struct PmDcU32List {
    #[cfg(test)]
    clone_probe: PmDcListCloneProbe,
    marker: u16,
    items: Option<(PmDcListMetadata, Vec<u32>)>,
}

#[derive(Serialize)]
struct PmDcU32ListRef<'a> {
    marker: u16,
    metadata: Option<&'a PmDcListMetadata>,
    values: &'a [u32],
}

impl Serialize for PmDcU32List {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (metadata, values) = match self.items.as_ref() {
            None => (None, &[][..]),
            Some((metadata, values)) => (Some(metadata), values.as_slice()),
        };
        PmDcU32ListRef {
            marker: self.marker,
            metadata,
            values,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct PmDcU32ListWire {
    marker: u16,
    metadata: Option<PmDcListMetadata>,
    values: Vec<u32>,
}

impl PmDcU32List {
    pub(crate) fn new(
        marker: u16,
        metadata: Option<PmDcListMetadata>,
        values: Vec<u32>,
    ) -> Option<Self> {
        if metadata
            .as_ref()
            .is_some_and(|value| !value.matches_marker(marker))
        {
            return None;
        }
        let items = match paired_items(metadata, values) {
            PairedItems::Empty => None,
            PairedItems::Complete(metadata, values) => Some((metadata, values)),
            PairedItems::Mismatch => return None,
        };
        Some(Self {
            #[cfg(test)]
            clone_probe: PmDcListCloneProbe,
            marker,
            items,
        })
    }

    pub(crate) fn values(&self) -> &[u32] {
        self.items
            .as_ref()
            .map_or(&[][..], |(_, values)| values.as_slice())
    }
}

impl From<PmDcU32List> for PmDcU32ListWire {
    fn from(value: PmDcU32List) -> Self {
        match value.items {
            None => Self {
                marker: value.marker,
                metadata: None,
                values: Vec::new(),
            },
            Some((metadata, values)) => Self {
                marker: value.marker,
                metadata: Some(metadata),
                values,
            },
        }
    }
}

impl TryFrom<PmDcU32ListWire> for PmDcU32List {
    type Error = String;

    fn try_from(wire: PmDcU32ListWire) -> Result<Self, Self::Error> {
        Self::new(wire.marker, wire.metadata, wire.values)
            .ok_or_else(|| "PmDc integer list metadata disagrees with length".to_owned())
    }
}

enum PairedItems<M, T> {
    Empty,
    Complete(M, Vec<T>),
    Mismatch,
}

fn paired_items<M, T>(metadata: Option<M>, values: Vec<T>) -> PairedItems<M, T> {
    match (metadata, values.is_empty()) {
        (None, true) => PairedItems::Empty,
        (Some(metadata), false) => PairedItems::Complete(metadata, values),
        _ => PairedItems::Mismatch,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "width", content = "values", rename_all = "snake_case")]
pub(crate) enum PmDcListMetadata {
    U16([u16; 2]),
    U32([u32; 2]),
}

impl PmDcListMetadata {
    fn matches_marker(&self, marker: u16) -> bool {
        matches!((marker, self), (8, Self::U16(_))) || (marker != 8 && matches!(self, Self::U32(_)))
    }
}

pub(crate) struct Cursor<'a> {
    source: View<'a>,
}

impl<'a> Cursor<'a> {
    pub(crate) const fn new(source: View<'a>) -> Self {
        Self { source }
    }

    pub(crate) fn into_view(self) -> View<'a> {
        self.source
    }

    pub(crate) fn remaining(&self) -> usize {
        self.source.remaining()
    }

    pub(crate) fn peek_u32(&self, field: &'static str) -> Result<u32, CodecError> {
        let mut view = self.source;
        crate::reader::u32(&mut view, field)
    }

    /// Reads a fixed-width byte array.
    pub(crate) fn take_array<const N: usize>(
        &mut self,
        field: &str,
    ) -> Result<[u8; N], CodecError> {
        self.source
            .array()
            .ok_or_else(|| CodecError::malformed(format_args!("truncated Inventor PmDc {field}")))
    }

    pub(crate) fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        crate::reader::u8(&mut self.source, field)
    }

    pub(crate) fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        crate::reader::u16(&mut self.source, field)
    }

    pub(crate) fn i16(&mut self, field: &'static str) -> Result<i16, CodecError> {
        crate::reader::i16(&mut self.source, field)
    }

    pub(crate) fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        crate::reader::u32(&mut self.source, field)
    }

    pub(crate) fn i32(&mut self, field: &'static str) -> Result<i32, CodecError> {
        crate::reader::i32(&mut self.source, field)
    }

    pub(crate) fn u32_array<const N: usize>(
        &mut self,
        field: &'static str,
    ) -> Result<[u32; N], CodecError> {
        crate::reader::u32_array(&mut self.source, field)
    }

    pub(crate) fn f64(
        &mut self,
        field: &str,
    ) -> Result<cadmpeg_ir::scalar::FiniteReal, CodecError> {
        let value = self.source.req_f64_le()?;
        cadmpeg_ir::scalar::FiniteReal::new(value).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor PmDc {field} is not finite"))
        })
    }

    pub(crate) fn utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &str,
    ) -> Result<String, CodecError> {
        let units = usize::try_from(self.u32("string length")?).map_err(|_| {
            CodecError::Malformed("Inventor numeric value exceeds target range".into())
        })?;
        if units > 1_048_576 {
            if self
                .source
                .counted(cadmpeg_core::decode::u64_from_index(units), 2)
                .is_none()
            {
                return Err(CodecError::malformed(format_args!(
                    "Inventor PmDc {field} UTF-16 payload is truncated"
                )));
            }
            return Err(ctx.refuse_codec_limit(
                "Inventor PmDc UTF-16 code units",
                1_048_576,
                cadmpeg_core::decode::u64_from_index(units),
            ));
        }
        crate::reader::utf16_text(
            ctx,
            &mut self.source,
            units,
            "PmDc text",
            "retain Inventor PmDc string",
        )
    }

    pub(crate) fn reference(&mut self, field: &'static str) -> Result<PmDcReference, CodecError> {
        crate::reader::pmdc_reference(&mut self.source, field)
    }

    pub(crate) fn finish(&self, record: &str) -> Result<(), CodecError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(CodecError::malformed(format_args!(
                "Inventor PmDc {record} has {} trailing bytes",
                self.remaining()
            )))
        }
    }
}

pub(crate) fn content_header(cursor: &mut Cursor<'_>) -> Result<PmDcContentHeader, CodecError> {
    Ok(PmDcContentHeader {
        header_value: cursor.u32("content header value")?,
        header_id: cursor.u16("content header id")?,
        next: cursor.reference("content header next reference")?,
        flags: cursor.u32("content header flags")?,
        context: cursor.reference("content header context reference")?,
        source_index: cursor.u32("content header source index")?,
    })
}

pub(crate) fn reference_list(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    marker: u16,
    field: &str,
) -> Result<PmDcReferenceList, CodecError> {
    let (count, metadata) = list_preamble(cursor, marker, field)?;
    let mut references = ctx.vector_storage(count, "admit Inventor PmDc references")?;
    let mut entry_steps = 0..count;
    while ctx
        .next_charged(&mut entry_steps, "visit Inventor PmDc list entries")?
        .is_some()
    {
        ctx.push_vec(
            &mut references,
            cursor.reference("reference-list entry")?,
            "admit Inventor PmDc references",
        )?;
    }
    PmDcReferenceList::new(marker, metadata, references).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc reference list metadata disagrees with length".into())
    })
}

fn list_preamble(
    cursor: &mut Cursor<'_>,
    marker: u16,
    field: &str,
) -> Result<(usize, Option<PmDcListMetadata>), CodecError> {
    let actual = [cursor.u16("list marker 0")?, cursor.u16("list marker 1")?];
    if actual != [marker, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} marker is {actual:?}"
        )));
    }
    let count = usize::try_from(cursor.u32("list count")?)
        .map_err(|_| CodecError::Malformed("Inventor numeric value exceeds target range".into()))?;
    let metadata = if count == 0 {
        None
    } else if marker == 8 {
        Some(PmDcListMetadata::U16([
            cursor.u16("list metadata 0")?,
            cursor.u16("list metadata 1")?,
        ]))
    } else {
        Some(PmDcListMetadata::U32([
            cursor.u32("list metadata 0")?,
            cursor.u32("list metadata 1")?,
        ]))
    };
    cursor
        .source
        .counted(cadmpeg_core::decode::u64_from_index(count), 4)
        .ok_or_else(|| {
            CodecError::malformed("Inventor PmDc list count exceeds remaining payload")
        })?;
    Ok((count, metadata))
}

pub(crate) fn u32_list(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    marker: u16,
    field: &str,
) -> Result<PmDcU32List, CodecError> {
    let (count, metadata) = list_preamble(cursor, marker, field)?;
    let mut values = ctx.vector_storage(count, "admit Inventor PmDc integers")?;
    let mut entry_steps = 0..count;
    while ctx
        .next_charged(&mut entry_steps, "visit Inventor PmDc list entries")?
        .is_some()
    {
        ctx.push_vec(
            &mut values,
            cursor.u32("integer-list value")?,
            "admit Inventor PmDc integers",
        )?;
    }
    PmDcU32List::new(marker, metadata, values).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc integer list metadata disagrees with length".into())
    })
}

type PairedMapItems<V> = ([u32; 2], Vec<(PmDcReference, V)>);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(
    try_from = "PmDcPairedMapWire<V>",
    bound(deserialize = "V: Deserialize<'de>")
)]
pub(crate) struct PmDcPairedMap<V> {
    #[cfg(test)]
    clone_probe: PmDcListCloneProbe,
    items: Option<PairedMapItems<V>>,
}

#[derive(Serialize)]
struct PmDcPairedMapRef<'a, V> {
    metadata: Option<[u32; 2]>,
    entries: &'a [(PmDcReference, V)],
}

impl<V: Serialize> Serialize for PmDcPairedMap<V> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PmDcPairedMapRef {
            metadata: self.metadata(),
            entries: self.entries(),
        }
        .serialize(serializer)
    }
}

#[derive(Serialize, Deserialize)]
struct PmDcPairedMapWire<V> {
    metadata: Option<[u32; 2]>,
    entries: Vec<(PmDcReference, V)>,
}

impl<V> PmDcPairedMap<V> {
    pub(crate) fn new(
        metadata: Option<[u32; 2]>,
        entries: Vec<(PmDcReference, V)>,
    ) -> Option<Self> {
        let items = match paired_items(metadata, entries) {
            PairedItems::Empty => None,
            PairedItems::Complete(metadata, entries) => Some((metadata, entries)),
            PairedItems::Mismatch => return None,
        };
        Some(Self {
            #[cfg(test)]
            clone_probe: PmDcListCloneProbe,
            items,
        })
    }

    /// The map that carries no metadata and no entries.
    pub(crate) fn empty() -> Self {
        Self {
            #[cfg(test)]
            clone_probe: PmDcListCloneProbe,
            items: None,
        }
    }

    pub(crate) fn metadata(&self) -> Option<[u32; 2]> {
        self.items.as_ref().map(|(metadata, _)| *metadata)
    }

    pub(crate) fn entries(&self) -> &[(PmDcReference, V)] {
        self.items
            .as_ref()
            .map_or(&[][..], |(_, entries)| entries.as_slice())
    }
}

impl<V> From<PmDcPairedMap<V>> for PmDcPairedMapWire<V> {
    fn from(value: PmDcPairedMap<V>) -> Self {
        match value.items {
            None => Self {
                metadata: None,
                entries: Vec::new(),
            },
            Some((metadata, entries)) => Self {
                metadata: Some(metadata),
                entries,
            },
        }
    }
}

impl<V> TryFrom<PmDcPairedMapWire<V>> for PmDcPairedMap<V> {
    type Error = String;

    fn try_from(wire: PmDcPairedMapWire<V>) -> Result<Self, Self::Error> {
        Self::new(wire.metadata, wire.entries)
            .ok_or_else(|| "PmDc map metadata disagrees with length".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{content_header, reference_list, Cursor};
    use crate::test_support::truncation::displayed_truncation;
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};
    use cadmpeg_core::decode::{DecodeContext, View};
    use cadmpeg_core::CodecError;

    #[test]
    fn fixed_type_identifier_admits_only_retained_storage() {
        let arena = DecodeArena::new();
        for cap in [0, 31, 32] {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            policy.limits.max_retained_bytes = cap;
            policy.limits.max_materialized_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("fixed identifier context");
            let result = super::type_id_string(&ctx, [0xaf; 16], "fixed type identifier");
            if cap < 32 {
                let error = result.expect_err("fixed identifier needs 32 retained bytes");
                assert!(matches!(error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == "fixed type identifier"
                        && limit.additional == 32
                        && Some(limit) == ctx.resource_refusal()));
            } else {
                assert_eq!(
                    result.expect("fixed formatting has no input-sized work"),
                    "afafafafafafafafafafafafafafafaf"
                );
            }
        }
    }

    #[test]
    fn paired_reference_copy_does_not_charge_fixed_metadata_work() {
        let references = vec![super::PmDcReference::from_packed(1)];
        let list =
            super::PmDcPairedReferenceList::new(Some([7_u16, 9]), references).expect("paired list");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // copy_slice visits the one reference. Copying the two metadata
        // words has a fixed extent and needs no source traversal charge.
        policy.limits.max_work_units = 1;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("paired copy context");
        let copy = list
            .try_clone_for_decode(&ctx, "copy paired references")
            .expect("only reference-sized work");
        assert_eq!(copy.metadata(), Some(&[7_u16, 9]));
        assert_eq!(copy.references(), list.references());
    }

    #[test]
    fn pmdc_utf16_local_ceiling_refuses_resources_for_complete_payload() {
        let units = 1_048_577_u32;
        let mut bytes = units.to_le_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(
            0_u8,
            usize::try_from(units).expect("length") * 2,
        ));
        let arena = DecodeArena::new();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let error = Cursor::new(root)
            .utf16(&ctx, "name")
            .expect_err("local string ceiling");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.operation == "Inventor PmDc UTF-16 code units"
                && limit.limit == 1_048_576 && Some(limit) == ctx.resource_refusal()));
        let truncated = units.to_le_bytes();
        let (ctx, root) =
            DecodeContext::from_root_bytes(&truncated, &arena, &DecodePolicy::service())
                .expect("root");
        assert!(matches!(
            Cursor::new(root).utf16(&ctx, "name"),
            Err(CodecError::Malformed(_))
        ));
    }

    #[test]
    fn typed_lists_reject_marker_metadata_width_disagreement() {
        let reference = super::PmDcReference::from_packed(1);
        for (marker, metadata) in [
            (8, super::PmDcListMetadata::U32([0; 2])),
            (2, super::PmDcListMetadata::U16([0; 2])),
        ] {
            assert!(
                super::PmDcReferenceList::new(marker, Some(metadata.clone()), vec![reference])
                    .is_none()
            );
            assert!(super::PmDcU32List::new(marker, Some(metadata.clone()), vec![1]).is_none());
            let wire = serde_json::json!({"marker": marker, "metadata": metadata, "references": [reference]});
            assert!(serde_json::from_value::<super::PmDcReferenceList>(wire).is_err());
            let wire = serde_json::json!({"marker": marker, "metadata": metadata, "values": [1]});
            assert!(serde_json::from_value::<super::PmDcU32List>(wire).is_err());
        }
    }

    #[test]
    fn references_reject_high_indices_on_every_construction_path() {
        assert!(super::PmDcReference::new(0x8000_0000, false).is_none());
        assert!(super::PmDcReference::zip(
            &cadmpeg_test_support::service_decode_context(),
            &[0x8000_0000],
            &[false],
        )
        .is_err());
        assert!(serde_json::from_value::<super::PmDcReference>(
            serde_json::json!({"index": 2_147_483_648_u32, "qualified": false})
        )
        .is_err());
        for (packed, index, qualified) in [
            (0, 0, false),
            (0x8000_0000, 0, true),
            (u32::MAX, 0x7fff_ffff, true),
        ] {
            let reference = super::PmDcReference::from_packed(packed);
            assert_eq!(
                (reference.index(), reference.qualified()),
                (index, qualified)
            );
            assert_eq!(
                serde_json::to_value(reference).expect("reference wire"),
                serde_json::json!({"index": index, "qualified": qualified})
            );
        }
    }

    #[test]
    fn pmdc_counted_lists_prove_extent_before_admission() {
        for marker in [2_u16, 8] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&marker.to_le_bytes());
            bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
            bytes.extend_from_slice(&1_000_000_u32.to_le_bytes());
            bytes.extend_from_slice(if marker == 8 { &[0; 4] } else { &[0; 8] });
            crate::test_support::test_fixtures::parse(&bytes, |ctx, root| {
                assert!(matches!(
                    reference_list(ctx, &mut Cursor::new(root), marker, "test"),
                    Err(CodecError::Malformed(_))
                ));
                assert!(matches!(
                    super::u32_list(ctx, &mut Cursor::new(root), marker, "test"),
                    Err(CodecError::Malformed(_))
                ));
            });
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = 0;
            let (ctx, root) =
                DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
            assert!(matches!(
                reference_list(&ctx, &mut Cursor::new(root), marker, "test"),
                Err(CodecError::Malformed(_))
            ));
            assert!(matches!(
                super::u32_list(&ctx, &mut Cursor::new(root), marker, "test"),
                Err(CodecError::Malformed(_))
            ));
        }
    }

    #[test]
    fn pmdc_counted_lists_admit_retained_storage() {
        let bytes = [
            2_u8, 0, 0, 0x30, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
        ];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("test context");
        assert!(
            matches!(reference_list(&ctx, &mut Cursor::new(root), 2, "test"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
        assert!(
            matches!(super::u32_list(&ctx, &mut Cursor::new(root), 2, "test"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::RetainedBytes)
        );
    }

    #[test]
    fn reference_zip_charges_one_source_step_per_pair() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Two pairs plus the collector's end probe: 2 + 1 = 3 work units.
        policy.limits.max_work_units = 3;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("zip context");
        let references = super::PmDcReference::zip(&ctx, &[1, 2], &[false, true])
            .expect("one traversal fits");
        assert_eq!(references[0].index(), 1);
        assert_eq!(references[1].index(), 2);
        assert!(references[1].qualified());
        assert!(matches!(ctx.charge_work(1, "probe reference zip"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::WorkUnits && limit.used == 3));
    }

    #[test]
    fn pmdc_counted_lists_refuse_only_the_next_item_step() {
        for count in [1_u32, 512] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&2_u16.to_le_bytes());
            bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
            bytes.resize(
                bytes.len() + usize::try_from(count).expect("fixture count") * 4,
                0,
            );
            for integers in [false, true] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = 0;
                let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                    .expect("PmDc counted-list context");
                let result = if integers {
                    super::u32_list(&ctx, &mut Cursor::new(view), 2, "test").map(|_| ())
                } else {
                    reference_list(&ctx, &mut Cursor::new(view), 2, "test").map(|_| ())
                };
                let error = result.expect_err("first list step refuses");
                assert!(matches!(&error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::WorkUnits
                        && limit.operation == "visit Inventor PmDc list entries"
                        && limit.used == 0
                        && limit.additional == 1));
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
                    if matches!(&error, CodecError::ResourceLimit(original) if original == &limit))
                );
            }
        }
    }

    #[test]
    fn pmdc_counted_lists_admit_each_item_and_the_end_probe() {
        for count in [0_u32, 1, 512] {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&2_u16.to_le_bytes());
            bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
            if count != 0 {
                bytes.extend_from_slice(&[0; 8]);
            }
            bytes.resize(
                bytes.len() + usize::try_from(count).expect("fixture count") * 4,
                0,
            );
            for integers in [false, true] {
                for end_probe in [0_u64, 1] {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_materialized_bytes = 0;
                    policy.limits.max_work_units = u64::from(count) + end_probe;
                    let (ctx, view) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
                        .expect("PmDc counted-list context");
                    let mut cursor = Cursor::new(view);
                    let result = if integers {
                        super::u32_list(&ctx, &mut cursor, 2, "test")
                            .map(|list| list.values().len())
                    } else {
                        reference_list(&ctx, &mut cursor, 2, "test")
                            .map(|list| list.references().len())
                    };
                    if end_probe == 0 {
                        let error = result.expect_err("list end probe refuses");
                        assert!(matches!(&error, CodecError::ResourceLimit(limit)
                            if limit.dimension == ResourceDimension::WorkUnits
                                && limit.operation == "visit Inventor PmDc list entries"
                                && limit.used == u64::from(count)
                                && limit.additional == 1));
                        assert!(
                            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
                            if matches!(&error, CodecError::ResourceLimit(original) if original == &limit))
                        );
                    } else {
                        assert_eq!(
                            result.expect("items and end probe fit"),
                            usize::try_from(count).expect("fixture count")
                        );
                        cursor.finish("test list").expect("all list bytes consumed");
                        ctx.finish_session().expect("list work fits exactly");
                    }
                }
            }
        }
    }

    #[test]
    fn pmdc_utf16_refuses_exact_utf8_retained_limit_before_decode() {
        let bytes = [1_u8, 0, 0, 0, 0xac, 0x20];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("PmDc string fits input cap");
        assert!(matches!(
            Cursor::new(root).utf16(&ctx, "name"),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::RetainedBytes
                    && limit.operation == "retain Inventor PmDc string"
        ));

        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("PmDc string fits service policy");
        assert_eq!(
            Cursor::new(root)
                .utf16(&ctx, "name")
                .expect("valid PmDc string"),
            "€"
        );
    }

    #[test]
    fn pmdc_utf16_needs_no_materialized_code_units() {
        let bytes = [1_u8, 0, 0, 0, b'A', 0];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("PmDc string fits input cap");
        assert_eq!(
            Cursor::new(root)
                .utf16(&ctx, "name")
                .expect("direct UTF-16 decode needs no temporary storage"),
            "A"
        );
    }

    #[test]
    fn truncated_scalar_reads_name_the_field() {
        let empty = &[];
        for (field, text) in [
            (
                "unit visibility",
                displayed_truncation(Cursor::new(View::over_retained(empty)).u8("unit visibility")),
            ),
            (
                "parameter tolerance",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).u16("parameter tolerance"),
                ),
            ),
            (
                "parameter terminal value",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).i16("parameter terminal value"),
                ),
            ),
            (
                "sketch state",
                displayed_truncation(Cursor::new(View::over_retained(empty)).u32("sketch state")),
            ),
            (
                "edge-item index reference value",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).i32("edge-item index reference value"),
                ),
            ),
            (
                "transform prefix",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty)).peek_u32("transform prefix"),
                ),
            ),
            (
                "content header next reference",
                displayed_truncation(
                    Cursor::new(View::over_retained(empty))
                        .reference("content header next reference"),
                ),
            ),
        ] {
            assert_eq!(
                text,
                format!("truncated input during {field} at space 0 offset 0")
            );
        }
    }

    #[test]
    fn a_truncated_content_header_names_the_field_it_stopped_in() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&7_u32.to_le_bytes());
        bytes.extend_from_slice(&9_u16.to_le_bytes());
        let mut cursor = Cursor::new(View::over_retained(&bytes));
        assert_eq!(
            displayed_truncation(content_header(&mut cursor)),
            "truncated input during content header next reference at space 0 offset 6"
        );
    }

    #[test]
    fn a_truncated_typed_list_names_its_preamble_field() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&8_u16.to_le_bytes());
        bytes.extend_from_slice(&0x3000_u16.to_le_bytes());
        let arena = DecodeArena::new();
        let text = match DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::default()) {
            Ok((ctx, root)) => {
                let mut cursor = Cursor::new(root);
                displayed_truncation(reference_list(&ctx, &mut cursor, 8, "sketch entity array"))
            }
            Err(error) => error.to_string(),
        };
        assert_eq!(
            text,
            "truncated input during list count at space 0 offset 4"
        );
    }

    mod serialization;
}
