// SPDX-License-Identifier: Apache-2.0
//! The integer coding of compressed integer sequences (spec §9.4).
//!
//! A sequence is coded as the differences between consecutive integers: the
//! most common difference, a 2-bit code per integer, and the integers that
//! are not the common difference in 1, 2, or 4 bytes (8-bit, 16-bit, or
//! 32-bit for 32-bit integers; 16-bit, 32-bit, or 64-bit for 64-bit
//! integers). The coded bytes are LZ4 compressed (spec §9.3).

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;

use super::compression::{expand, Size};

/// The width of the integers of a sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Width {
    Bits32,
    Bits64,
}

impl Width {
    const fn bytes(self) -> usize {
        match self {
            Self::Bits32 => 4,
            Self::Bits64 => 8,
        }
    }
}

fn overflow() -> CodecError {
    CodecError::malformed("a compressed integer sequence overflows its width")
}

/// `previous + delta` in the two's-complement arithmetic of `width`: the
/// coder takes differences modulo 2^32 or 2^64.
fn modular(previous: i64, delta: i64, width: Width) -> Option<i64> {
    let (modulus, half): (i128, i128) = match width {
        Width::Bits32 => (1 << 32, 1 << 31),
        Width::Bits64 => (1 << 64, 1 << 63),
    };
    let sum = i128::from(previous) + i128::from(delta);
    i64::try_from((sum + half).rem_euclid(modulus) - half).ok()
}

/// The largest coded size of `count` integers.
fn coded_size(count: usize, width: Width) -> Option<usize> {
    if count == 0 {
        return Some(0);
    }
    let codes = count.checked_mul(2)?.checked_add(7)? / 8;
    width
        .bytes()
        .checked_add(codes)?
        .checked_add(count.checked_mul(width.bytes())?)
}

/// Decodes `count` coded integers from `coded`, as signed values.
pub(super) fn decode(
    ctx: &DecodeContext<'_>,
    coded: View<'_>,
    count: usize,
    width: Width,
) -> Result<Vec<i64>, CodecError> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut view = coded;
    let common = match width {
        Width::Bits32 => i64::from(view.req_i32_le()?),
        Width::Bits64 => view.req_i64_le()?,
    };
    let codes_length = count
        .checked_mul(2)
        .and_then(|bits| bits.checked_add(7))
        .ok_or_else(overflow)?
        / 8;
    // The codes bound the count by the coded bytes.
    let codes = view.req_take(codes_length)?;
    let mut out = ctx.alloc_filled(count, 0_i64, "usdc_integers")?;
    let mut previous = 0_i64;
    for (index, slot) in out.iter_mut().enumerate() {
        let code = (codes[index / 4] >> (2 * (index % 4))) & 3;
        let delta = match (code, width) {
            (0, _) => common,
            (1, Width::Bits32) => i64::from(view.req_u8()?.cast_signed()),
            (2, Width::Bits32) | (1, Width::Bits64) => i64::from(view.req_i16_le()?),
            (3, Width::Bits32) | (2, Width::Bits64) => i64::from(view.req_i32_le()?),
            _ => view.req_i64_le()?,
        };
        previous = modular(previous, delta, width).ok_or_else(overflow)?;
        *slot = previous;
    }
    Ok(out)
}

/// Reads a compressed integer sequence of `count` integers at the view's
/// cursor: its compressed size, then its LZ4-compressed coded bytes.
pub(super) fn read<'a>(
    ctx: &DecodeContext<'a>,
    view: &mut View<'a>,
    count: usize,
    width: Width,
) -> Result<Vec<i64>, CodecError> {
    let size = view.req_u64_le()?;
    let size = usize::try_from(size).map_err(|_| {
        CodecError::malformed("a compressed integer size exceeds the address space")
    })?;
    let compressed = view.req_take_child(size)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let limit = coded_size(count, width).ok_or_else(overflow)?;
    let coded = expand(ctx, compressed, Size::AtMost(limit))?;
    decode(ctx, coded, count, width)
}

#[cfg(test)]
mod tests;
