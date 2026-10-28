// SPDX-License-Identifier: Apache-2.0
//! The XT positive-integer cell that stores node indices, pointers, and counts.
//!
//! A big-endian word `w` in `0..=0x7fff` is the code `w`. A word with its high
//! bit set is the negative magnitude `m` of a two-word cell, and the second
//! word is the quotient `q`; the code is `q * 32767 + m`. Both `m` and `q` lie
//! in `1..=32767`. A pointer code is one more than the XT index, so code `1`
//! is the null pointer ([spec §4](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).

use cadmpeg_core::decode::View;

/// The largest code a one-word cell stores.
pub(crate) const NARROW_MAX: u16 = 0x7fff;
const PART_MAX: u16 = 32767;

/// The code and the byte width of the cell at `at`.
pub(crate) fn code_at(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let word = View::u16_be_at(bytes, at)?;
    if word <= NARROW_MAX {
        return Some((u32::from(word), 2));
    }
    let magnitude = (!word).checked_add(1)?;
    let quotient = View::u16_be_at(bytes, at.checked_add(2)?)?;
    if magnitude > PART_MAX || quotient == 0 || quotient > PART_MAX {
        return None;
    }
    Some((
        u32::from(quotient) * u32::from(PART_MAX) + u32::from(magnitude),
        4,
    ))
}

/// Read the cell at `*at` and advance past it.
pub(crate) fn read_code(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let (code, width) = code_at(bytes, *at)?;
    *at = at.checked_add(width)?;
    Some(code)
}

/// Read the cell at `*at` as a stream-local record key and advance past it.
///
/// A record key is a `u16`; a code past `0xffff` names no keyed record.
pub(crate) fn read_key(bytes: &[u8], at: &mut usize) -> Option<u16> {
    u16::try_from(read_code(bytes, at)?).ok()
}

/// Take the cell at the view position as a record key.
pub(crate) fn take_key(view: &mut View<'_>) -> Option<u16> {
    let (code, width) = code_at(view.unread(), 0)?;
    view.skip(width)?;
    u16::try_from(code).ok()
}

/// Take `N` consecutive cells at the view position as record keys.
pub(crate) fn take_keys<const N: usize>(view: &mut View<'_>) -> Option<[u16; N]> {
    let mut keys = [0; N];
    for key in &mut keys {
        *key = take_key(view)?;
    }
    Some(keys)
}

/// The one- or two-word cell that stores `code`, or `None` past the two-word
/// range.
pub(crate) fn encode(code: u32) -> Option<([u16; 2], usize)> {
    if code <= u32::from(NARROW_MAX) {
        return Some(([u16::try_from(code).ok()?, 0], 1));
    }
    let part = u32::from(PART_MAX);
    let mut quotient = code / part;
    let mut magnitude = code % part;
    if magnitude == 0 {
        quotient -= 1;
        magnitude = part;
    }
    let quotient = u16::try_from(quotient).ok().filter(|q| *q <= PART_MAX)?;
    let magnitude = u16::try_from(magnitude).ok()?;
    Some(([(!magnitude).checked_add(1)?, quotient], 2))
}

#[cfg(test)]
mod tests {
    use super::{code_at, encode, read_key};

    fn cell(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_be_bytes()).collect()
    }

    #[test]
    fn narrow_words_are_their_own_code_through_0x7fff() {
        for word in [0, 1, 2, 0x7ffe, 0x7fff] {
            assert_eq!(code_at(&cell(&[word]), 0), Some((u32::from(word), 2)));
        }
    }

    #[test]
    fn two_word_cells_are_quotient_times_32767_plus_magnitude() {
        assert_eq!(code_at(&cell(&[0xffff, 1]), 0), Some((32_768, 4)));
        assert_eq!(code_at(&cell(&[0x8001, 1]), 0), Some((65_534, 4)));
        assert_eq!(code_at(&cell(&[0xffff, 2]), 0), Some((65_535, 4)));
        assert_eq!(code_at(&cell(&[0xfed0, 1]), 0), Some((33_071, 4)));
        assert_eq!(
            code_at(&cell(&[0x8001, 0x7fff]), 0),
            Some((32_767 * 32_767 + 32_767, 4))
        );
    }

    #[test]
    fn out_of_range_parts_and_truncated_cells_have_no_code() {
        assert_eq!(code_at(&cell(&[0x8000, 1]), 0), None);
        assert_eq!(code_at(&cell(&[0xffff, 0]), 0), None);
        assert_eq!(code_at(&cell(&[0xffff, 0x8000]), 0), None);
        assert_eq!(code_at(&cell(&[0xffff]), 0), None);
        assert_eq!(code_at(&[0x12], 0), None);
    }

    #[test]
    fn a_code_past_u16_is_no_record_key() {
        let mut at = 0;
        assert_eq!(read_key(&cell(&[0xffff, 2]), &mut at), Some(0xffff));
        assert_eq!(at, 4);
        let mut at = 0;
        assert_eq!(read_key(&cell(&[0xfffe, 2]), &mut at), None);
    }

    #[test]
    fn encoding_is_the_inverse_of_decoding() {
        for code in [
            0,
            1,
            0x7ffe,
            0x7fff,
            0x8000,
            65_534,
            65_535,
            0x1_0000,
            1_073_709_056,
        ] {
            let (words, count) = encode(code).expect("code in the two-word range");
            assert_eq!(code_at(&cell(&words[..count]), 0), Some((code, count * 2)));
        }
        assert_eq!(encode(1_073_709_057), None);
    }
}
