// SPDX-License-Identifier: Apache-2.0
//! Common `PmDc` scalar, reference, content-header, and typed-list grammar.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

pub(crate) fn type_id_string(value: [u8; 16]) -> String {
    use std::fmt::Write as _;

    let mut result = String::with_capacity(32);
    for byte in value {
        write!(result, "{byte:02x}").expect("writing to a string cannot fail");
    }
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

    #[cfg(test)]
    pub(crate) fn metadata(&self) -> Option<&PmDcListMetadata> {
        self.items.as_ref().map(|(metadata, _)| metadata)
    }

    pub(crate) fn references(&self) -> &[PmDcReference] {
        self.items
            .as_ref()
            .map(|(_, references)| references.as_slice())
            .unwrap_or(&[])
    }
}

impl From<PmDcReferenceList> for PmDcReferenceListWire {
    fn from(value: PmDcReferenceList) -> Self {
        match value.items {
            None => Self {
                marker: value.marker,
                metadata: None,
                references: Vec::new(),
            },
            Some((metadata, references)) => Self {
                marker: value.marker,
                metadata: Some(metadata),
                references,
            },
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
            .map(|(_, values)| values.as_slice())
            .unwrap_or(&[])
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

    pub(crate) fn peek_u32(&self, _field: &str) -> Result<u32, CodecError> {
        let mut view = self.source;
        Ok(view.req_u32_le()?)
    }

    pub(crate) fn take(&mut self, len: usize, _field: &str) -> Result<&'a [u8], CodecError> {
        Ok(self.source.req_take(len)?)
    }

    pub(crate) fn u8(&mut self, _field: &str) -> Result<u8, CodecError> {
        Ok(self.source.req_u8()?)
    }

    pub(crate) fn u16(&mut self, _field: &str) -> Result<u16, CodecError> {
        Ok(self.source.req_u16_le()?)
    }

    pub(crate) fn i16(&mut self, _field: &str) -> Result<i16, CodecError> {
        Ok(self.source.req_i16_le()?)
    }

    pub(crate) fn u32(&mut self, _field: &str) -> Result<u32, CodecError> {
        Ok(self.source.req_u32_le()?)
    }

    pub(crate) fn i32(&mut self, _field: &str) -> Result<i32, CodecError> {
        Ok(self.source.req_i32_le()?)
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
        let units = self.u32(&format!("{field} length"))? as usize;
        if units > 1_048_576 {
            return Err(CodecError::malformed(format_args!(
                "Inventor PmDc {field} exceeds 1048576 code units"
            )));
        }
        let len = units.checked_mul(2).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor PmDc {field} length overflows"))
        })?;
        ctx.charge_retained(len as u64, "retain Inventor PmDc string", None)?;
        self.source.utf16_le(units).ok_or_else(|| {
            CodecError::malformed(format_args!("Inventor PmDc {field} is not UTF-16"))
        })
    }

    pub(crate) fn reference(&mut self, field: &str) -> Result<PmDcReference, CodecError> {
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
        next: cursor.reference("content next reference")?,
        flags: cursor.u32("content flags")?,
        context: cursor.reference("content context reference")?,
        source_index: cursor.u32("content source index")?,
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
    for index in 0..count {
        references.push(cursor.reference(&format!("{field} reference {index}"))?);
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
    let actual = [
        cursor.u16(&format!("{field} marker kind"))?,
        cursor.u16(&format!("{field} marker form"))?,
    ];
    if actual != [marker, 0x3000] {
        return Err(CodecError::malformed(format_args!(
            "Inventor PmDc {field} marker is {actual:?}"
        )));
    }
    let count = cursor.u32(&format!("{field} count"))? as usize;
    ctx.charge_collection_items(count as u64, admission)?;
    let metadata = if count == 0 {
        None
    } else if marker == 8 {
        Some(PmDcListMetadata::U16([
            cursor.u16(&format!("{field} metadata 0"))?,
            cursor.u16(&format!("{field} metadata 1"))?,
        ]))
    } else {
        Some(PmDcListMetadata::U32([
            cursor.u32(&format!("{field} metadata 0"))?,
            cursor.u32(&format!("{field} metadata 1"))?,
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
    for index in 0..count {
        values.push(cursor.u32(&format!("{field} value {index}"))?);
    }
    PmDcU32List::new(marker, metadata, values).ok_or_else(|| {
        CodecError::Malformed("Inventor PmDc integer list metadata disagrees with length".into())
    })
}
