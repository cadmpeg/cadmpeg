// SPDX-License-Identifier: Apache-2.0
//! Compressed stream frame.

use cadmpeg_container::compression::inflate_zlib_member;
use cadmpeg_core::decode::{DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

use crate::layout::frame_header as header;

/// Inflates a framed stream and returns its payload.
pub(crate) fn payload<'a>(
    ctx: &DecodeContext<'a>,
    stream: View<'_>,
) -> Result<View<'a>, CodecError> {
    let mut cursor = stream;
    if cursor.req_u32_le()? != header::MAGIC_VALUE {
        return Err(CodecError::Malformed("frame magic is absent".into()));
    }
    let block_size = cursor.req_u32_le()?;
    let payload_length = cursor.req_u64_le()?;
    let compressed_length = cursor.req_u32_le()?;
    if payload_length > u64::from(block_size) {
        return Err(CodecError::malformed(format_args!(
            "frame payload length {payload_length} exceeds block size {block_size}"
        )));
    }
    let compressed = cursor
        .counted(u64::from(compressed_length), 1)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "frame compressed length {compressed_length} exceeds the stream"
            ))
        })?;
    let source = cursor.req_take_child(compressed.get())?;
    let (block, consumed) =
        inflate_zlib_member(ctx, source, ExpandSpec::Exact(u64::from(block_size)))?;
    if consumed != source.window().len() {
        return Err(CodecError::Malformed(
            "frame zlib stream ends before its compressed length".into(),
        ));
    }
    let length = usize::try_from(payload_length)
        .map_err(|_| CodecError::Malformed("frame payload length does not fit memory".into()))?;
    block
        .child(block.start(), block.start() + length)
        .ok_or_else(|| CodecError::Malformed("frame payload escapes its block".into()))
}

#[cfg(test)]
mod tests;
