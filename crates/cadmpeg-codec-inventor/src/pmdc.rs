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
pub(crate) struct PmDcReferenceList {
    pub(crate) marker: u16,
    pub(crate) metadata: Option<PmDcListMetadata>,
    pub(crate) references: Vec<PmDcReference>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "width", content = "values", rename_all = "snake_case")]
pub(crate) enum PmDcListMetadata {
    U16([u16; 2]),
    U32([u32; 2]),
}

pub(crate) struct Cursor<'a> {
    source: View<'a>,
    position: usize,
}

impl<'a> Cursor<'a> {
    pub(crate) const fn new(source: View<'a>) -> Self {
        Self {
            source,
            position: 0,
        }
    }

    pub(crate) fn remaining(&self) -> usize {
        self.source.window().len().saturating_sub(self.position)
    }

    pub(crate) fn peek_u32(&self, field: &str) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(
            self.source
                .window()
                .get(self.position..self.position.saturating_add(4))
                .ok_or_else(|| CodecError::Malformed(format!("truncated Inventor PmDc {field}")))?
                .try_into()
                .expect("four-byte field"),
        ))
    }

    pub(crate) fn take(&mut self, len: usize, field: &str) -> Result<&'a [u8], CodecError> {
        let end = self.position.checked_add(len).ok_or_else(|| {
            CodecError::Malformed(format!("Inventor PmDc {field} range overflows"))
        })?;
        let value = self
            .source
            .window()
            .get(self.position..end)
            .ok_or_else(|| CodecError::Malformed(format!("truncated Inventor PmDc {field}")))?;
        self.position = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self, field: &str) -> Result<u8, CodecError> {
        Ok(self.take(1, field)?[0])
    }

    pub(crate) fn u16(&mut self, field: &str) -> Result<u16, CodecError> {
        Ok(u16::from_le_bytes(
            self.take(2, field)?.try_into().expect("two-byte field"),
        ))
    }

    pub(crate) fn i16(&mut self, field: &str) -> Result<i16, CodecError> {
        Ok(i16::from_le_bytes(
            self.take(2, field)?.try_into().expect("two-byte field"),
        ))
    }

    pub(crate) fn u32(&mut self, field: &str) -> Result<u32, CodecError> {
        Ok(u32::from_le_bytes(
            self.take(4, field)?.try_into().expect("four-byte field"),
        ))
    }

    pub(crate) fn f64(&mut self, field: &str) -> Result<f64, CodecError> {
        let value = f64::from_le_bytes(self.take(8, field)?.try_into().expect("eight-byte field"));
        if !value.is_finite() {
            return Err(CodecError::Malformed(format!(
                "Inventor PmDc {field} is not finite"
            )));
        }
        Ok(value)
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
            Err(CodecError::Malformed(format!(
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
    let actual = [
        cursor.u16(&format!("{field} marker kind"))?,
        cursor.u16(&format!("{field} marker form"))?,
    ];
    if actual != [marker, 0x3000] {
        return Err(CodecError::Malformed(format!(
            "Inventor PmDc {field} marker is {actual:?}"
        )));
    }
    let count = cursor.u32(&format!("{field} count"))? as usize;
    ctx.charge_collection_items(count as u64, "admit Inventor PmDc references")?;
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
    let mut references = Vec::with_capacity(count);
    for index in 0..count {
        references.push(cursor.reference(&format!("{field} reference {index}"))?);
    }
    Ok(PmDcReferenceList {
        marker,
        metadata,
        references,
    })
}
