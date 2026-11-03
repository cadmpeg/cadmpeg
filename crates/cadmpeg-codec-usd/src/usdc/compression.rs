// SPDX-License-Identifier: Apache-2.0
//! The LZ4 block format and its chunked framing (spec §9.3).
//!
//! The output of every expansion is charged through
//! [`DecodeContext::begin_expand`] at its declared size before a byte is
//! produced, and decoding stops at that size.

use cadmpeg_core::decode::{DecodeContext, ExpandSpec, View};
use cadmpeg_core::CodecError;

/// Largest output of one chunk of the chunked framing (spec §9.3).
const MAX_CHUNK_OUTPUT: usize = 0x7E00_0000;
/// Length nibble that continues in the following bytes.
const LENGTH_CONTINUES: usize = 15;
/// Smallest match length; the stored nibble adds to it.
const MIN_MATCH: usize = 4;
/// Largest number of output bytes one input byte of a block yields.
const MAX_RATIO: usize = 255;

fn corrupt(what: &str) -> CodecError {
    CodecError::malformed(format_args!("corrupt LZ4 block: {what}"))
}

/// Reads an extended length: `nibble`, plus the following bytes while they
/// are 255, plus the first byte below 255.
fn extended(view: &mut View<'_>, nibble: usize) -> Result<usize, CodecError> {
    let mut length = nibble;
    if nibble == LENGTH_CONTINUES {
        loop {
            let byte = view.req_u8()?;
            length = length
                .checked_add(usize::from(byte))
                .ok_or_else(|| corrupt("a length overflows"))?;
            if byte != u8::MAX {
                break;
            }
        }
    }
    Ok(length)
}

/// Decodes one LZ4 block from `block` onto the end of `out`, which grows to
/// at most `limit` bytes.
fn block(block: View<'_>, out: &mut Vec<u8>, limit: usize) -> Result<(), CodecError> {
    let start = out.len();
    let mut view = block;
    while !view.is_empty() {
        let token = view.req_u8()?;
        let literals = extended(&mut view, usize::from(token >> 4))?;
        let bytes = view.req_take(literals)?;
        if out
            .len()
            .checked_add(bytes.len())
            .is_none_or(|end| end > limit)
        {
            return Err(corrupt("the output exceeds its declared size"));
        }
        out.extend_from_slice(bytes);
        if view.is_empty() {
            break;
        }
        let offset = usize::from(view.req_u16_le()?);
        let length = extended(&mut view, usize::from(token & 0x0F))?
            .checked_add(MIN_MATCH)
            .ok_or_else(|| corrupt("a match length overflows"))?;
        if offset == 0 || offset > out.len() - start {
            return Err(corrupt("a match refers before the block"));
        }
        if out.len().checked_add(length).is_none_or(|end| end > limit) {
            return Err(corrupt("the output exceeds its declared size"));
        }
        let from = out.len() - offset;
        for index in 0..length {
            let byte = out[from + index];
            out.push(byte);
        }
    }
    Ok(())
}

/// The size an expansion states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    /// The output is exactly this many bytes.
    Exact(usize),
    /// The output is at most this many bytes.
    AtMost(usize),
}

/// Expands the chunked LZ4 framing in `input`, as a view in a derived
/// space.
pub(super) fn expand<'a>(
    ctx: &DecodeContext<'a>,
    input: View<'_>,
    size: Size,
) -> Result<View<'a>, CodecError> {
    let (limit, spec) = match size {
        Size::Exact(size) => (
            size,
            ExpandSpec::Exact(cadmpeg_core::decode::u64_from_index(size)),
        ),
        Size::AtMost(limit) => {
            // One input byte yields at most `MAX_RATIO` output bytes, so the
            // output is charged against the expansion allowance before it
            // exists.
            let reach = input
                .remaining()
                .checked_mul(MAX_RATIO)
                .map_or(limit, |reach| reach.min(limit));
            drop(
                ctx.begin_expand(ExpandSpec::Exact(cadmpeg_core::decode::u64_from_index(
                    reach,
                )))?,
            );
            (reach, ExpandSpec::Unknown)
        }
    };
    let mut writer = ctx.begin_expand(spec)?;
    let mut out = ctx.alloc_filled(0, 0_u8, "usdc_expand")?;
    if let Size::Exact(size) = size {
        out.try_reserve_exact(size)
            .map_err(|_| corrupt("the declared size cannot be allocated"))?;
    }
    let mut view = input;
    let chunks = view.req_u8()?;
    if chunks == 0 {
        block(view, &mut out, limit)?;
    } else {
        for _ in 0..chunks {
            let length = view.req_i32_le()?;
            let length =
                usize::try_from(length).map_err(|_| corrupt("a chunk size is negative"))?;
            let chunk = view.req_take_child(length)?;
            let chunk_limit = out
                .len()
                .checked_add(MAX_CHUNK_OUTPUT)
                .map_or(limit, |chunk_limit| chunk_limit.min(limit));
            block(chunk, &mut out, chunk_limit)?;
        }
    }
    if let Size::Exact(size) = size {
        if out.len() != size {
            return Err(CodecError::malformed(format_args!(
                "an LZ4 expansion yields {} bytes, not the declared {size}",
                out.len()
            )));
        }
    }
    writer.write(&out)?;
    writer.finalize()
}

#[cfg(test)]
mod tests;
