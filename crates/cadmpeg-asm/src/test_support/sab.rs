// SPDX-License-Identifier: Apache-2.0
//! Locate payload bytes when editing binary record fixtures.

use crate::kernel_header::RefWidth;
use crate::sab::{lex, Lexed, Record};
use crate::stream_error::{StreamError, StreamFailure, StreamFormat};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Frame one fixture with the service decode policy.
pub fn frame(
    bytes: &[u8],
    start: usize,
    limit: usize,
    width: RefWidth,
) -> Result<Vec<Record>, StreamError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .map_err(|error| StreamError {
            format: StreamFormat::Binary,
            offset: start,
            reason: format!("fixture exceeds the service input limit: {error}"),
        })?;
    match crate::sab::frame(&ctx, bytes, start, limit, width) {
        Ok(records) => Ok(records),
        Err(
            StreamFailure::Parse(error)
            | StreamFailure::Malformed(error)
            | StreamFailure::NotImplemented(error),
        ) => Err(error),
        Err(StreamFailure::Resource(error)) => Err(StreamError {
            format: StreamFormat::Binary,
            offset: start,
            reason: format!("fixture exhausted a resource: {error}"),
        }),
    }
}

/// Frame a fixture whose final record ends at the enclosing byte boundary.
pub fn frame_history(
    bytes: &[u8],
    start: usize,
    limit: usize,
    width: RefWidth,
) -> Result<Vec<Record>, StreamError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .map_err(|error| StreamError {
            format: StreamFormat::Binary,
            offset: start,
            reason: format!("fixture exceeds the service input limit: {error}"),
        })?;
    match crate::sab::frame_history(&ctx, bytes, start, limit, width) {
        Ok(records) => Ok(records),
        Err(
            StreamFailure::Parse(error)
            | StreamFailure::Malformed(error)
            | StreamFailure::NotImplemented(error),
        ) => Err(error),
        Err(StreamFailure::Resource(error)) => Err(StreamError {
            format: StreamFormat::Binary,
            offset: start,
            reason: format!("fixture exhausted a resource: {error}"),
        }),
    }
}

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
        if bytes[token_offset] == tag && matches!(&token, Lexed::Value(_) | Lexed::Str(_)) {
            offsets.push(token_offset);
        }
        position = next;
        if matches!(&token, Lexed::Terminator) {
            break;
        }
    }
    Ok(offsets)
}
