// SPDX-License-Identifier: Apache-2.0
//! Checked little-endian primitive readers shared by binary codecs.

/// Returns `count` bytes at `offset` without advancing external state.
pub fn bytes_at(bytes: &[u8], offset: usize, count: usize) -> Option<&[u8]> {
    bytes.get(offset..offset.checked_add(count)?)
}

/// Takes `count` bytes and advances `position` only when the full slice exists.
pub fn take<'a>(bytes: &'a [u8], position: &mut usize, count: usize) -> Option<&'a [u8]> {
    let value = bytes_at(bytes, *position, count)?;
    *position += count;
    Some(value)
}

macro_rules! readers {
    ($(($at:ident, $take:ident, $ty:ty, $width:literal)),* $(,)?) => {
        $(
            #[doc = concat!("Reads a little-endian `", stringify!($ty), "` at `offset`.")]
            pub fn $at(bytes: &[u8], offset: usize) -> Option<$ty> {
                Some(<$ty>::from_le_bytes(bytes_at(bytes, offset, $width)?.try_into().ok()?))
            }

            #[doc = concat!("Takes a little-endian `", stringify!($ty), "` and advances `position`.")]
            pub fn $take(bytes: &[u8], position: &mut usize) -> Option<$ty> {
                Some(<$ty>::from_le_bytes(take(bytes, position, $width)?.try_into().ok()?))
            }
        )*
    };
}

readers!(
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

/// Reads three consecutive little-endian `f64` values.
pub fn vec3_at(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
    Some([
        f64_at(bytes, offset)?,
        f64_at(bytes, offset.checked_add(8)?)?,
        f64_at(bytes, offset.checked_add(16)?)?,
    ])
}

/// Takes three consecutive little-endian `f64` values.
pub fn take_vec3(bytes: &[u8], position: &mut usize) -> Option<[f64; 3]> {
    let value = vec3_at(bytes, *position)?;
    *position += 24;
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::{f64_at, take_u32, take_vec3};

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
    }
}
