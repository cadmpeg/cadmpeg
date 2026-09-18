// SPDX-License-Identifier: Apache-2.0
//! Common `PmDc` scalar, reference, content-header, and typed-list grammar.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

/// Lowercase hexadecimal digits, the only characters a hex rendering holds.
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Append `bytes` to `text` as lowercase hexadecimal digit pairs.
pub(crate) fn push_hex(text: &mut String, bytes: &[u8]) {
    for byte in bytes {
        text.push(char::from(HEX_DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(HEX_DIGITS[usize::from(byte & 0x0f)]));
    }
}

/// Builds an Inventor type identifier from the `time_low` field of its GUID.
pub(crate) const fn inventor_id(time_low: u32) -> [u8; 16] {
    let first = time_low.to_le_bytes();
    [
        first[0], first[1], first[2], first[3], 0xd0, 0x11, 0xf8, 0xd1, 0x00, 0x08, 0xca, 0xbc,
        0x06, 0x63, 0xdc, 0x09,
    ]
}

pub(crate) fn type_id_string(value: [u8; 16]) -> String {
    let mut result = String::with_capacity(32);
    push_hex(&mut result, &value);
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PmDcReference {
    pub(crate) index: u32,
    pub(crate) qualified: bool,
}

impl PmDcReference {
    pub(crate) fn zip(indices: Vec<u32>, qualifiers: Vec<bool>) -> Result<Vec<Self>, String> {
        if indices.len() != qualifiers.len() {
            return Err(format!(
                "reference count {} differs from qualifier count {}",
                indices.len(),
                qualifiers.len()
            ));
        }
        Ok(indices
            .into_iter()
            .zip(qualifiers)
            .map(|(index, qualified)| Self { index, qualified })
            .collect())
    }

    /// The zero-based record ordinal this reference names.
    ///
    /// A `PmDc` reference is one-based. Index 0 is the null reference: it names
    /// no record, and it is not the record at ordinal 0.
    pub(crate) fn record_ordinal(self) -> Option<u32> {
        self.index.checked_sub(1)
    }

    pub(crate) fn unzip(refs: &[Self]) -> (Vec<u32>, Vec<bool>) {
        refs.iter().map(|r| (r.index, r.qualified)).unzip()
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PmDcReferenceListWire", into = "PmDcReferenceListWire")]
pub(crate) struct PmDcReferenceList {
    pub(crate) marker: u16,
    items: Option<(PmDcListMetadata, Vec<PmDcReference>)>,
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
        Some(Self {
            marker,
            items: paired_items(metadata, references)?,
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
    items: Option<(M, Vec<PmDcReference>)>,
}

impl<M> PmDcPairedReferenceList<M> {
    pub(crate) fn new(metadata: Option<M>, references: Vec<PmDcReference>) -> Option<Self> {
        Some(Self {
            items: paired_items(metadata, references)?,
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

    pub(crate) fn into_references(self) -> Vec<PmDcReference> {
        self.items
            .map(|(_, references)| references)
            .unwrap_or_default()
    }
}

impl<M> Default for PmDcPairedReferenceList<M> {
    fn default() -> Self {
        Self { items: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PmDcU32ListWire", into = "PmDcU32ListWire")]
pub(crate) struct PmDcU32List {
    pub(crate) marker: u16,
    items: Option<(PmDcListMetadata, Vec<u32>)>,
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
        Some(Self {
            marker,
            items: paired_items(metadata, values)?,
        })
    }

    pub(crate) fn values(&self) -> &[u32] {
        self.items
            .as_ref()
            .map_or(&[] as &[_], |(_, values)| values.as_slice())
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

// The outer option reports a mismatched metadata/list pair; the inner option is an empty list.
#[allow(clippy::option_option)]
pub(crate) fn paired_items<M, T>(
    metadata: Option<M>,
    values: Vec<T>,
) -> Option<Option<(M, Vec<T>)>> {
    match (metadata, values.is_empty()) {
        (None, true) => Some(None),
        (Some(metadata), false) => Some(Some((metadata, values))),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "width", content = "values", rename_all = "snake_case")]
pub(crate) enum PmDcListMetadata {
    U16([u16; 2]),
    U32([u32; 2]),
}

pub(crate) struct Cursor<'a> {
    source: View<'a>,
}

impl<'a> Cursor<'a> {
    pub(crate) const fn new(source: View<'a>) -> Self {
        Self { source }
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

    pub(crate) fn f64(&mut self, field: &str) -> Result<f64, CodecError> {
        let value = self.source.req_f64_le()?;
        if !value.is_finite() {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} is not finite"
            )));
        }
        Ok(value)
    }

    pub(crate) fn utf16(
        &mut self,
        ctx: &DecodeContext<'_>,
        field: &str,
    ) -> Result<String, CodecError> {
        let units = self.u32("string length")? as usize;
        if units > 1_048_576 {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} exceeds 1048576 code units"
            )));
        }
        let len = units.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor PmDc {field} length overflows"))
        })?;
        ctx.charge_retained(len as u64, "retain Inventor PmDc string")?;
        self.source.utf16_le(units).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor PmDc {field} is not UTF-16"))
        })
    }

    pub(crate) fn reference(&mut self, field: &'static str) -> Result<PmDcReference, CodecError> {
        let value = self.u32(field)?;
        Ok(PmDcReference {
            index: value & 0x7fff_ffff,
            qualified: value & 0x8000_0000 != 0,
        })
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
    let (count, metadata) =
        list_preamble(ctx, cursor, marker, field, "admit Inventor PmDc references")?;
    let mut references = Vec::with_capacity(count);
    for _ in 0..count {
        references.push(cursor.reference("reference-list entry")?);
    }
    PmDcReferenceList::new(marker, metadata, references).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc reference list metadata disagrees with length".into())
    })
}

fn list_preamble(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    marker: u16,
    field: &str,
    admission: &'static str,
) -> Result<(usize, Option<PmDcListMetadata>), CodecError> {
    let actual = [cursor.u16("list marker 0")?, cursor.u16("list marker 1")?];
    if actual != [marker, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} marker is {actual:?}"
        )));
    }
    let count = cursor.u32("list count")? as usize;
    ctx.charge_collection_items(count as u64, admission)?;
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
    Ok((count, metadata))
}

pub(crate) fn u32_list(
    ctx: &DecodeContext<'_>,
    cursor: &mut Cursor<'_>,
    marker: u16,
    field: &str,
) -> Result<PmDcU32List, CodecError> {
    let (count, metadata) =
        list_preamble(ctx, cursor, marker, field, "admit Inventor PmDc integers")?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        values.push(cursor.u32("integer-list value")?);
    }
    PmDcU32List::new(marker, metadata, values).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc integer list metadata disagrees with length".into())
    })
}

pub(crate) fn unique_by<'a, T, K: Eq + std::hash::Hash>(
    records: &'a [T],
    key: impl Fn(&'a T) -> K,
) -> std::collections::HashMap<K, &'a T> {
    let mut unique = std::collections::HashMap::new();
    for record in records {
        unique
            .entry(key(record))
            .and_modify(|value| *value = None)
            .or_insert(Some(record));
    }
    unique
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .collect()
}

type PairedMapItems<V> = ([u32; 2], Vec<(PmDcReference, V)>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "PmDcPairedMapWire<V>",
    into = "PmDcPairedMapWire<V>",
    bound(
        serialize = "V: Serialize + Clone",
        deserialize = "V: Deserialize<'de>"
    )
)]
pub(crate) struct PmDcPairedMap<V> {
    items: Option<PairedMapItems<V>>,
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
        Some(Self {
            items: paired_items(metadata, entries)?,
        })
    }

    /// The map that carries no metadata and no entries.
    pub(crate) fn empty() -> Self {
        Self { items: None }
    }

    pub(crate) fn metadata(&self) -> Option<[u32; 2]> {
        self.items.as_ref().map(|(metadata, _)| *metadata)
    }

    pub(crate) fn entries(&self) -> &[(PmDcReference, V)] {
        self.items
            .as_ref()
            .map_or(&[] as &[_], |(_, entries)| entries.as_slice())
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
    use super::*;
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy};

    /// The diagnostic a truncated read produces, without an unwrap on the route.
    fn truncation<T: std::fmt::Debug>(result: Result<T, CodecError>) -> String {
        match result {
            Ok(value) => format!("the read succeeded with {value:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn truncated_scalar_reads_name_the_field() {
        let empty = &[];
        for (field, text) in [
            (
                "unit visibility",
                truncation(Cursor::new(View::over_retained(empty)).u8("unit visibility")),
            ),
            (
                "parameter tolerance",
                truncation(Cursor::new(View::over_retained(empty)).u16("parameter tolerance")),
            ),
            (
                "parameter terminal value",
                truncation(Cursor::new(View::over_retained(empty)).i16("parameter terminal value")),
            ),
            (
                "sketch state",
                truncation(Cursor::new(View::over_retained(empty)).u32("sketch state")),
            ),
            (
                "edge-item index reference value",
                truncation(
                    Cursor::new(View::over_retained(empty)).i32("edge-item index reference value"),
                ),
            ),
            (
                "transform prefix",
                truncation(Cursor::new(View::over_retained(empty)).peek_u32("transform prefix")),
            ),
            (
                "content header next reference",
                truncation(
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
            truncation(content_header(&mut cursor)),
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
                truncation(reference_list(&ctx, &mut cursor, 8, "sketch entity array"))
            }
            Err(error) => error.to_string(),
        };
        assert_eq!(
            text,
            "truncated input during list count at space 0 offset 4"
        );
    }
}
