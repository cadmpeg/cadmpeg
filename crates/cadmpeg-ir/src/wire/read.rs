// SPDX-License-Identifier: Apache-2.0
//! Checked byte-slice readers shared by endian-specific modules.
#![deny(clippy::disallowed_methods)]

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
    ($conversion:ident, $endian:literal; $(($at:ident, $take:ident, $ty:ty, $width:literal)),* $(,)?) => {
        $(
            #[doc = concat!("Reads a ", $endian, " `", stringify!($ty), "` at `offset`.")]
            pub fn $at(bytes: &[u8], offset: usize) -> Option<$ty> {
                Some(<$ty>::$conversion($crate::wire::read::bytes_at(bytes, offset, $width)?.try_into().ok()?))
            }

            #[doc = concat!("Takes a ", $endian, " `", stringify!($ty), "` and advances `position`.")]
            pub fn $take(bytes: &[u8], position: &mut usize) -> Option<$ty> {
                Some(<$ty>::$conversion($crate::wire::read::take(bytes, position, $width)?.try_into().ok()?))
            }
        )*

        #[doc = concat!("Reads consecutive ", $endian, " `f64` values at `offset`.")]
        pub fn f64s_at(bytes: &[u8], offset: usize, count: usize) -> Option<Vec<f64>> {
            let byte_length = count.checked_mul(8)?;
            let values = $crate::wire::read::bytes_at(bytes, offset, byte_length)?;
            values.chunks_exact(8)
                .map(|value| Some(f64::$conversion(value.try_into().ok()?)))
                .collect()
        }

        #[doc = concat!("Takes consecutive ", $endian, " `f64` values.")]
        pub fn take_f64s(bytes: &[u8], position: &mut usize, count: usize) -> Option<Vec<f64>> {
            let values = f64s_at(bytes, *position, count)?;
            *position = position.checked_add(count.checked_mul(8)?)?;
            Some(values)
        }

        #[doc = concat!("Reads three consecutive ", $endian, " `f64` values.")]
        pub fn vec3_at(bytes: &[u8], offset: usize) -> Option<[f64; 3]> {
            Some([f64_at(bytes, offset)?, f64_at(bytes, offset.checked_add(8)?)?, f64_at(bytes, offset.checked_add(16)?)?])
        }

        #[doc = concat!("Takes three consecutive ", $endian, " `f64` values.")]
        pub fn take_vec3(bytes: &[u8], position: &mut usize) -> Option<[f64; 3]> {
            let value = vec3_at(bytes, *position)?;
            *position += 24;
            Some(value)
        }
    };
}

pub(crate) use readers;
