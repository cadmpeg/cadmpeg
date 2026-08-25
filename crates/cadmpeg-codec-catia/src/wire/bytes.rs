//! Free-function byte readers shared across CATIA record families.
//!
//! Absolute-offset scalar and reference readers used by the per-family scan
//! loops: finite-checked `f64` scalars, points, and vectors; 24-bit and
//! compact integer decoders; persistent and allocation reference tokens; and
//! fixed-size `f64` array reads.

use super::cursor::Cursor;
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::{Point3, Vector3};

pub(crate) fn finite_f64_lane(bytes: &[u8]) -> Option<Vec<f64>> {
    if !bytes.len().is_multiple_of(8) {
        return None;
    }
    let mut view = View::over_retained(bytes);
    let mut values = Vec::with_capacity(bytes.len() / 8);
    while !view.is_empty() {
        let value = view.f64_le()?;
        if !value.is_finite() {
            return None;
        }
        values.push(value);
    }
    Some(values)
}

pub(crate) fn read_f64_array<const N: usize>(data: &[u8], start: usize) -> Option<[f64; N]> {
    let mut values = [0.0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = f64_le(data, start.checked_add(index.checked_mul(8)?)?)?;
    }
    Some(values)
}

pub(crate) fn f64_le(bytes: &[u8], at: usize) -> Option<f64> {
    let value = View::f64_le_at(bytes, at)?;
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
    View::u24_le_at(bytes, at)
}

pub(crate) fn compact_int(bytes: &[u8], at: &mut usize) -> Option<u32> {
    let mut cursor = Cursor::new_at(bytes, *at);
    let value = cursor.compact_uint()?;
    *at = cursor.position();
    Some(value)
}

pub(crate) fn persistent_ref(bytes: &[u8], at: &mut usize) -> Option<u32> {
    if bytes.get(*at) == Some(&0x0a) {
        let value = u32::from(View::u16_le_at(bytes, *at + 1)?);
        *at += 3;
        Some(value)
    } else {
        compact_int(bytes, at)
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
            let value = u32::from(View::u16_le_at(bytes, *at + 1)?);
            *at += 3;
            Some(value)
        }
        byte if byte != 0 && matches!(byte % 4, 2 | 3) => {
            *at += 1;
            Some(u32::from(byte))
        }
        _ => compact_int(bytes, at),
    }
}
