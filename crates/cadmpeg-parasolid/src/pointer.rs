// SPDX-License-Identifier: Apache-2.0
//! Encoded XMT indices (`parasolid.md` §1): a nonnegative `int16`, or a
//! negative `int16` remainder and a positive `int16` quotient.

use cadmpeg_core::decode::View;

/// The divisor of the two-word form.
const QUOTIENT_UNIT: u32 = 32_767;

/// The largest pointer: quotient and remainder magnitude 32767.
pub const MAX: u32 = 32_768 * 32_767;

/// Read one complete pointer and advance the view.
///
/// A failed read leaves the view unchanged.
#[must_use]
pub fn read(view: &mut View<'_>) -> Option<u32> {
    let mut next = *view;
    let first = next.i16_be()?;
    let value = if first >= 0 {
        u32::from(first.unsigned_abs())
    } else {
        let remainder = first.checked_neg()?;
        let quotient = next.i16_be()?;
        (quotient > 0).then_some(())?;
        u32::from(quotient.unsigned_abs()) * QUOTIENT_UNIT + u32::from(remainder.unsigned_abs())
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

/// Append one pointer in its shortest form.
///
/// Returns `None` when the value exceeds the two-word range.
pub fn write(out: &mut Vec<u8>, value: u32) -> Option<()> {
    if let Ok(short) = i16::try_from(value) {
        out.extend_from_slice(&short.to_be_bytes());
        return Some(());
    }
    let quotient = (value - 1) / QUOTIENT_UNIT;
    let remainder = i16::try_from(value - quotient * QUOTIENT_UNIT).ok()?;
    let quotient = i16::try_from(quotient).ok()?;
    out.extend_from_slice(&(-remainder).to_be_bytes());
    out.extend_from_slice(&quotient.to_be_bytes());
    Some(())
}

#[cfg(test)]
mod tests {
    use super::{read, write, MAX};
    use cadmpeg_core::decode::View;

    #[test]
    fn short_and_wide_pointer_boundaries() {
        for (bytes, expected) in [
            (&[0, 0][..], 0),
            (&[0, 1][..], 1),
            (&[0x7f, 0xfe][..], 0x7ffe),
            (&[0x7f, 0xff][..], 0x7fff),
            // 32768 = 1 * 32767 + 1.
            (&[0xff, 0xff, 0, 1][..], 0x8000),
            // 36225 = 1 * 32767 + 3458.
            (&[0xf2, 0x7e, 0, 1][..], 36_225),
            // 65534 = 1 * 32767 + 32767.
            (&[0x80, 0x01, 0, 1][..], 65_534),
            (&[0xff, 0xff, 0, 2][..], 65_535),
            (&[0xff, 0xfe, 0, 2][..], 0x1_0000),
            (&[0x80, 0x01, 0x7f, 0xff][..], MAX),
        ] {
            let mut view = View::over_retained(bytes);
            assert_eq!(read(&mut view), Some(expected));
            assert_eq!(view.remaining(), 0);
        }
    }

    #[test]
    fn written_pointers_take_the_shortest_form_and_read_back() {
        for value in [0, 1, 0x7fff, 0x8000, 36_225, 65_534, 65_535, 0x1_0000, MAX] {
            let mut bytes = Vec::new();
            write(&mut bytes, value).unwrap();
            assert_eq!(bytes.len(), if value <= 0x7fff { 2 } else { 4 });
            let mut view = View::over_retained(&bytes);
            assert_eq!(read(&mut view), Some(value));
        }
        assert_eq!(write(&mut Vec::new(), MAX + 1), None);
    }

    #[test]
    fn largest_short_pointer_preserves_the_next_pointer() {
        let mut view = View::over_retained(&[0x7f, 0xff, 0, 1]);
        assert_eq!(read(&mut view), Some(0x7fff));
        assert_eq!(read(&mut view), Some(1));
        assert_eq!(view.remaining(), 0);
    }

    #[test]
    fn incomplete_and_out_of_range_pointers_keep_the_cursor() {
        for bytes in [
            &[][..],
            &[0][..],
            &[0x80, 1][..],
            &[0x80, 1, 0][..],
            // Remainder word -32768 has no int16 magnitude.
            &[0x80, 0, 0, 1][..],
            // Quotient words zero and negative.
            &[0xff, 0xff, 0, 0][..],
            &[0xff, 0xff, 0x80, 0][..],
        ] {
            let mut view = View::over_retained(bytes);
            assert_eq!(read(&mut view), None);
            assert_eq!(view.position(), 0);
        }
    }
}
