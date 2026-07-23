// SPDX-License-Identifier: Apache-2.0
//! Checked little-endian primitive readers shared by binary codecs.
#![deny(clippy::disallowed_methods)]

use crate::wire::read::readers;

readers!(from_le_bytes, "little-endian";
    (u16_at, take_u16, u16, 2),
    (i16_at, take_i16, i16, 2),
    (u32_at, take_u32, u32, 4),
    (i32_at, take_i32, i32, 4),
    (u64_at, take_u64, u64, 8),
    (i64_at, take_i64, i64, 8),
    (f32_at, take_f32, f32, 4),
    (f64_at, take_f64, f64, 8),
);

/// Reads a signed little-endian integer with a four- or eight-byte width.
pub fn int_at(bytes: &[u8], offset: usize, width: usize) -> Option<i64> {
    match width {
        4 => Some(i64::from(i32_at(bytes, offset)?)),
        8 => i64_at(bytes, offset),
        _ => None,
    }
}

/// Takes a signed little-endian integer with a four- or eight-byte width.
pub fn take_int(bytes: &[u8], position: &mut usize, width: usize) -> Option<i64> {
    let value = int_at(bytes, *position, width)?;
    *position += width;
    Some(value)
}

/// Reads a u32-length-prefixed byte slice and returns it with the end offset.
pub fn lp_u32_bytes_at(bytes: &[u8], offset: usize) -> Option<(&[u8], usize)> {
    let length = usize::try_from(u32_at(bytes, offset)?).ok()?;
    let start = offset.checked_add(4)?;
    let end = start.checked_add(length)?;
    Some((bytes.get(start..end)?, end))
}

/// Takes a u32-length-prefixed byte slice, advancing only on success.
pub fn take_lp_u32_bytes<'a>(bytes: &'a [u8], position: &mut usize) -> Option<&'a [u8]> {
    let (value, end) = lp_u32_bytes_at(bytes, *position)?;
    *position = end;
    Some(value)
}

/// Decodes `count` UTF-16LE code units at `offset` and returns the end offset.
pub fn utf16le_at(bytes: &[u8], offset: usize, count: usize) -> Option<(String, usize)> {
    let byte_length = count.checked_mul(2)?;
    let end = offset.checked_add(byte_length)?;
    let units = bytes
        .get(offset..end)?
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    Some((String::from_utf16(&units).ok()?, end))
}

#[cfg(test)]
mod tests {
    use super::{f64_at, f64s_at, lp_u32_bytes_at, take_f64s, take_u32, take_vec3, utf16le_at};

    #[test]
    fn failed_take_does_not_advance() {
        let mut position = 0;
        assert_eq!(take_u32(&[1, 2, 3], &mut position), None);
        assert_eq!(position, 0);
    }

    #[test]
    fn reads_scalars_and_vectors() {
        let mut bytes = Vec::new();
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        assert_eq!(f64_at(&bytes, 8), Some(2.0));
        assert_eq!(take_vec3(&bytes, &mut 0), Some([1.0, 2.0, 3.0]));
        assert_eq!(f64s_at(&bytes, 8, 2), Some(vec![2.0, 3.0]));
        let mut position = 8;
        assert_eq!(take_f64s(&bytes, &mut position, 2), Some(vec![2.0, 3.0]));
        assert_eq!(position, 24);
        assert_eq!(take_f64s(&bytes, &mut position, 1), None);
        assert_eq!(position, 24);
    }

    #[test]
    fn reads_length_prefixed_bytes_and_utf16() {
        assert_eq!(
            lp_u32_bytes_at(b"\x03\0\0\0abc", 0),
            Some((b"abc".as_slice(), 7))
        );
        assert_eq!(utf16le_at(b"A\0B\0", 0, 2), Some(("AB".to_string(), 4)));
    }
}
