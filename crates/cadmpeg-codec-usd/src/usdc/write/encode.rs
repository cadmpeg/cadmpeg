// SPDX-License-Identifier: Apache-2.0
//! Compressed buffers and compressed integer sequences (spec §9.3, §9.4).
//!
//! A buffer is written as LZ4 blocks. Each match starts at least 12 bytes
//! before the end of its block and the last 5 bytes of a block are
//! literals, so that every LZ4 decoder expands it to its input.

use cadmpeg_core::CodecError;

use crate::usdc::integers::Width;

/// The most bytes one chunk of a compressed buffer expands to.
const MAX_CHUNK: usize = 0x7E00_0000;
/// A length nibble that continues in the following bytes.
const LENGTH_CONTINUES: usize = 15;
/// The shortest match.
const MIN_MATCH: usize = 4;
/// The bytes at the end of a block that are literals.
const LAST_LITERALS: usize = 5;
/// The least distance from the start of a match to the end of its block.
const MATCH_LIMIT: usize = 12;
/// The farthest match offset.
const MAX_OFFSET: usize = 0xFFFF;
/// The multiplier of the match-finder hash.
const HASH_FACTOR: u64 = 2_654_435_761;

fn too_large() -> CodecError {
    CodecError::NotImplemented("a USDC section above the format's size limits".to_owned())
}

/// `length` as a length extension after a nibble of 15.
fn extension(length: usize, out: &mut Vec<u8>) -> Result<(), CodecError> {
    let mut rest = length - LENGTH_CONTINUES;
    while rest >= usize::from(u8::MAX) {
        out.push(u8::MAX);
        rest -= usize::from(u8::MAX);
    }
    out.push(u8::try_from(rest).map_err(|_| too_large())?);
    Ok(())
}

/// One sequence: `literals`, then the match of `length` bytes at
/// `offset` bytes back, if any.
fn sequence(
    literals: &[u8],
    copy: Option<(usize, usize)>,
    out: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let nibble = |length: usize| u8::try_from(length.min(LENGTH_CONTINUES)).unwrap_or(0x0F);
    let match_length = copy.map_or(0, |(_, length)| length - MIN_MATCH);
    out.push(nibble(literals.len()) << 4 | nibble(match_length));
    if literals.len() >= LENGTH_CONTINUES {
        extension(literals.len(), out)?;
    }
    out.extend_from_slice(literals);
    if let Some((offset, _)) = copy {
        out.extend_from_slice(
            &u16::try_from(offset)
                .map_err(|_| too_large())?
                .to_le_bytes(),
        );
        if match_length >= LENGTH_CONTINUES {
            extension(match_length, out)?;
        }
    }
    Ok(())
}

/// The match-finder slot of the four bytes at the start of `bytes`, in a
/// table of `slots` entries.
fn slot(bytes: &[u8], slots: usize) -> usize {
    let word = bytes[..MIN_MATCH]
        .iter()
        .rev()
        .fold(0_u64, |word, byte| word << 8 | u64::from(*byte));
    usize::try_from((word * HASH_FACTOR) >> 16).unwrap_or(0) % slots
}

/// One LZ4 block that expands to `data`.
fn block(data: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
    let mut table = vec![None; 0x10000];
    let mut anchor = 0;
    let mut at = 0;
    while at + MATCH_LIMIT <= data.len() {
        let slot = slot(&data[at..], table.len());
        let candidate: Option<usize> = table[slot];
        table[slot] = Some(at);
        let Some(candidate) = candidate.filter(|candidate| {
            at - candidate <= MAX_OFFSET
                && data[*candidate..*candidate + MIN_MATCH] == data[at..at + MIN_MATCH]
        }) else {
            at += 1;
            continue;
        };
        let end = data.len() - LAST_LITERALS;
        let mut length = MIN_MATCH;
        while at + length < end && data[candidate + length] == data[at + length] {
            length += 1;
        }
        sequence(&data[anchor..at], Some((at - candidate, length)), out)?;
        at += length;
        anchor = at;
    }
    sequence(&data[anchor..], None, out)
}

/// `data` as a compressed buffer.
pub(super) fn compress(data: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 255 + 16);
    if data.len() <= MAX_CHUNK {
        out.push(0);
        block(data, &mut out)?;
        return Ok(out);
    }
    let chunks = data.chunks(MAX_CHUNK);
    out.push(u8::try_from(chunks.len()).map_err(|_| too_large())?);
    for chunk in chunks {
        let mut body = Vec::new();
        block(chunk, &mut body)?;
        out.extend_from_slice(
            &i32::try_from(body.len())
                .map_err(|_| too_large())?
                .to_le_bytes(),
        );
        out.extend_from_slice(&body);
    }
    Ok(out)
}

/// The bytes of the narrowest difference of a `width` sequence that
/// holds `value`, and its code.
fn narrowest(value: i64, width: Width) -> (u8, usize) {
    let (narrow, middle, wide) = match width {
        Width::Bits32 => (1, 2, 4),
        Width::Bits64 => (2, 4, 8),
    };
    let fits = |bytes: usize| match bytes {
        1 => i8::try_from(value).is_ok(),
        2 => i16::try_from(value).is_ok(),
        4 => i32::try_from(value).is_ok(),
        _ => true,
    };
    if fits(narrow) {
        (1, narrow)
    } else if fits(middle) {
        (2, middle)
    } else {
        (3, wide)
    }
}

/// The coded form of an integer sequence with `differences` (spec §9.4):
/// the most frequent difference as the common difference, two-bit codes,
/// then the other differences at the narrowest width that holds them.
fn coded(differences: &[i64], width: Width) -> Vec<u8> {
    let mut counts: std::collections::BTreeMap<i64, usize> = std::collections::BTreeMap::new();
    for difference in differences {
        *counts.entry(*difference).or_default() += 1;
    }
    let common = counts
        .iter()
        .max_by_key(|(difference, count)| (**count, std::cmp::Reverse(**difference)))
        .map_or(0, |(difference, _)| *difference);
    let common_bytes = match width {
        Width::Bits32 => 4,
        Width::Bits64 => 8,
    };
    let mut out = common.to_le_bytes()[..common_bytes].to_vec();
    let codes_start = out.len();
    out.resize(codes_start + (2 * differences.len()).div_ceil(8), 0);
    for (index, difference) in differences.iter().enumerate() {
        let code = if *difference == common {
            0
        } else {
            let (code, bytes) = narrowest(*difference, width);
            out.extend_from_slice(&difference.to_le_bytes()[..bytes]);
            code
        };
        out[codes_start + index / 4] |= code << (2 * (index % 4));
    }
    out
}

/// `coded` as a compressed integer sequence: its `uint64` byte size, then
/// the compressed buffer.
fn sequence_of(coded: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
    let buffer = compress(coded)?;
    out.extend_from_slice(
        &u64::try_from(buffer.len())
            .map_err(|_| too_large())?
            .to_le_bytes(),
    );
    out.extend_from_slice(&buffer);
    Ok(())
}

/// `values` as a compressed 32-bit integer sequence.
pub(super) fn integers(values: &[i32], out: &mut Vec<u8>) -> Result<(), CodecError> {
    if values.is_empty() {
        return sequence_of(&[], out);
    }
    let mut previous = 0_i32;
    let differences: Vec<i64> = values
        .iter()
        .map(|value| {
            // wrapping-exception: integer differences are modulo 2^32 (spec §9.4)
            let difference = value.wrapping_sub(previous);
            previous = *value;
            i64::from(difference)
        })
        .collect();
    sequence_of(&coded(&differences, Width::Bits32), out)
}

/// `values` as a compressed 64-bit integer sequence.
pub(super) fn wide_integers(values: &[i64], out: &mut Vec<u8>) -> Result<(), CodecError> {
    if values.is_empty() {
        return sequence_of(&[], out);
    }
    let mut previous = 0_i64;
    let differences: Vec<i64> = values
        .iter()
        .map(|value| {
            // wrapping-exception: integer differences are modulo 2^64 (spec §9.4)
            let difference = value.wrapping_sub(previous);
            previous = *value;
            difference
        })
        .collect();
    sequence_of(&coded(&differences, Width::Bits64), out)
}

/// `values` as indices of a compressed 32-bit sequence.
pub(super) fn indices(values: &[u32], out: &mut Vec<u8>) -> Result<(), CodecError> {
    let signed: Vec<i32> = values.iter().map(|value| value.cast_signed()).collect();
    integers(&signed, out)
}

#[cfg(test)]
mod tests;
