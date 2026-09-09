// SPDX-License-Identifier: Apache-2.0
//! Paged logical-record framing.

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::layout::{continuation_page, instance_stream_header, record_start_page, terminal_page};
use crate::{CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER};

/// One exact logical record recovered from the `InstanceProperties` page
/// framing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordFrame {
    /// Byte offset in the dechunked logical stream.
    logical_offset: usize,
    /// Complete record bytes, including the opening marker.
    bytes: Vec<u8>,
}

impl RecordFrame {
    /// Returns the record offset in the dechunked logical stream.
    pub const fn logical_offset(&self) -> usize {
        self.logical_offset
    }

    /// Returns the complete record bytes, including the opening marker.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Split a paged `InstanceProperties` stream into logical records.
///
/// The stream is a [`STREAM_HEADER_LEN`]-byte header followed by fixed
/// [`PAGE_SIZE`] pages. A page whose bytes 4..8 hold [`RECORD_MARKER`] opens a
/// record, [`CONTINUATION_MARKER`] extends it, and a page opening with
/// [`TERMINAL_MARKER`] closes it and carries the used byte count as a `u16` at
/// offset 4. Every record is returned with the opening marker restored so
/// record offsets match the on-page layout.
pub fn record_frames(bytes: &[u8]) -> Result<Vec<RecordFrame>, CodecError> {
    if bytes.len() < STREAM_HEADER_LEN + PAGE_SIZE {
        return Err(CodecError::Malformed(
            "Protein page stream is shorter than its header and one page".into(),
        ));
    }
    if View::u32_le_at(bytes, instance_stream_header::DECLARED_SIZE) != Some(PAGE_SIZE as u32) {
        return Err(CodecError::Malformed(
            "Protein declared page size is invalid".into(),
        ));
    }
    if !(bytes.len() - STREAM_HEADER_LEN).is_multiple_of(PAGE_SIZE) {
        return Err(CodecError::Malformed(
            "Protein page stream has a partial trailing page".into(),
        ));
    }
    let mut records = Vec::new();
    let mut current: Option<RecordFrame> = None;
    let mut logical_offset = 0usize;
    for page in bytes[STREAM_HEADER_LEN..].chunks_exact(PAGE_SIZE) {
        if page.get(record_start_page::MARKER..record_start_page::BODY) == Some(RECORD_MARKER) {
            if let Some(record) = current.take() {
                logical_offset =
                    logical_offset
                        .checked_add(record.bytes.len())
                        .ok_or_else(|| {
                            CodecError::Malformed(
                                "Protein record-start logical offset overflow".into(),
                            )
                        })?;
                records.push(record);
            }
            let mut frame = RecordFrame {
                logical_offset,
                bytes: RECORD_MARKER.to_vec(),
            };
            frame
                .bytes
                .extend_from_slice(&page[record_start_page::BODY..]);
            current = Some(frame);
        } else if page.get(continuation_page::MARKER..continuation_page::BODY)
            == Some(CONTINUATION_MARKER)
        {
            current
                .as_mut()
                .ok_or_else(|| {
                    CodecError::Malformed("Protein continuation page has no open record".into())
                })?
                .bytes
                .extend_from_slice(&page[continuation_page::BODY..]);
        } else if page.get(terminal_page::MARKER..terminal_page::USED) == Some(TERMINAL_MARKER) {
            let used = View::u16_le_at(page, terminal_page::USED).ok_or_else(|| {
                CodecError::Malformed("Protein terminal used-byte count is truncated".into())
            })? as usize;
            let mut frame = current.take().unwrap_or_else(|| RecordFrame {
                logical_offset,
                bytes: RECORD_MARKER.to_vec(),
            });
            frame.bytes.extend_from_slice(
                page.get(terminal_page::BODY..terminal_page::BODY + used)
                    .ok_or_else(|| {
                        CodecError::Malformed("Protein terminal payload is truncated".into())
                    })?,
            );
            logical_offset = logical_offset
                .checked_add(frame.bytes.len())
                .ok_or_else(|| {
                    CodecError::Malformed("Protein terminal logical offset overflow".into())
                })?;
            records.push(frame);
        } else {
            return Err(CodecError::Malformed(
                "Protein page marker is unknown".into(),
            ));
        }
    }
    if let Some(record) = current {
        records.push(record);
    }
    Ok(records)
}
