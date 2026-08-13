// SPDX-License-Identifier: Apache-2.0
//! Endian and compact-integer encodings used by synthetic CATPart fixtures.

#![allow(clippy::unwrap_used)]

pub(crate) fn be32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

pub(crate) fn le_f32(v: f32) -> [u8; 4] {
    v.to_le_bytes()
}

pub(crate) fn be_f32(v: f32) -> [u8; 4] {
    v.to_be_bytes()
}

pub(crate) fn le_f64(v: f64) -> [u8; 8] {
    v.to_le_bytes()
}

pub(crate) fn compact_uint_bytes(value: u32) -> Vec<u8> {
    if value <= 63 {
        return vec![u8::try_from(value * 4 + 1).expect("single-byte compact integer")];
    }
    let width = if u16::try_from(value).is_ok() {
        2
    } else if value <= 0x00ff_ffff {
        3
    } else {
        4
    };
    let mut bytes = vec![u8::try_from(width * 4).expect("compact integer width")];
    bytes.extend_from_slice(&value.to_le_bytes()[..width]);
    bytes
}
