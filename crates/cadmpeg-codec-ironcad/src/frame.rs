// SPDX-License-Identifier: Apache-2.0
//! Compressed stream frame.

use cadmpeg_container::compression::inflate_zlib_member;
use cadmpeg_core::decode::{DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

use crate::layout::frame_header as header;

/// The payload of a framed stream.
pub(crate) struct Frame<'a> {
    pub(crate) payload: View<'a>,
    /// Number of stream bytes after the last block.
    pub(crate) trailing: usize,
}

/// Inflates a framed stream and returns its payload.
pub(crate) fn payload<'a>(
    ctx: &DecodeContext<'a>,
    stream: View<'_>,
) -> Result<Frame<'a>, CodecError> {
    let mut cursor = stream;
    if cursor.req_u32_le()? != header::MAGIC_VALUE {
        return Err(CodecError::Malformed("frame magic is absent".into()));
    }
    let block_size = cursor.req_u32_le()?;
    let payload_length = cursor.req_u64_le()?;
    let mut compressed_length = cursor.req_u32_le()?;
    let length = usize::try_from(payload_length)
        .map_err(|_| CodecError::Malformed("frame payload length does not fit memory".into()))?;
    let block_bytes = usize::try_from(block_size)
        .map_err(|_| CodecError::Malformed("frame block size does not fit memory".into()))?;
    let count = block_count(length, block_bytes)?;
    let mut blocks = Vec::new();
    for index in 0..count {
        if index > 0 {
            compressed_length = cursor.req_u32_le()?;
        }
        blocks.push(block(ctx, &mut cursor, compressed_length, block_size)?);
    }
    let trailing = cursor.unread().len();
    if let [only] = blocks.as_slice() {
        let payload = only
            .child(only.start(), only.start() + length)
            .ok_or_else(|| CodecError::Malformed("frame payload escapes its block".into()))?;
        return Ok(Frame { payload, trailing });
    }
    let mut writer = ctx.begin_expand(ExpandSpec::Exact(payload_length))?;
    let mut remaining = length;
    for block in &blocks {
        let take = remaining.min(block.window().len());
        writer.write(&block.window()[..take])?;
        remaining -= take;
    }
    Ok(Frame {
        payload: writer.finalize()?,
        trailing,
    })
}

/// The number of blocks whose inflated bytes hold `length` payload bytes, and
/// at least one.
fn block_count(length: usize, block_bytes: usize) -> Result<usize, CodecError> {
    if block_bytes == 0 {
        return Err(CodecError::Malformed("frame block size is zero".into()));
    }
    Ok(length.div_ceil(block_bytes).max(1))
}

/// Inflates the block of `compressed_length` zlib bytes at `cursor`.
fn block<'a>(
    ctx: &DecodeContext<'a>,
    cursor: &mut View<'_>,
    compressed_length: u32,
    block_size: u32,
) -> Result<View<'a>, CodecError> {
    let compressed = cursor
        .counted(u64::from(compressed_length), 1)
        .ok_or_else(|| {
            CodecError::malformed(format_args!(
                "frame compressed length {compressed_length} exceeds the stream"
            ))
        })?;
    let source = cursor.req_take_child(compressed.get())?;
    let (inflated, consumed) =
        inflate_zlib_member(ctx, source, ExpandSpec::Exact(u64::from(block_size)))?;
    if consumed != source.window().len() {
        return Err(CodecError::Malformed(
            "frame zlib stream ends before its compressed length".into(),
        ));
    }
    Ok(inflated)
}

#[cfg(test)]
mod tests;
