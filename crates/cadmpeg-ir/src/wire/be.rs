// SPDX-License-Identifier: Apache-2.0
//! Checked big-endian primitive readers shared by binary codecs.
#![deny(clippy::disallowed_methods)]

use crate::wire::read::readers;

readers!(from_be_bytes, "big-endian";
    (u16_at, take_u16, u16, 2), (i16_at, take_i16, i16, 2),
    (u32_at, take_u32, u32, 4), (i32_at, take_i32, i32, 4),
    (u64_at, take_u64, u64, 8), (i64_at, take_i64, i64, 8),
    (f32_at, take_f32, f32, 4), (f64_at, take_f64, f64, 8),
);

#[cfg(test)]
mod tests {
    use super::{f64_at, f64s_at, take_f64s, take_u32, take_vec3};

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
            bytes.extend_from_slice(&value.to_be_bytes());
        }
        assert_eq!(f64_at(&bytes, 8), Some(2.0));
        assert_eq!(take_vec3(&bytes, &mut 0), Some([1.0, 2.0, 3.0]));
        assert_eq!(f64s_at(&bytes, 8, 2), Some(vec![2.0, 3.0]));
        let mut position = 8;
        assert_eq!(take_f64s(&bytes, &mut position, 2), Some(vec![2.0, 3.0]));
        assert_eq!(position, 24);
    }
}
