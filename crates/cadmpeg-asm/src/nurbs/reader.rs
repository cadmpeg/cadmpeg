// SPDX-License-Identifier: Apache-2.0
//! Byte-level readers, markers, and integer/float payload primitives shared across the NURBS decoders.

use crate::kernel_header::RefWidth;
use crate::sab::{int_le_at, vec3_le_at};
use cadmpeg_core::decode::View;
use cadmpeg_ir::math::{Point3, Vector3};

/// Millimetres per ASM model-space length unit (centimetres).
pub const LEN_TO_MM: f64 = 10.0;

pub(crate) fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm.is_finite() && norm > 0.0).then(|| vector.scale(1.0 / norm))
}

pub(crate) const NUBS_MARKER: &[u8] = b"\x0d\x04nubs";

const NURBS_MARKER: &[u8] = b"\x0d\x05nurbs";

/// Integer/ref payload widths to probe, `BinaryFile8` first. A wrong-width
/// parse cannot yield a false positive: in-range integers (degrees ≤ 20, knot
/// counts ≤ 1000) store zero high bytes, so an 8-byte read on a 4-byte stream
/// swallows the next tag byte into the value and fails the range check, while
/// a 4-byte read on an 8-byte stream leaves a zero byte where the next tag
/// must be and fails tag dispatch.
pub(crate) const INT_WIDTHS: [RefWidth; 2] = [RefWidth::Eight, RefWidth::Four];

/// Consume a `tag`-prefixed integer of `int_width` bytes at `*pos`, advancing
/// past it.
pub(crate) fn take_tagged_int(
    b: &[u8],
    pos: &mut usize,
    tag: u8,
    int_width: RefWidth,
) -> Option<i64> {
    if *b.get(*pos)? != tag {
        return None;
    }
    let v = int_le_at(b, *pos + 1, int_width)?;
    *pos += 1 + int_width.bytes();
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

/// Positions of the `nubs`/`nurbs` markers `b` itself owns, in order: those
/// outside every construction nested within `b`. A leading `0x0f` is `b`'s own
/// scope opening and is not counted as nesting.
///
/// A scope's members and the members of the constructions it nests are
/// indistinguishable to a raw byte scan, so a scan that ignores nesting reports
/// a nested support's cache as the scope's own.
pub(crate) fn owned_marker_positions(b: &[u8], int_width: RefWidth) -> Vec<usize> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut pos = usize::from(b.first() == Some(&0x0f));
    while pos < b.len() {
        match b[pos] {
            0x0f => depth += 1,
            0x10 => depth = depth.saturating_sub(1),
            _ => {
                if depth == 0 && marker_at(b, pos).is_some() {
                    out.push(pos);
                }
            }
        }
        match crate::nurbs::subtypes::next_token(b, pos, int_width) {
            Some(next) => pos = next,
            None => break,
        }
    }
    out
}

/// Positions of the B-spline markers owned by a complete record's unique
/// cache-bearing outer construction.
///
/// Record slices can contain auxiliary outer definitions before the carrier
/// construction. Enter every non-reference outer scope and admit its markers
/// only when exactly one such scope owns markers. Multiple cache-bearing outer
/// scopes are ambiguous and therefore not writable.
pub(crate) fn construction_marker_positions(b: &[u8], int_width: RefWidth) -> Vec<usize> {
    let candidates = crate::nurbs::subtypes::owned_subtype_defs(b, int_width)
        .into_iter()
        .filter(|(_, name)| *name != b"ref")
        .filter_map(|(start, _)| {
            let scope = crate::nurbs::subtypes::subtype_span(b, start, int_width)?;
            let positions = owned_marker_positions(scope, int_width)
                .into_iter()
                .map(|position| start + position)
                .collect::<Vec<_>>();
            (!positions.is_empty()).then_some(positions)
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return owned_marker_positions(b, int_width);
    }
    if candidates.len() != 1 {
        return Vec::new();
    }
    candidates.into_iter().next().unwrap_or_default()
}

/// Bounds for the shared ASM NURBS knot expansion check.
const MAX_NURBS_POLES: usize = 100_000;
const MAX_NURBS_DEGREE: usize = 20;
const MAX_EXPANDED_NURBS_KNOTS: usize = MAX_NURBS_POLES + MAX_NURBS_DEGREE + 1;

/// Checked expansion metadata for one unique-knot multiplicity table.
pub(crate) struct KnotExpansionLayout {
    pub(crate) n_poles: usize,
    pub(crate) expanded_len: usize,
    pub(crate) expanded_run_lengths: Vec<usize>,
}

pub(crate) fn checked_knot_layout(
    multiplicities: &[i64],
    degree: i64,
) -> Option<KnotExpansionLayout> {
    let degree = usize::try_from(degree)
        .ok()
        .filter(|degree| (1..=MAX_NURBS_DEGREE).contains(degree))?;
    let mut sum = 0usize;
    let mut expanded_len = 0usize;
    let mut expanded_run_lengths = Vec::with_capacity(multiplicities.len());
    for (index, &multiplicity) in multiplicities.iter().enumerate() {
        let multiplicity = usize::try_from(multiplicity).ok()?;
        sum = sum.checked_add(multiplicity)?;
        let endpoint_extra = usize::from(index == 0 || index + 1 == multiplicities.len());
        let run_length = multiplicity.checked_add(endpoint_extra)?;
        expanded_len = expanded_len.checked_add(run_length)?;
        if expanded_len > MAX_EXPANDED_NURBS_KNOTS {
            return None;
        }
        expanded_run_lengths.push(run_length);
    }
    let n_poles = sum.checked_sub(degree - 1)?;
    if !(2..=MAX_NURBS_POLES).contains(&n_poles) {
        return None;
    }
    let derived_max = n_poles.checked_add(degree)?.checked_add(1)?;
    (expanded_len <= derived_max).then_some(KnotExpansionLayout {
        n_poles,
        expanded_len,
        expanded_run_lengths,
    })
}

/// Unique native knot payload offsets.
pub struct KnotLayout {
    /// Payload offsets for unique knot values.
    pub value_offsets: Vec<usize>,
    /// Payload offsets for stored multiplicities.
    pub multiplicity_offsets: Vec<usize>,
}

/// Read a knot table of `n` `(knot, multiplicity)` pairs, returning the expanded
/// clamped knot vector and pole count `sum(mult) - (degree - 1)`.
pub(crate) fn read_knots(
    b: &[u8],
    pos: &mut usize,
    n: usize,
    degree: i64,
    int_width: RefWidth,
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
        knots.push(View::f64_le_at(b, *pos + 1)?);
        *pos += 9;
        multiplicity_offsets.push(*pos + 1);
        mults.push(take_tagged_int(b, pos, 0x04, int_width)?);
    }
    let expansion = checked_knot_layout(&mults, degree)?;
    let mut expanded = Vec::with_capacity(expansion.expanded_len);
    for (kv, &run_length) in knots.iter().zip(&expansion.expanded_run_lengths) {
        for _ in 0..run_length {
            expanded.push(*kv);
        }
    }
    Some((
        expanded,
        expansion.n_poles,
        KnotLayout {
            value_offsets,
            multiplicity_offsets,
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
            *comp = View::f64_le_at(b, *pos + 1)?;
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
    int_width: RefWidth,
) -> Option<Vec<usize>> {
    (*bytes.get(*position)? == 0x04).then_some(())?;
    let count = usize::try_from(int_le_at(bytes, *position + 1, int_width)?).ok()?;
    *position += 1 + int_width.bytes();
    (0..count)
        .map(|_| take_double_payload(bytes, position))
        .collect()
}

pub(crate) fn take_f64(bytes: &[u8], position: &mut usize) -> Option<f64> {
    if bytes.get(*position) != Some(&0x06) {
        return None;
    }
    let value = View::f64_le_at(bytes, *position + 1)?;
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
    unit_vector(Vector3::from(value))
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

pub(crate) fn take_native_string(
    bytes: &[u8],
    position: &mut usize,
    int_width: RefWidth,
) -> Option<String> {
    let (length, header) = match *bytes.get(*position)? {
        0x07 => (usize::from(*bytes.get(*position + 1)?), 2),
        0x08 => (usize::from(View::u16_le_at(bytes, *position + 1)?), 3),
        // The `0x09` length prefix is the stream's integer width, not a fixed
        // four bytes ([spec §2.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#21-tag-table)).
        0x09 => (
            usize::try_from(int_le_at(bytes, *position + 1, int_width)?).ok()?,
            1 + int_width.bytes(),
        ),
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
    let value = View::f64_le_at(bytes, *position + 1)?;
    *position += 9;
    Some(value)
}

pub(crate) fn take_optional_range_value(
    bytes: &[u8],
    position: &mut usize,
) -> Option<Nullable<f64>> {
    match bytes.get(*position)? {
        0x0a => {
            *position += 1;
            take_f64(bytes, position).map(Nullable::Value)
        }
        0x0b => {
            *position += 1;
            Some(Nullable::Null)
        }
        0x06 => take_f64(bytes, position).map(Nullable::Value),
        _ => None,
    }
}

pub(crate) fn take_native_vec3(bytes: &[u8], position: &mut usize, tag: u8) -> Option<[f64; 3]> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let values = vec3_le_at(bytes, *position + 1)?;
    *position += 25;
    Some(values)
}

#[cfg(test)]
mod string_width_tests {
    use super::take_native_string;
    use crate::kernel_header::RefWidth;

    /// A `0x09` string whose length prefix is the stream integer width.
    fn long_string_bytes(payload: &str, int_width: RefWidth) -> Vec<u8> {
        let mut bytes = vec![0x09];
        let mut length = (payload.len() as u64).to_le_bytes().to_vec();
        length.truncate(int_width.bytes());
        bytes.extend_from_slice(&length);
        bytes.extend_from_slice(payload.as_bytes());
        bytes
    }

    #[test]
    fn long_string_length_prefix_is_the_stream_int_width() {
        for int_width in [RefWidth::Four, RefWidth::Eight] {
            let bytes = long_string_bytes("#TS0200\ndegree 3", int_width);
            let mut position = 0;
            let value = take_native_string(&bytes, &mut position, int_width)
                .unwrap_or_else(|| panic!("string at width {int_width}"));
            assert_eq!(value, "#TS0200\ndegree 3");
            assert_eq!(position, bytes.len());
        }
    }

    #[test]
    fn long_string_read_at_the_wrong_width_never_yields_the_payload() {
        // A width-8 stream read at width 4 starts four bytes early; the
        // leading NUL bytes make the mismatch visible instead of parsing as
        // the intended payload.
        let bytes = long_string_bytes("#TS0200\ndegree 3", RefWidth::Eight);
        let mut position = 0;
        let value = take_native_string(&bytes, &mut position, RefWidth::Four);
        assert_ne!(value.as_deref(), Some("#TS0200\ndegree 3"));
    }
}
