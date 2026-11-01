// SPDX-License-Identifier: Apache-2.0
//! Compressed buffers and compressed integer sequences (spec §9.3, §9.4).
//!
//! A buffer is written as LZ4 blocks of literals only: each block is one
//! sequence with no match, which every LZ4 decoder expands to its input.

use cadmpeg_core::CodecError;

/// The most bytes one chunk of a compressed buffer expands to.
const MAX_CHUNK: usize = 0x7E00_0000;
/// A length nibble that continues in the following bytes.
const LENGTH_CONTINUES: usize = 15;

fn too_large() -> CodecError {
    CodecError::NotImplemented("a USDC section above the format's size limits".to_owned())
}

/// One LZ4 block that holds `data` as literals.
fn block(data: &[u8], out: &mut Vec<u8>) -> Result<(), CodecError> {
    if data.len() < LENGTH_CONTINUES {
        let length = u8::try_from(data.len()).map_err(|_| too_large())?;
        out.push(length << 4);
    } else {
        out.push(0xF0);
        let mut rest = data.len() - LENGTH_CONTINUES;
        while rest >= usize::from(u8::MAX) {
            out.push(u8::MAX);
            rest -= usize::from(u8::MAX);
        }
        out.push(u8::try_from(rest).map_err(|_| too_large())?);
    }
    out.extend_from_slice(data);
    Ok(())
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

/// The coded form of a 32-bit integer sequence (spec §9.4): the most
/// frequent difference as the common difference, two-bit codes, then the
/// other differences at the narrowest width that holds them.
fn coded(values: &[i32]) -> Vec<u8> {
    let differences: Vec<i32> = values
        .iter()
        .scan(0_i32, |previous, value| {
            // wrapping-exception: integer differences are modulo 2^32 (spec §9.4)
            let difference = value.wrapping_sub(*previous);
            *previous = *value;
            Some(difference)
        })
        .collect();
    let mut counts: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    for difference in &differences {
        *counts.entry(*difference).or_default() += 1;
    }
    let common = counts
        .iter()
        .max_by_key(|(difference, count)| (**count, std::cmp::Reverse(**difference)))
        .map_or(0, |(difference, _)| *difference);
    let mut out = common.to_le_bytes().to_vec();
    let codes_start = out.len();
    out.resize(codes_start + (2 * differences.len()).div_ceil(8), 0);
    for (index, difference) in differences.iter().enumerate() {
        let code: u8 = if *difference == common {
            0
        } else if let Ok(narrow) = i8::try_from(*difference) {
            out.push(narrow.to_le_bytes()[0]);
            1
        } else if let Ok(narrow) = i16::try_from(*difference) {
            out.extend_from_slice(&narrow.to_le_bytes());
            2
        } else {
            out.extend_from_slice(&difference.to_le_bytes());
            3
        };
        out[codes_start + index / 4] |= code << (2 * (index % 4));
    }
    out
}

/// `values` as a compressed 32-bit integer sequence: its `uint64` byte
/// size, then the compressed buffer.
pub(super) fn integers(values: &[i32], out: &mut Vec<u8>) -> Result<(), CodecError> {
    let buffer = if values.is_empty() {
        compress(&[])?
    } else {
        compress(&coded(values))?
    };
    out.extend_from_slice(
        &u64::try_from(buffer.len())
            .map_err(|_| too_large())?
            .to_le_bytes(),
    );
    out.extend_from_slice(&buffer);
    Ok(())
}

/// `values` as indices of a compressed 32-bit sequence.
pub(super) fn indices(values: &[u32], out: &mut Vec<u8>) -> Result<(), CodecError> {
    let signed: Vec<i32> = values.iter().map(|value| value.cast_signed()).collect();
    integers(&signed, out)
}

#[cfg(test)]
mod tests;
