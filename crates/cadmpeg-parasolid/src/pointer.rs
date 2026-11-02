// SPDX-License-Identifier: Apache-2.0
//! Native node pointers carry a short word or two words with 31 value bits.

use cadmpeg_core::decode::View;

/// Read one complete pointer and advance the view.
#[must_use]
pub fn read(view: &mut View<'_>) -> Option<u32> {
    let mut next = *view;
    let first = next.u16_be()?;
    let value = if first <= 0x7fff {
        u32::from(first)
    } else {
        u32::from(first & 0x7fff) | (u32::from(next.u16_be()?) << 15)
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

#[cfg(test)]
mod tests {
    use super::read;
    use cadmpeg_core::decode::View;

    #[test]
    fn short_and_wide_pointer_boundaries() {
        for (bytes, expected) in [
            (&[0, 0][..], 0),
            (&[0, 1][..], 1),
            (&[0x7f, 0xfe][..], 0x7ffe),
            (&[0xff, 0xff, 0, 0][..], 0x7fff),
            (&[0x7f, 0xff][..], 0x7fff),
            (&[0x80, 0, 0, 1][..], 0x8000),
            (&[0xff, 0xff, 0, 1][..], 0xffff),
            (&[0x80, 0, 0, 2][..], 0x1_0000),
            (&[0xff, 0xff, 0xff, 0xff][..], 0x7fff_ffff),
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
        for bytes in [&[][..], &[0][..], &[0x80, 0][..], &[0x80, 0, 0][..]] {
            let mut view = View::over_retained(bytes);
            assert_eq!(read(&mut view), None);
            assert_eq!(view.position(), 0);
        }
    }
}
