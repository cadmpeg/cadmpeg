// SPDX-License-Identifier: Apache-2.0
//! Byte-level readers, markers, and integer/float payload primitives shared across the NURBS decoders.

use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::wire::le::{f64_at as read_f64, int_at as read_int, u16_at, u32_at};

/// Millimetres per ASM model-space length unit (centimetres).
pub(crate) const LEN_TO_MM: f64 = 10.0;

pub(crate) fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0)
        .then(|| Vector3::new(vector.x / norm, vector.y / norm, vector.z / norm))
}

pub(crate) const NUBS_MARKER: &[u8] = b"\x0d\x04nubs";

const NURBS_MARKER: &[u8] = b"\x0d\x05nurbs";

/// Integer/ref payload widths to probe, `BinaryFile8` first. A wrong-width
/// parse cannot yield a false positive: in-range integers (degrees ≤ 20, knot
/// counts ≤ 1000) store zero high bytes, so an 8-byte read on a 4-byte stream
/// swallows the next tag byte into the value and fails the range check, while
/// a 4-byte read on an 8-byte stream leaves a zero byte where the next tag
/// must be and fails tag dispatch.
pub(crate) const INT_WIDTHS: [usize; 2] = [8, 4];

/// Read an `int_width`-byte little-endian signed integer.
/// Consume a `tag`-prefixed integer of `int_width` bytes at `*pos`, advancing
/// past it.
pub(crate) fn take_tagged_int(b: &[u8], pos: &mut usize, tag: u8, int_width: usize) -> Option<i64> {
    if *b.get(*pos)? != tag {
        return None;
    }
    let v = read_int(b, *pos + 1, int_width)?;
    *pos += 1 + int_width;
    Some(v)
}

/// The B-spline marker at `pos`, if any: `(control-point dimension, byte length
/// of the marker, rational?)`.
pub(crate) fn marker_at(b: &[u8], pos: usize) -> Option<(usize, usize, bool)> {
    if b[pos..].starts_with(NUBS_MARKER) {
        Some((3, NUBS_MARKER.len(), false))
    } else if b[pos..].starts_with(NURBS_MARKER) {
        Some((4, NURBS_MARKER.len(), true))
    } else {
        None
    }
}

/// Positions of every `nubs`/`nurbs` marker in `b`, in order.
pub(crate) fn marker_positions(b: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if b.len() < NUBS_MARKER.len() {
        return out;
    }
    for pos in 0..=b.len() - NUBS_MARKER.len() {
        if marker_at(b, pos).is_some() {
            out.push(pos);
        }
    }
    out
}

/// Read a knot table of `n` `(knot, multiplicity)` pairs, returning the expanded
/// clamped knot vector and pole count `sum(mult) - (degree - 1)`.
pub(crate) struct KnotLayout {
    pub(crate) value_offsets: Vec<usize>,
    pub(crate) multiplicity_offsets: Vec<usize>,
    pub(crate) expanded_run_lengths: Vec<usize>,
}

pub(crate) fn read_knots(
    b: &[u8],
    pos: &mut usize,
    n: usize,
    degree: i64,
    int_width: usize,
) -> Option<(Vec<f64>, usize, KnotLayout)> {
    let mut knots = Vec::new();
    let mut mults = Vec::new();
    let mut value_offsets = Vec::new();
    let mut multiplicity_offsets = Vec::new();
    for _ in 0..n {
        if *b.get(*pos)? != 0x06 {
            return None;
        }
        value_offsets.push(*pos + 1);
        knots.push(read_f64(b, *pos + 1)?);
        *pos += 9;
        multiplicity_offsets.push(*pos + 1);
        mults.push(take_tagged_int(b, pos, 0x04, int_width)?);
    }
    let sum: i64 = mults.iter().sum();
    let n_poles = sum - (degree - 1);
    if !(2..=100_000).contains(&n_poles) {
        return None;
    }
    let mut expanded = Vec::new();
    let mut expanded_run_lengths = Vec::new();
    for (i, (kv, m)) in knots.iter().zip(&mults).enumerate() {
        let extra = i64::from(i == 0 || i == n - 1);
        let run_length = usize::try_from((*m + extra).max(0)).ok()?;
        expanded_run_lengths.push(run_length);
        for _ in 0..run_length {
            expanded.push(*kv);
        }
    }
    Some((
        expanded,
        n_poles as usize,
        KnotLayout {
            value_offsets,
            multiplicity_offsets,
            expanded_run_lengths,
        },
    ))
}

/// Read `count` control points of `cp_dims` doubles each at `*pos`. Returns the
/// scaled `(x, y, z)` positions and, for rational blocks, the weights.
pub(crate) fn read_control_points(
    b: &[u8],
    pos: &mut usize,
    count: usize,
    cp_dims: usize,
) -> Option<(Vec<Point3>, Option<Vec<f64>>)> {
    let mut points = Vec::with_capacity(count);
    let mut weights = if cp_dims == 4 {
        Some(Vec::with_capacity(count))
    } else {
        None
    };
    for _ in 0..count {
        let mut comps = [0.0f64; 4];
        for comp in comps.iter_mut().take(cp_dims) {
            if *b.get(*pos)? != 0x06 {
                return None;
            }
            *comp = read_f64(b, *pos + 1)?;
            *pos += 9;
        }
        points.push(Point3::new(
            comps[0] * LEN_TO_MM,
            comps[1] * LEN_TO_MM,
            comps[2] * LEN_TO_MM,
        ));
        if let Some(w) = weights.as_mut() {
            w.push(comps[3]);
        }
    }
    Some((points, weights))
}

/// CLOSURE enum value `2` denotes a periodic parametric direction.
pub(crate) fn is_periodic(enum_val: i64) -> bool {
    enum_val == 2
}

pub(crate) enum Nullable<T> {
    Null,
    Value(T),
}

impl<T> Nullable<T> {
    pub(crate) fn value(self) -> Option<T> {
        match self {
            Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }
}

pub(crate) fn take_double_payload(bytes: &[u8], position: &mut usize) -> Option<usize> {
    (*bytes.get(*position)? == 0x06).then_some(())?;
    let payload = *position + 1;
    bytes.get(payload..payload + 8)?;
    *position = payload + 8;
    Some(payload)
}

pub(crate) fn take_float_array_payloads(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Vec<usize>> {
    (*bytes.get(*position)? == 0x04).then_some(())?;
    let raw = bytes.get(*position + 1..*position + 1 + int_width)?;
    let count = match int_width {
        8 => usize::try_from(i64::from_le_bytes(raw.try_into().ok()?)).ok()?,
        4 => usize::try_from(i32::from_le_bytes(raw.try_into().ok()?)).ok()?,
        _ => return None,
    };
    *position += 1 + int_width;
    (0..count)
        .map(|_| take_double_payload(bytes, position))
        .collect()
}

pub(crate) fn take_f64(bytes: &[u8], position: &mut usize) -> Option<f64> {
    if bytes.get(*position) != Some(&0x06) {
        return None;
    }
    let value = read_f64(bytes, *position + 1)?;
    *position += 9;
    Some(value)
}

pub(crate) fn take_bool(bytes: &[u8], position: &mut usize) -> Option<bool> {
    let value = match bytes.get(*position)? {
        0x0a => true,
        0x0b => false,
        _ => return None,
    };
    *position += 1;
    Some(value)
}

pub(crate) fn normalized(value: [f64; 3]) -> Option<Vector3> {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    (length.is_finite() && length > 0.0)
        .then(|| Vector3::new(value[0] / length, value[1] / length, value[2] / length))
}

pub(crate) fn take_native_ident(bytes: &[u8], position: &mut usize) -> Option<String> {
    if !matches!(bytes.get(*position), Some(0x0d | 0x0e)) {
        return None;
    }
    let length = usize::from(*bytes.get(*position + 1)?);
    let start = *position + 2;
    let end = start.checked_add(length)?;
    let value = String::from_utf8(bytes.get(start..end)?.to_vec()).ok()?;
    *position = end;
    Some(value)
}

pub(crate) fn take_float_array(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Vec<f64>> {
    let count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(*position) != Some(&0x06) {
            return None;
        }
        values.push(read_f64(bytes, *position + 1)?);
        *position += 9;
    }
    Some(values)
}

pub(crate) fn take_native_string(bytes: &[u8], position: &mut usize) -> Option<String> {
    let (length, header) = match *bytes.get(*position)? {
        0x07 => (usize::from(*bytes.get(*position + 1)?), 2),
        0x08 => (usize::from(u16_at(bytes, *position + 1)?), 3),
        0x09 => (usize::try_from(u32_at(bytes, *position + 1)?).ok()?, 5),
        _ => return None,
    };
    let start = *position + header;
    let end = start.checked_add(length)?;
    let value = String::from_utf8(bytes.get(start..end)?.to_vec()).ok()?;
    *position = end;
    Some(value)
}

pub(crate) fn take_range_value(bytes: &[u8], position: &mut usize) -> Option<f64> {
    if matches!(bytes.get(*position), Some(0x0a | 0x0b)) {
        *position += 1;
    }
    if bytes.get(*position) != Some(&0x06) {
        return None;
    }
    let value = read_f64(bytes, *position + 1)?;
    *position += 9;
    Some(value)
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is an absent bound.
pub(crate) fn take_optional_range_value(bytes: &[u8], position: &mut usize) -> Option<Option<f64>> {
    match bytes.get(*position)? {
        0x0a => {
            *position += 1;
            take_f64(bytes, position).map(Some)
        }
        0x0b => {
            *position += 1;
            Some(None)
        }
        0x06 => take_f64(bytes, position).map(Some),
        _ => None,
    }
}

pub(crate) fn take_native_vec3(bytes: &[u8], position: &mut usize, tag: u8) -> Option<[f64; 3]> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let values = [
        read_f64(bytes, *position + 1)?,
        read_f64(bytes, *position + 9)?,
        read_f64(bytes, *position + 17)?,
    ];
    *position += 25;
    Some(values)
}
