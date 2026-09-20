// SPDX-License-Identifier: Apache-2.0
//! Locate payload bytes when editing binary record fixtures.

use crate::kernel_header::RefWidth;
use crate::sab::{lex, Lexed, Record};
use crate::stream_error::{StreamError, StreamFormat};

/// Byte offsets of payload tokens with `tag` inside one framed record.
pub fn payload_token_offsets(
    bytes: &[u8],
    record: &Record,
    ref_width: RefWidth,
    tag: u8,
) -> Result<Vec<usize>, StreamError> {
    let end = record
        .offset
        .checked_add(record.len)
        .ok_or_else(|| StreamError {
            format: StreamFormat::Binary,
            offset: record.offset,
            reason: "record byte extent overflows".to_owned(),
        })?;
    let bytes = bytes.get(..end).ok_or_else(|| StreamError {
        format: StreamFormat::Binary,
        offset: record.offset,
        reason: "record byte extent exceeds the available stream".to_owned(),
    })?;
    let mut position = record.offset;
    let mut offsets = Vec::new();
    while position < end {
        let token_offset = position;
        let (token, next) = lex(bytes, position, ref_width)?;
        if bytes[token_offset] == tag && matches!(&token, Lexed::Value(_)) {
            offsets.push(token_offset);
        }
        position = next;
        if matches!(&token, Lexed::Terminator) {
            break;
        }
    }
    Ok(offsets)
}
