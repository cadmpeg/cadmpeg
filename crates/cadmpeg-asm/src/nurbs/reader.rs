// SPDX-License-Identifier: Apache-2.0
//! Byte-level readers, markers, and integer/float payload primitives shared across the NURBS decoders.

use crate::kernel_header::RefWidth;
use crate::sab::{int_le_at, vec3_le_at};
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsPoles3, WeightedPole3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::NonZeroReal;

/// Poles read from one record, each pole carrying the weight read from its own
/// slot.
///
/// The record states a pole and its weight together, so the reader states rows
/// and no reader downstream pairs a pole lane with a weight lane.
#[derive(Debug, Clone)]
pub(super) enum ReadPoles3 {
    /// A polynomial record: its poles carry no weight.
    Polynomial(Vec<Point3>),
    /// A rational record: every pole carries its weight.
    Rational(Vec<WeightedPole3>),
}

impl ReadPoles3 {
    /// Start a read of `count` poles in the marker-selected form.
    pub(super) fn with_capacity(count: usize, rational: bool) -> Self {
        if rational {
            Self::Rational(Vec::with_capacity(count))
        } else {
            Self::Polynomial(Vec::with_capacity(count))
        }
    }

    /// Add one pole with the weight its own slot states.
    pub(super) fn push(&mut self, point: Point3, weight: f64) -> Option<()> {
        match self {
            Self::Polynomial(points) => points.push(point),
            Self::Rational(points) => points.push(WeightedPole3 {
                point,
                weight: NonZeroReal::new(weight)?,
            }),
        }
        Some(())
    }

    /// The poles as one lane, in the order they were read.
    pub(super) fn into_lane(self) -> NurbsPoles3 {
        match self {
            Self::Polynomial(points) => NurbsPoles3::Polynomial { points },
            Self::Rational(points) => NurbsPoles3::Rational { points },
        }
    }

    /// The poles as a `u`-major grid, read in the stream's `v`-major order.
    pub(super) fn into_transposed_grid(
        self,
        u_count: usize,
        v_count: usize,
    ) -> Option<NurbsPoleGrid> {
        fn transpose<T: Clone>(flat: &[T], u_count: usize, v_count: usize) -> Option<Vec<Vec<T>>> {
            (flat.len() == u_count.checked_mul(v_count)?).then_some(())?;
            (0..u_count)
                .map(|u| {
                    (0..v_count)
                        .map(|v| flat.get(v.checked_mul(u_count)?.checked_add(u)?).cloned())
                        .collect::<Option<Vec<_>>>()
                })
                .collect()
        }
        match self {
            Self::Polynomial(points) => Some(NurbsPoleGrid::Polynomial {
                rows: transpose(&points, u_count, v_count)?,
            }),
            Self::Rational(points) => Some(NurbsPoleGrid::Rational {
                rows: transpose(&points, u_count, v_count)?,
            }),
        }
    }
}

/// Millimetres per ASM model-space length unit (centimetres).
pub const LEN_TO_MM: f64 = 10.0;

pub(crate) const NUBS_MARKER: &[u8] = b"\x0d\x04nubs";

const NURBS_MARKER: &[u8] = b"\x0d\x05nurbs";

/// B-spline marker selecting whether each pole carries a weight component.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum BsplineMarker {
    /// Non-rational block.
    Nubs,
    /// Rational block with a homogeneous weight after each pole's coordinates.
    Nurbs,
}

impl BsplineMarker {
    /// Encoded identifier length, including its tag and length byte.
    pub(super) fn byte_len(self) -> usize {
        match self {
            Self::Nubs => NUBS_MARKER.len(),
            Self::Nurbs => NURBS_MARKER.len(),
        }
    }

    /// Doubles per model-space control point.
    pub(super) fn cp_dims(self) -> usize {
        match self {
            Self::Nubs => 3,
            Self::Nurbs => 4,
        }
    }

    /// Whether poles carry homogeneous weights.
    pub(super) fn rational(self) -> bool {
        self == Self::Nurbs
    }
}

/// Integer/ref payload widths to probe, `BinaryFile8` first. A wrong-width
/// parse cannot yield a false positive: in-range integers (degrees ≤ 20, knot
/// counts ≤ 1000) store zero high bytes, so an 8-byte read on a 4-byte stream
/// swallows the next tag byte into the value and fails the range check, while
/// a 4-byte read on an 8-byte stream leaves a zero byte where the next tag
/// must be and fails tag dispatch.
pub(super) const INT_WIDTHS: [RefWidth; 2] = [RefWidth::Eight, RefWidth::Four];

/// Consume a `tag`-prefixed integer of `int_width` bytes at `*pos`, advancing
/// past it.
pub(super) fn take_tagged_int(
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

/// The B-spline marker at byte `pos`, if any.
pub(super) fn marker_at(b: &[u8], pos: usize) -> Option<BsplineMarker> {
    if b[pos..].starts_with(NUBS_MARKER) {
        Some(BsplineMarker::Nubs)
    } else if b[pos..].starts_with(NURBS_MARKER) {
        Some(BsplineMarker::Nurbs)
    } else {
        None
    }
}

/// Positions of every `nubs`/`nurbs` marker in `b`, in order.
pub(super) fn marker_positions(b: &[u8]) -> Vec<usize> {
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
///
/// A `0x10` with no open scope is a malformed stream and is refused: pinning
/// the depth at zero would make every later marker read as one `b` owns.
fn owned_marker_positions(b: &[u8], int_width: RefWidth) -> Option<Vec<usize>> {
    let (out, balanced) = walk_owned_markers(b, int_width);
    balanced.then_some(out)
}

impl crate::nurbs::subtypes::SubtypeScope<'_> {
    /// Byte offsets of the B-spline markers the scope itself owns, relative to
    /// the scope's own start.
    ///
    /// Total: the unbalanced stream that [`owned_marker_positions`] refuses is
    /// a state this type cannot hold.
    pub(super) fn owned_marker_positions(&self, int_width: RefWidth) -> Vec<usize> {
        walk_owned_markers(self.bytes(), int_width).0
    }
}

/// The owned-marker walk, with the balance it observed.
///
/// The second element is `false` when the walk met a `0x10` that no `0x0f` in
/// `b` matches. A balanced scope always answers `true`.
fn walk_owned_markers(b: &[u8], int_width: RefWidth) -> (Vec<usize>, bool) {
    let mut out = Vec::new();
    let mut depth = 0usize;
    // The scope's own leading `0x0f` is skipped, so the close that matches it
    // is the one close this walk admits at depth zero.
    let mut outer = usize::from(b.first() == Some(&0x0f));
    let mut pos = outer;
    while pos < b.len() {
        match b[pos] {
            0x0f => depth += 1,
            0x10 => match depth.checked_sub(1) {
                Some(next) => depth = next,
                None => match outer.checked_sub(1) {
                    Some(next) => outer = next,
                    None => return (out, false),
                },
            },
            _ => {
                if depth == 0 && marker_at(b, pos).is_some() {
                    out.push(pos);
                }
            }
        }
        match crate::nurbs::subtypes::next_token(b, pos, int_width) {
            Some(next) => pos = next,
            None => return (out, false),
        }
    }
    (out, depth == 0 && outer == 0)
}

/// Positions of the B-spline markers owned by a complete record's unique
/// cache-bearing outer construction.
///
/// Record slices can contain auxiliary outer definitions before the carrier
/// construction. Enter every non-reference outer scope and admit its markers
/// only when exactly one such scope owns markers. Multiple cache-bearing outer
/// scopes are ambiguous and therefore not writable.
pub(super) fn construction_marker_positions(b: &[u8], int_width: RefWidth) -> Option<Vec<usize>> {
    let candidates = crate::nurbs::subtypes::owned_subtype_defs(b, int_width)?
        .into_iter()
        .filter(|(_, name)| *name != b"ref")
        .filter_map(|(start, _)| {
            let scope = crate::nurbs::subtypes::subtype_span(b, start, int_width)?;
            let positions = scope
                .owned_marker_positions(int_width)
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
        return Some(Vec::new());
    }
    candidates.into_iter().next()
}

/// Bounds for the shared ASM NURBS knot expansion check.
const MAX_NURBS_POLES: usize = 100_000;
const MAX_NURBS_DEGREE: usize = 20;
const MAX_EXPANDED_NURBS_KNOTS: usize = MAX_NURBS_POLES + MAX_NURBS_DEGREE + 1;

/// Checked expansion metadata for one unique-knot multiplicity table.
pub(in crate::nurbs) struct KnotExpansionLayout {
    pub(super) n_poles: usize,
    pub(super) expanded_run_lengths: Vec<usize>,
}

impl KnotExpansionLayout {
    pub(super) fn expanded_len(&self) -> usize {
        self.expanded_run_lengths.iter().sum()
    }
}

pub(super) fn checked_knot_layout(
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
        expanded_run_lengths,
    })
}

/// Unique native knot payload offsets.
pub struct KnotLayout {
    /// Payload offsets for unique knot values.
    pub value_offsets: Vec<usize>,
}

/// Read a knot table of `n` `(knot, multiplicity)` pairs, returning the expanded
/// clamped knot vector and pole count `sum(mult) - (degree - 1)`.
pub(super) fn read_knots(
    b: &[u8],
    pos: &mut usize,
    n: usize,
    degree: i64,
    int_width: RefWidth,
) -> Option<(Vec<f64>, usize, KnotLayout)> {
    let mut knots = Vec::new();
    let mut mults = Vec::new();
    let mut value_offsets = Vec::new();
    for _ in 0..n {
        if *b.get(*pos)? != 0x06 {
            return None;
        }
        value_offsets.push(*pos + 1);
        knots.push(View::f64_le_at(b, *pos + 1)?);
        *pos += 9;
        mults.push(take_tagged_int(b, pos, 0x04, int_width)?);
    }
    let expansion = checked_knot_layout(&mults, degree)?;
    let mut expanded = Vec::with_capacity(expansion.expanded_len());
    for (kv, &run_length) in knots.iter().zip(&expansion.expanded_run_lengths) {
        for _ in 0..run_length {
            expanded.push(*kv);
        }
    }
    Some((expanded, expansion.n_poles, KnotLayout { value_offsets }))
}

/// Read `count` control points in the marker-selected form at `*pos`. Returns the
/// scaled `(x, y, z)` positions and, for rational blocks, the weights.
pub(super) fn read_control_points(
    b: &[u8],
    pos: &mut usize,
    count: usize,
    marker: BsplineMarker,
) -> Option<ReadPoles3> {
    let mut poles = ReadPoles3::with_capacity(count, marker.rational());
    for _ in 0..count {
        let mut comps = [0.0f64; 4];
        for comp in comps.iter_mut().take(marker.cp_dims()) {
            if *b.get(*pos)? != 0x06 {
                return None;
            }
            *comp = View::f64_le_at(b, *pos + 1)?;
            *pos += 9;
        }
        poles.push(
            Point3::new(
                comps[0] * LEN_TO_MM,
                comps[1] * LEN_TO_MM,
                comps[2] * LEN_TO_MM,
            ),
            comps[3],
        )?;
    }
    Some(poles)
}

/// CLOSURE enum value `2` denotes a periodic parametric direction.
pub(super) fn is_periodic(enum_val: i64) -> bool {
    enum_val == 2
}

pub(super) enum Nullable<T> {
    Null,
    Value(T),
}

impl<T> Nullable<T> {
    pub(super) fn value(self) -> Option<T> {
        match self {
            Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }
}

pub(super) fn take_double_payload(bytes: &[u8], position: &mut usize) -> Option<usize> {
    (*bytes.get(*position)? == 0x06).then_some(())?;
    let payload = *position + 1;
    bytes.get(payload..payload + 8)?;
    *position = payload + 8;
    Some(payload)
}

pub(super) fn take_float_array_payloads(
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

pub(super) fn take_f64(bytes: &[u8], position: &mut usize) -> Option<f64> {
    if bytes.get(*position) != Some(&0x06) {
        return None;
    }
    let value = View::f64_le_at(bytes, *position + 1)?;
    *position += 9;
    Some(value)
}

pub(super) fn take_bool(bytes: &[u8], position: &mut usize) -> Option<bool> {
    let value = match bytes.get(*position)? {
        0x0a => true,
        0x0b => false,
        _ => return None,
    };
    *position += 1;
    Some(value)
}

pub(super) fn normalized(value: [f64; 3]) -> Option<Vector3> {
    cadmpeg_ir::features::FiniteVector3::new(Vector3::from(value))?.unit_nonzero()
}

pub(super) fn take_native_ident(bytes: &[u8], position: &mut usize) -> Option<String> {
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

pub(super) fn take_native_string(
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

pub(super) fn take_range_value(bytes: &[u8], position: &mut usize) -> Option<f64> {
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

pub(super) fn take_optional_range_value(
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

pub(super) fn take_native_vec3(bytes: &[u8], position: &mut usize, tag: u8) -> Option<[f64; 3]> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let values = vec3_le_at(bytes, *position + 1)?;
    *position += 25;
    Some(values)
}

#[cfg(test)]
mod marker_ownership_tests {
    use super::{owned_marker_positions, NUBS_MARKER};
    use crate::kernel_header::RefWidth;

    #[test]
    fn raw_marker_walk_refuses_unclosed_own_and_nested_scopes() {
        let mut bytes = vec![0x0f];
        bytes.extend_from_slice(NUBS_MARKER);
        assert_eq!(owned_marker_positions(&bytes, RefWidth::Four), None);
        bytes.push(0x10);
        assert_eq!(
            owned_marker_positions(&bytes, RefWidth::Four),
            Some(vec![1])
        );
        bytes.push(0x0f);
        assert_eq!(owned_marker_positions(&bytes, RefWidth::Four), None);
    }
}

#[cfg(test)]
mod string_width_tests {
    use super::{take_native_string, ReadPoles3};
    use crate::kernel_header::RefWidth;
    use cadmpeg_ir::geometry::nurbs::NurbsPoleGrid;
    use cadmpeg_ir::math::Point3;

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

    /// The reader states one row per pole, so a weight lane that is shorter
    /// than the pole lane has no spelling here and there is no refusal for a
    /// reader to drop: a pole whose own slot holds an unusable weight ends the
    /// read instead.
    #[test]
    fn read_poles_state_rows_and_refuse_an_unusable_weight() {
        let mut poles = ReadPoles3::with_capacity(2, true);
        assert!(poles.push(Point3::new(0.0, 0.0, 0.0), 1.0).is_some());
        assert!(poles.push(Point3::new(1.0, 0.0, 0.0), 0.0).is_none());
        let ReadPoles3::Rational(rows) = poles else {
            panic!("a rational read states weighted rows");
        };
        assert_eq!(rows.len(), 1);

        let mut grid = ReadPoles3::with_capacity(4, false);
        for index in 0..4 {
            assert!(grid
                .push(Point3::new(f64::from(index), 0.0, 0.0), 1.0)
                .is_some());
        }
        let NurbsPoleGrid::Polynomial { rows } = grid
            .into_transposed_grid(2, 2)
            .expect("a four-pole read states a two-by-two grid")
        else {
            panic!("a polynomial read states unweighted rows");
        };
        assert_eq!(rows[0][0].x, 0.0);
        assert_eq!(rows[0][1].x, 2.0);
        assert_eq!(rows[1][0].x, 1.0);
        assert_eq!(rows[1][1].x, 3.0);
    }
}
