// SPDX-License-Identifier: Apache-2.0
//! Native node pointers carry the positive-integer encoding without its bias.
//!
//! A big-endian word up to `0x7fff` is the value. A word with its high bit set
//! is the two's-complement negation of a magnitude `m`, and the next word is a
//! quotient `q`. The value is `q * 32767 + m`, with `m` and `q` in `1..=32767`.

use cadmpeg_core::decode::View;

const PART_MAX: u16 = 32_767;

/// Read one complete pointer and advance the view.
#[must_use]
pub fn read(view: &mut View<'_>) -> Option<u32> {
    let mut next = *view;
    let first = next.u16_be()?;
    let value = if first <= PART_MAX {
        u32::from(first)
    } else {
        let magnitude = (!first).checked_add(1)?;
        let quotient = next.u16_be()?;
        if magnitude > PART_MAX || quotient == 0 || quotient > PART_MAX {
            return None;
        }
        u32::from(quotient) * u32::from(PART_MAX) + u32::from(magnitude)
    };
    *view = next;
    Some(value)
}

/// Read one complete pointer at an offset and advance the offset.
#[must_use]
pub fn read_at(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let mut view = View::over_retained(bytes);
    view.seek(*at)?;
    let value = read(&mut view)?;
    *at = view.position();
    Some(value)
}

/// The one or two words that store `value`, and how many of them are used.
#[must_use]
pub fn encode(value: u32) -> Option<([u16; 2], usize)> {
    if value <= u32::from(PART_MAX) {
        return Some(([u16::try_from(value).ok()?, 0], 1));
    }
    let part = u32::from(PART_MAX);
    let mut quotient = value / part;
    let mut magnitude = value % part;
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
    use super::{encode, read, read_at};
    use cadmpeg_core::decode::View;

    #[test]
    fn short_and_wide_pointer_boundaries() {
        for (bytes, expected) in [
            (&[0, 0][..], 0),
            (&[0, 1][..], 1),
            (&[0x7f, 0xfe][..], 0x7ffe),
            (&[0x7f, 0xff][..], 0x7fff),
            (&[0xff, 0xff, 0, 1][..], 32_768),
            (&[0x80, 0x01, 0, 1][..], 65_534),
            (&[0xff, 0xff, 0, 2][..], 65_535),
            (&[0xfe, 0xd0, 0, 1][..], 33_071),
            (&[0x80, 0x01, 0x7f, 0xff][..], 32_767 * 32_767 + 32_767),
        ] {
            let mut view = View::over_retained(bytes);
            assert_eq!(read(&mut view), Some(expected));
            assert_eq!(view.remaining(), 0);
        }
    }

    #[test]
    fn largest_short_pointer_preserves_the_next_pointer() {
        let mut view = View::over_retained(&[0x7f, 0xff, 0, 1]);
        assert_eq!(read(&mut view), Some(0x7fff));
        assert_eq!(read(&mut view), Some(1));
        assert_eq!(view.remaining(), 0);
    }

    #[test]
    fn incomplete_pointers_keep_the_cursor() {
        for bytes in [&[][..], &[0][..], &[0x80, 1][..], &[0x80, 1, 0][..]] {
            let mut view = View::over_retained(bytes);
            assert_eq!(read(&mut view), None);
            assert_eq!(view.position(), 0);
        }
    }

    #[test]
    fn out_of_range_wide_parts_have_no_value() {
        for bytes in [
            &[0x80, 0, 0, 1][..],
            &[0xff, 0xff, 0, 0][..],
            &[0xff, 0xff, 0x80, 0][..],
        ] {
            let mut view = View::over_retained(bytes);
            assert_eq!(read(&mut view), None);
            assert_eq!(view.position(), 0);
        }
    }

    #[test]
    fn encoding_is_the_inverse_of_reading() {
        for value in [
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
            let (words, count) = encode(value).expect("value in the two-word range");
            let bytes: Vec<u8> = words[..count]
                .iter()
                .flat_map(|word| word.to_be_bytes())
                .collect();
            let mut at = 0;
            assert_eq!(read_at(&bytes, &mut at), Some(value));
            assert_eq!(at, bytes.len());
        }
        assert_eq!(encode(1_073_709_057), None);
    }
}
