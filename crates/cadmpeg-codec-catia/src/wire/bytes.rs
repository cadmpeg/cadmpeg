//! Free-function byte readers shared across CATIA record families.
//!
//! Absolute-offset scalar and reference readers used by the per-family scan
//! loops: finite-checked `f64` scalars, points, and vectors; 24-bit integer
//! decoding; persistent and allocation reference tokens; and fixed-size `f64`
//! array reads. The persistent and allocation tokens fall back to the compact
//! unsigned reader in [`super::tokens`].

use super::tokens::compact_uint;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::wire::le::{f64_at, u16_at as u16_le};

pub(crate) fn finite_f64_lane(bytes: &[u8]) -> Option<Vec<f64>> {
    if !bytes.len().is_multiple_of(8) {
        return None;
    }
    bytes
        .chunks_exact(8)
        .map(|bytes| {
            let value = f64::from_le_bytes(bytes.try_into().ok()?);
            value.is_finite().then_some(value)
        })
        .collect()
}

pub(crate) fn read_f64_array<const N: usize>(data: &[u8], start: usize) -> Option<[f64; N]> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = f64_le(data, start.checked_add(index.checked_mul(8)?)?)?;
    }
    Some(values)
}

pub(crate) fn f64_le(bytes: &[u8], at: usize) -> Option<f64> {
    let value = f64_at(bytes, at)?;
    value.is_finite().then_some(value)
}

pub(crate) fn f64_point(bytes: &[u8], at: usize) -> Option<Point3> {
    Some(Point3::new(
        f64_le(bytes, at)?,
        f64_le(bytes, at + 8)?,
        f64_le(bytes, at + 16)?,
    ))
}

pub(crate) fn f64_vector(bytes: &[u8], at: usize) -> Option<Vector3> {
    Some(Vector3::new(
        f64_le(bytes, at)?,
        f64_le(bytes, at + 8)?,
        f64_le(bytes, at + 16)?,
    ))
}

pub(crate) fn u32_le_24(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.get(at)?,
        *bytes.get(at + 1)?,
        *bytes.get(at + 2)?,
        0,
    ]))
}

pub(crate) fn persistent_ref(bytes: &[u8], at: &mut usize) -> Option<u32> {
    if bytes.get(*at) == Some(&0x0a) {
        let value = u32::from(u16_le(bytes, *at + 1)?);
        *at += 3;
        Some(value)
    } else {
        compact_uint(bytes, at)
    }
}

pub(crate) fn allocation_ref(bytes: &[u8], at: &mut usize) -> Option<u32> {
    match *bytes.get(*at)? {
        0x06 => {
            let value = u32::from(*bytes.get(*at + 1)?);
            *at += 2;
            Some(value)
        }
        0x0a => {
            let value = u32::from(u16_le(bytes, *at + 1)?);
            *at += 3;
            Some(value)
        }
        byte if byte != 0 && matches!(byte % 4, 2 | 3) => {
            *at += 1;
            Some(u32::from(byte))
        }
        _ => compact_uint(bytes, at),
    }
}
