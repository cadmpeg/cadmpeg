// SPDX-License-Identifier: Apache-2.0
//! Declared auxiliary XT extents in the weldment carrier.

use super::{retained_copy, stream_header, ExtractedStream, ExtractedStreams};
use crate::layout::{parasolid_terminal as terminal, weldment_xt_entry_prefix as prefix};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

pub(super) fn extract(
    payload: &[u8],
    ctx: &DecodeContext<'_>,
) -> Result<ExtractedStreams, CodecError> {
    let mut streams = Vec::new();
    let mut cursor: usize = 0;
    while let Some(start) = cursor
        .checked_add(prefix::LEN)
        .filter(|end| *end <= payload.len())
    {
        let word = View::u32_le_at(payload, cursor + prefix::LEADING_WORD);
        if word != Some(prefix::LEADING_WORD_VALUE) || !payload[start..].starts_with(b"PS\0\0") {
            cursor += 1;
            continue;
        }
        let length = View::u32_le_at(payload, cursor + prefix::STREAM_LEN)
            .and_then(|length| usize::try_from(length).ok())
            .filter(|length| *length > 0)
            .ok_or_else(|| CodecError::malformed("SLDPRT weldment XT length is not positive"))?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= payload.len())
            .ok_or_else(|| {
                CodecError::malformed("SLDPRT weldment XT extent exceeds its carrier")
            })?;
        let bytes = &payload[start..end];
        let header = stream_header(bytes)?.ok_or_else(|| {
            CodecError::malformed("SLDPRT weldment XT extent has no complete header")
        })?;
        let terminal_at = length
            .checked_sub(terminal::LEN)
            .filter(|offset| *offset >= header.body_offset)
            .ok_or_else(|| CodecError::malformed("SLDPRT weldment XT extent cuts its header"))?;
        if View::u16_be_at(bytes, terminal_at + terminal::NODE_TYPE)
            != Some(terminal::NODE_TYPE_VALUE)
            || View::u16_be_at(bytes, terminal_at + terminal::NULL_INDEX_WIRE)
                != Some(terminal::NULL_INDEX_WIRE_VALUE)
        {
            return Err(CodecError::malformed(
                "SLDPRT weldment XT extent lacks the terminal wire suffix",
            ));
        }
        streams.push(ExtractedStream {
            offset: start,
            payload: retained_copy(ctx, bytes, "retain bounded weldment Parasolid stream")?,
            header,
        });
        cursor = end;
    }
    Ok(ExtractedStreams::complete(streams))
}
