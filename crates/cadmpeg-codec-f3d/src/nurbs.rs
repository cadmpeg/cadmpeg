// SPDX-License-Identifier: Apache-2.0
//! Decode cached B-spline blocks from spline and procedural SAB records.
//!
//! A `0x0d`-tagged `nubs` marker introduces a non-rational block; `nurbs`
//! introduces a rational block. Surface blocks contain two degrees, closure and
//! singularity enums, U and V knot tables, and a control grid. Curve blocks
//! contain one degree, a closure enum, one knot table, and a control polygon.
//! The token after the first degree distinguishes the two forms.
//!
//! Spline surfaces and procedural curves store solved geometry in these caches.
//! The public decode functions accept one record's bytes, with variants that
//! also follow references through the active slice's subtype table.
//!
//! Endpoint knot multiplicities are stored as `degree` rather than
//! `degree + 1`; the clamped knot vector is recovered by adding one at each
//! end. Control-point x/y/z are model-space lengths converted from centimetres
//! to millimetres; knots and rational weights are not scaled. Surface control
//! grids are stored v-major (v outer, u inner) and are transposed to the IR's
//! u-major order.
//!
//! Integer-family payloads (`0x04` int, `0x0c` ref, `0x15` enum) are 4 bytes in
//! `BinaryFile4` streams and 8 in `BinaryFile8` ([spec §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#3-asm-binary-header)); doubles are always
//! 8. Record bytes omit the stream width, so each decoder tests both layouts
//! and validates tags, degrees, counts, and block extents.

use cadmpeg_ir::geometry::{BlendCrossSection, BlendRadiusLaw, NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

/// Millimetres per ASM model-space length unit (centimetres).
const LEN_TO_MM: f64 = 10.0;

const NUBS_MARKER: &[u8] = b"\x0d\x04nubs";
const NURBS_MARKER: &[u8] = b"\x0d\x05nurbs";

/// Integer/ref payload widths to probe, `BinaryFile8` first. A wrong-width
/// parse cannot yield a false positive: in-range integers (degrees ≤ 20, knot
/// counts ≤ 1000) store zero high bytes, so an 8-byte read on a 4-byte stream
/// swallows the next tag byte into the value and fails the range check, while
/// a 4-byte read on an 8-byte stream leaves a zero byte where the next tag
/// must be and fails tag dispatch.
const INT_WIDTHS: [usize; 2] = [8, 4];

/// Read an `int_width`-byte little-endian signed integer.
fn read_int(b: &[u8], p: usize, int_width: usize) -> Option<i64> {
    if int_width == 4 {
        b.get(p..p + 4).map(|s| {
            i64::from(i32::from_le_bytes(
                s.try_into()
                    .expect("invariant: b.get(p..p+4) is a 4-byte slice"),
            ))
        })
    } else {
        b.get(p..p + 8).map(|s| {
            i64::from_le_bytes(
                s.try_into()
                    .expect("invariant: b.get(p..p+8) is an 8-byte slice"),
            )
        })
    }
}

fn read_f64(b: &[u8], p: usize) -> Option<f64> {
    b.get(p..p + 8).map(|s| {
        f64::from_le_bytes(
            s.try_into()
                .expect("invariant: b.get(p..p+8) is an 8-byte slice"),
        )
    })
}

/// Consume a `tag`-prefixed integer of `int_width` bytes at `*pos`, advancing
/// past it.
fn take_tagged_int(b: &[u8], pos: &mut usize, tag: u8, int_width: usize) -> Option<i64> {
    if *b.get(*pos)? != tag {
        return None;
    }
    let v = read_int(b, *pos + 1, int_width)?;
    *pos += 1 + int_width;
    Some(v)
}

/// The B-spline marker at `pos`, if any: `(control-point dimension, byte length
/// of the marker, rational?)`.
fn marker_at(b: &[u8], pos: usize) -> Option<(usize, usize, bool)> {
    if b[pos..].starts_with(NUBS_MARKER) {
        Some((3, NUBS_MARKER.len(), false))
    } else if b[pos..].starts_with(NURBS_MARKER) {
        Some((4, NURBS_MARKER.len(), true))
    } else {
        None
    }
}

/// Positions of every `nubs`/`nurbs` marker in `b`, in order.
fn marker_positions(b: &[u8]) -> Vec<usize> {
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
struct KnotLayout {
    value_offsets: Vec<usize>,
    multiplicity_offsets: Vec<usize>,
    expanded_run_lengths: Vec<usize>,
}

fn read_knots(
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
fn read_control_points(
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
fn is_periodic(enum_val: i64) -> bool {
    enum_val == 2
}

/// Decode a surface `nubs`/`nurbs` block at `marker_pos`, or `None` if the bytes
/// there are not a well-formed surface block.
struct DecodedSurfaceBlock {
    surface: NurbsSurface,
    end: usize,
    control_value_offsets: Vec<usize>,
    rational: bool,
    u_knot_layout: KnotLayout,
    v_knot_layout: KnotLayout,
    periodic_value_offsets: [usize; 2],
    degree_value_offsets: [usize; 2],
}

fn decode_surface_block(
    b: &[u8],
    marker_pos: usize,
    int_width: usize,
) -> Option<DecodedSurfaceBlock> {
    let (cp_dims, marker_len, rational) = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker_len;

    let degree_u_offset = pos + 1;
    let degree_u = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    let degree_v_offset = pos + 1;
    let degree_v = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=20).contains(&degree_u) || !(1..=20).contains(&degree_v) {
        return None;
    }
    // Some caches carry an optional scope identifier (`u`/`v`/`both`) before the
    // enum block; skip it so knot counts stay aligned.
    if b.get(pos) == Some(&0x0d) {
        let len = *b.get(pos + 1)? as usize;
        pos += 2 + len;
    }
    let mut enums = [0i64; 4];
    let mut enum_value_offsets = [0usize; 4];
    for (ordinal, e) in enums.iter_mut().enumerate() {
        enum_value_offsets[ordinal] = pos + 1;
        *e = take_tagged_int(b, &mut pos, 0x15, int_width)?;
    }
    let n_uniq_u = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    let n_uniq_v = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=1000).contains(&n_uniq_u) || !(1..=1000).contains(&n_uniq_v) {
        return None;
    }

    let (u_knots, n_poles_u, u_knot_layout) =
        read_knots(b, &mut pos, n_uniq_u as usize, degree_u, int_width)?;
    let (v_knots, n_poles_v, v_knot_layout) =
        read_knots(b, &mut pos, n_uniq_v as usize, degree_v, int_width)?;
    if n_poles_u.checked_mul(n_poles_v).is_none_or(|n| n > 200_000) {
        return None;
    }

    // Grid is stored v-major (v outer, u inner); transpose to the IR's u-major
    // order where index `u * v_count + v` is pole `(u, v)`.
    let control_start = pos;
    let (flat, flat_w) = read_control_points(b, &mut pos, n_poles_u * n_poles_v, cp_dims)?;
    let control_value_offsets = (0..n_poles_u * n_poles_v * cp_dims)
        .map(|ordinal| control_start + ordinal * 9 + 1)
        .collect();
    let mut control_points = vec![Point3::new(0.0, 0.0, 0.0); n_poles_u * n_poles_v];
    let mut weights = flat_w.as_ref().map(|_| vec![0.0f64; n_poles_u * n_poles_v]);
    for v in 0..n_poles_v {
        for u in 0..n_poles_u {
            let file_idx = v * n_poles_u + u;
            let ir_idx = u * n_poles_v + v;
            control_points[ir_idx] = flat[file_idx];
            if let (Some(w), Some(fw)) = (weights.as_mut(), flat_w.as_ref()) {
                w[ir_idx] = fw[file_idx];
            }
        }
    }

    Some(DecodedSurfaceBlock {
        surface: NurbsSurface {
            u_degree: degree_u as u32,
            v_degree: degree_v as u32,
            u_knots,
            v_knots,
            u_count: n_poles_u as u32,
            v_count: n_poles_v as u32,
            control_points,
            weights,
            u_periodic: is_periodic(enums[0]),
            v_periodic: is_periodic(enums[1]),
        },
        end: pos,
        control_value_offsets,
        rational,
        u_knot_layout,
        v_knot_layout,
        periodic_value_offsets: [enum_value_offsets[0], enum_value_offsets[1]],
        degree_value_offsets: [degree_u_offset, degree_v_offset],
    })
}

/// Writable value offsets for the final valid surface cache in one carrier record.
pub(crate) struct SurfacePatchLayout {
    /// Native v-major tagged-double payload offsets, excluding each tag byte.
    pub(crate) control_value_offsets: Vec<usize>,
    /// Whether every pole includes a fourth rational weight component.
    pub(crate) rational: bool,
    /// Pole count in the u direction.
    pub(crate) u_count: usize,
    /// Pole count in the v direction.
    pub(crate) v_count: usize,
    /// Native payload offsets and expanded run lengths for U knots.
    pub(crate) u_knots: KnotPatchLayout,
    /// Native payload offsets and expanded run lengths for V knots.
    pub(crate) v_knots: KnotPatchLayout,
    /// Offset immediately after the final control component.
    pub(crate) end: usize,
    /// Payload offsets for the U/V closure enums.
    pub(crate) periodic_value_offsets: [usize; 2],
    /// Payload offsets for the U/V degree integers.
    pub(crate) degree_value_offsets: [usize; 2],
}

/// Unique native knot payload offsets.
pub(crate) struct KnotPatchLayout {
    /// Payload offsets for unique knot values.
    pub(crate) value_offsets: Vec<usize>,
    /// Payload offsets for stored multiplicities.
    pub(crate) multiplicity_offsets: Vec<usize>,
    /// Repetition count of each unique value in the expanded IR vector.
    #[expect(dead_code)]
    pub(crate) expanded_run_lengths: Vec<usize>,
}

impl From<KnotLayout> for KnotPatchLayout {
    fn from(value: KnotLayout) -> Self {
        Self {
            value_offsets: value.value_offsets,
            multiplicity_offsets: value.multiplicity_offsets,
            expanded_run_lengths: value.expanded_run_lengths,
        }
    }
}

/// Locate the final valid `nubs`/`nurbs` surface block in a carrier record.
pub(crate) fn final_surface_patch_layout(record: &[u8]) -> Option<SurfacePatchLayout> {
    let decoded = INT_WIDTHS.into_iter().find_map(|int_width| {
        marker_positions(record)
            .into_iter()
            .filter_map(|position| decode_surface_block(record, position, int_width))
            .next_back()
    })?;
    Some(SurfacePatchLayout {
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        u_count: decoded.surface.u_count as usize,
        v_count: decoded.surface.v_count as usize,
        u_knots: decoded.u_knot_layout.into(),
        v_knots: decoded.v_knot_layout.into(),
        end: decoded.end,
        periodic_value_offsets: decoded.periodic_value_offsets,
        degree_value_offsets: decoded.degree_value_offsets,
    })
}

/// Locate the surface block at `ordinal` among valid surface caches in a carrier record.
pub(crate) fn surface_patch_layout_at(record: &[u8], ordinal: usize) -> Option<SurfacePatchLayout> {
    let decoded = INT_WIDTHS.into_iter().find_map(|int_width| {
        marker_positions(record)
            .into_iter()
            .filter_map(|position| decode_surface_block(record, position, int_width))
            .nth(ordinal)
    })?;
    Some(SurfacePatchLayout {
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        u_count: decoded.surface.u_count as usize,
        v_count: decoded.surface.v_count as usize,
        u_knots: decoded.u_knot_layout.into(),
        v_knots: decoded.v_knot_layout.into(),
        end: decoded.end,
        periodic_value_offsets: decoded.periodic_value_offsets,
        degree_value_offsets: decoded.degree_value_offsets,
    })
}

/// Decode a curve `nubs`/`nurbs` block at `marker_pos`, or `None` if the bytes
/// there are not a well-formed 3D curve block.
struct DecodedCurveBlock {
    curve: NurbsCurve,
    end: usize,
    control_value_offsets: Vec<usize>,
    rational: bool,
    knot_layout: KnotLayout,
    periodic_value_offset: usize,
    degree_value_offset: usize,
}

fn decode_curve_block(b: &[u8], marker_pos: usize, int_width: usize) -> Option<DecodedCurveBlock> {
    let (cp_dims, marker_len, rational) = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker_len;

    let degree_value_offset = pos + 1;
    let degree = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let periodic_value_offset = pos + 1;
    let closure = take_tagged_int(b, &mut pos, 0x15, int_width)?;
    let n_uniq = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knots, n_poles, knot_layout) =
        read_knots(b, &mut pos, n_uniq as usize, degree, int_width)?;
    let control_start = pos;
    let (control_points, weights) = read_control_points(b, &mut pos, n_poles, cp_dims)?;
    let control_value_offsets = (0..n_poles * cp_dims)
        .map(|ordinal| control_start + ordinal * 9 + 1)
        .collect();

    Some(DecodedCurveBlock {
        curve: NurbsCurve {
            degree: degree as u32,
            knots,
            control_points,
            weights,
            periodic: is_periodic(closure),
        },
        end: pos,
        control_value_offsets,
        rational,
        knot_layout,
        periodic_value_offset,
        degree_value_offset,
    })
}

/// Writable value offsets for a 3D curve cache in one carrier record.
pub(crate) struct CurvePatchLayout {
    /// Tagged-double payload offsets in pole/component order.
    pub(crate) control_value_offsets: Vec<usize>,
    /// Whether every pole includes a fourth rational weight component.
    pub(crate) rational: bool,
    /// Number of control points.
    pub(crate) control_count: usize,
    /// Native unique-knot payloads and expanded run lengths.
    pub(crate) knots: KnotPatchLayout,
    /// Offset immediately after the final control component.
    pub(crate) end: usize,
    /// Payload offset for the closure enum.
    pub(crate) periodic_value_offset: usize,
    /// Payload offset for the degree integer.
    pub(crate) degree_value_offset: usize,
}

/// Locate the first valid 3D curve cache in a carrier record.
pub(crate) fn first_curve_patch_layout(record: &[u8]) -> Option<CurvePatchLayout> {
    let decoded = INT_WIDTHS.into_iter().find_map(|int_width| {
        marker_positions(record)
            .into_iter()
            .find_map(|position| decode_curve_block(record, position, int_width))
    })?;
    Some(CurvePatchLayout {
        control_count: decoded.curve.control_points.len(),
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        knots: decoded.knot_layout.into(),
        end: decoded.end,
        periodic_value_offset: decoded.periodic_value_offset,
        degree_value_offset: decoded.degree_value_offset,
    })
}

/// Locate the final valid 3D curve cache in a carrier record.
pub(crate) fn final_curve_patch_layout(record: &[u8]) -> Option<CurvePatchLayout> {
    let decoded = INT_WIDTHS.into_iter().find_map(|int_width| {
        marker_positions(record)
            .into_iter()
            .filter_map(|position| decode_curve_block(record, position, int_width))
            .next_back()
    })?;
    Some(CurvePatchLayout {
        control_count: decoded.curve.control_points.len(),
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        knots: decoded.knot_layout.into(),
        end: decoded.end,
        periodic_value_offset: decoded.periodic_value_offset,
        degree_value_offset: decoded.degree_value_offset,
    })
}

/// The decoded payload of a 2D `nubs` or `nurbs` pcurve block.
pub struct NurbsPcurve {
    /// Curve degree.
    pub degree: u32,
    /// Full clamped knot vector.
    pub knots: Vec<f64>,
    /// UV control points in surface-parameter space, without length scaling.
    pub control_points: Vec<Point2>,
    /// Per-pole homogeneous weights; absent for a `nubs` block.
    pub weights: Option<Vec<f64>>,
    /// Whether the parameter curve is periodic.
    pub periodic: bool,
}

/// Writable value offsets for one 2D pcurve cache.
pub(crate) struct PcurvePatchLayout {
    /// Tagged-integer payload offset for the curve degree.
    pub(crate) degree_value_offset: usize,
    /// Tagged-double payload offsets in `(u, v)` pole order.
    pub(crate) control_value_offsets: Vec<usize>,
    /// Tagged-double payload offsets for homogeneous weights.
    pub(crate) weight_value_offsets: Vec<usize>,
    /// Number of UV control points.
    pub(crate) control_count: usize,
    /// Native unique-knot payloads and expanded run lengths.
    pub(crate) knots: KnotPatchLayout,
    /// Payload offset for the closure enum.
    pub(crate) periodic_value_offset: usize,
    /// Offset immediately after the final UV control component.
    pub(crate) control_end: usize,
}

/// Locate the final valid non-rational 2D pcurve block in a carrier record.
pub(crate) fn final_pcurve_patch_layout(record: &[u8]) -> Option<PcurvePatchLayout> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| final_pcurve_patch_layout_at(record, int_width))
}

fn final_pcurve_patch_layout_at(record: &[u8], int_width: usize) -> Option<PcurvePatchLayout> {
    marker_positions(record)
        .into_iter()
        .filter_map(|marker_pos| {
            let (_cp_dims, marker_len, rational) = marker_at(record, marker_pos)?;
            let mut pos = marker_pos + marker_len;
            let degree_value_offset = pos + 1;
            let degree = take_tagged_int(record, &mut pos, 0x04, int_width)?;
            if !(1..=20).contains(&degree) {
                return None;
            }
            let periodic_value_offset = pos + 1;
            let _closure = take_tagged_int(record, &mut pos, 0x15, int_width)?;
            let unique = take_tagged_int(record, &mut pos, 0x04, int_width)?;
            if !(1..=1000).contains(&unique) {
                return None;
            }
            let (_knots, control_count, knot_layout) =
                read_knots(record, &mut pos, unique as usize, degree, int_width)?;
            let mut offsets = Vec::with_capacity(control_count * 2);
            let mut weight_offsets = Vec::with_capacity(control_count * usize::from(rational));
            for _ in 0..control_count * 2 {
                if record.get(pos) != Some(&0x06) {
                    return None;
                }
                offsets.push(pos + 1);
                pos += 9;
                if rational && offsets.len() % 2 == 0 {
                    if record.get(pos) != Some(&0x06) {
                        return None;
                    }
                    weight_offsets.push(pos + 1);
                    pos += 9;
                }
            }
            Some(PcurvePatchLayout {
                degree_value_offset,
                control_value_offsets: offsets,
                weight_value_offsets: weight_offsets,
                control_count,
                knots: knot_layout.into(),
                periodic_value_offset,
                control_end: pos,
            })
        })
        .next_back()
}

/// Decode the parameter-space fit tolerance immediately following the UV cache.
pub(crate) fn decode_pcurve_fit_tolerance(record: &[u8]) -> Option<f64> {
    let layout = final_pcurve_patch_layout(record)?;
    (record.get(layout.control_end) == Some(&0x06))
        .then(|| read_f64(record, layout.control_end + 1))
        .flatten()
}

fn decode_pcurve_block(b: &[u8], marker_pos: usize, int_width: usize) -> Option<NurbsPcurve> {
    let (_cp_dims, marker_len, rational) = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker_len;
    let degree = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = take_tagged_int(b, &mut pos, 0x15, int_width)?;
    let n_uniq = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knots, n_poles, _knot_layout) =
        read_knots(b, &mut pos, n_uniq as usize, degree, int_width)?;
    let mut control_points = Vec::with_capacity(n_poles);
    let mut weights = rational.then(|| Vec::with_capacity(n_poles));
    for _ in 0..n_poles {
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let u = read_f64(b, pos + 1)?;
        pos += 9;
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let v = read_f64(b, pos + 1)?;
        pos += 9;
        control_points.push(Point2::new(u, v));
        if let Some(weights) = weights.as_mut() {
            if *b.get(pos)? != 0x06 {
                return None;
            }
            weights.push(read_f64(b, pos + 1)?);
            pos += 9;
        }
    }
    Some(NurbsPcurve {
        degree: degree as u32,
        knots,
        control_points,
        weights,
        periodic: is_periodic(closure),
    })
}

/// Decode the face-surface cache of a spline surface record: the LAST valid
/// surface block in the record (the final `setSurfaceShape` cache; earlier
/// blocks are support surfaces or 2D pcurves). Returns `None` when no surface
/// block is present or parseable.
pub fn decode_surface_cache(record_bytes: &[u8]) -> Option<NurbsSurface> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| decode_surface_cache_at(record_bytes, int_width))
}

fn decode_surface_cache_at(record_bytes: &[u8], int_width: usize) -> Option<NurbsSurface> {
    marker_positions(record_bytes)
        .into_iter()
        .filter_map(|pos| decode_surface_block(record_bytes, pos, int_width))
        .map(|decoded| decoded.surface)
        .next_back()
}

/// A decoded native procedural definition and the fit contract of its solved cache.
pub struct DecodedProceduralSurface {
    /// The native procedural surface construction (blend, sweep, loft, or
    /// taper family) decoded from its subtype-dispatched inline fields.
    pub definition: DecodedProceduralSurfaceDefinition,
    /// `surface_fit_tolerance` of the cached B-spline block, if present.
    /// `0.0` indicates fidelity to the procedural surface rather than
    /// identity with a primitive ([spec §7.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#75-nubsnurbs-blocks-b-spline-curves-and-surfaces)).
    pub cache_fit_tolerance: Option<f64>,
}

/// Source-native procedural semantics before embedded geometry is assigned IR ids.
pub enum DecodedProceduralSurfaceDefinition {
    /// Translation of an embedded directrix along a length-bearing direction.
    Extrusion {
        /// Embedded directrix cache.
        directrix: NurbsCurve,
        /// Length-bearing sweep direction.
        direction: Vector3,
    },
    /// Rolling-ball blend with embedded support and spine caches.
    Blend {
        /// Embedded support caches in side order.
        supports: Box<[Option<NurbsSurface>; 2]>,
        /// Embedded center/spine curve.
        spine: Option<NurbsCurve>,
        /// Signed radius law.
        radius: BlendRadiusLaw,
        /// Blend cross-section family.
        cross_section: BlendCrossSection,
    },
}

/// Decode an inline `cyl_spl_sur` translational-extrusion definition.
pub fn decode_cyl_spl_sur(record_bytes: &[u8]) -> Option<DecodedProceduralSurface> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| decode_cyl_spl_sur_at(record_bytes, int_width))
}

fn decode_cyl_spl_sur_at(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let marker = b"\x0f\x0d\x0bcyl_spl_sur";
    let start = record_bytes
        .windows(marker.len())
        .position(|w| w == marker)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let directrix = decode_curve_cache_at(span, int_width)?;

    let mut doubles = Vec::new();
    let mut direction = None;
    let mut pos = marker.len();
    while pos < span.len() {
        match span[pos] {
            0x06 if direction.is_none() => doubles.push(read_f64(span, pos + 1)?),
            0x14 if direction.is_none() => {
                direction = Some(Vector3::new(
                    read_f64(span, pos + 1)? * LEN_TO_MM,
                    read_f64(span, pos + 9)? * LEN_TO_MM,
                    read_f64(span, pos + 17)? * LEN_TO_MM,
                ));
            }
            _ => {}
        }
        pos = next_token(span, pos, int_width)?;
    }
    let _u_range = [*doubles.first()?, *doubles.get(1)?];
    let decoded_cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let _v_range = [
        *decoded_cache.surface.v_knots.first()?,
        *decoded_cache.surface.v_knots.last()?,
    ];
    let cache_fit_tolerance = (span.get(decoded_cache.end) == Some(&0x06))
        .then(|| read_f64(span, decoded_cache.end + 1).map(|v| v * LEN_TO_MM))
        .flatten();

    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction: direction?,
        },
        cache_fit_tolerance,
    })
}

fn decode_rb_blend_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let marker = b"\x0f\x0d\x10rb_blend_spl_sur";
    let start = record_bytes
        .windows(marker.len())
        .position(|w| w == marker)?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;

    let mut support_count = 0usize;
    let mut radius_boundary = None;
    let mut pos = marker.len();
    while pos < cache.end {
        match span[pos] {
            0x0d | 0x0e => {
                let len = usize::from(*span.get(pos + 1)?);
                let name = span.get(pos + 2..pos + 2 + len)?;
                if [b"plane".as_slice(), b"sphere", b"cone", b"torus"].contains(&name) {
                    support_count += 1;
                }
            }
            0x15 if read_int(span, pos + 1, int_width) == Some(-1) => radius_boundary = Some(pos),
            _ => {}
        }
        pos = next_token(span, pos, int_width)?;
    }
    let boundary = radius_boundary?;
    let mut radius_values = Vec::new();
    let mut pos = marker.len();
    while pos < boundary {
        if span[pos] == 0x06 {
            radius_values.push(read_f64(span, pos + 1)?);
        }
        pos = next_token(span, pos, int_width)?;
    }
    let end = *radius_values.last()? * LEN_TO_MM;
    let start = *radius_values.get(radius_values.len().checked_sub(2)?)? * LEN_TO_MM;
    let radius = if start == end {
        BlendRadiusLaw::Constant {
            signed_radius: start,
        }
    } else {
        BlendRadiusLaw::Linear { start, end }
    };
    let center_curve = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_curve_block(span, at, int_width))
        .map(|decoded| decoded.curve)
        .next_back();
    let mut support_caches = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .filter(|decoded| decoded.end < cache.end)
        .map(|decoded| decoded.surface);
    let supports = [
        (support_count > 0).then(|| support_caches.next()).flatten(),
        (support_count > 1).then(|| support_caches.next()).flatten(),
    ];
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|v| v * LEN_TO_MM))
        .flatten();

    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new(supports),
            spine: center_curve,
            radius,
            cross_section: BlendCrossSection::Circular,
        },
        cache_fit_tolerance,
    })
}

/// Decode a native procedural definition, following nested subtype-table references.
pub fn decode_procedural_surface_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<DecodedProceduralSurface> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_procedural_resolving_refs(record_bytes, active_bytes, &mut Vec::new(), int_width)
    })
}

fn decode_procedural_resolving_refs(
    bytes: &[u8],
    active_bytes: &[u8],
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    if let Some(decoded) = decode_cyl_spl_sur_at(bytes, int_width)
        .or_else(|| decode_rb_blend_spl_sur(bytes, int_width))
    {
        return Some(decoded);
    }
    let table = subtype_table(active_bytes);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_resolving_refs(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            seen,
            int_width,
        ) {
            return Some(decoded);
        }
    }
    None
}

/// Decode a surface cache from a carrier record, following the ASM subtype
/// table when the record stores a nested `ref N` instead of an inline cache.
/// `active_bytes` is the full active SAB slice; `N` indexes its non-`ref`
/// subtype openings in byte order.
pub fn decode_surface_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsSurface> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            &mut Vec::new(),
            decode_surface_cache_at,
            int_width,
        )
    })
}

/// Decode the 3D curve cache of a procedural curve record: the FIRST valid curve
/// block (surface and 2D pcurve blocks in the record are skipped because they do
/// not parse as a 3D curve block). Returns `None` when none is present.
pub fn decode_curve_cache(record_bytes: &[u8]) -> Option<NurbsCurve> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| decode_curve_cache_at(record_bytes, int_width))
}

fn decode_curve_cache_at(record_bytes: &[u8], int_width: usize) -> Option<NurbsCurve> {
    marker_positions(record_bytes).into_iter().find_map(|pos| {
        decode_curve_block(record_bytes, pos, int_width).map(|decoded| decoded.curve)
    })
}

/// Decode a curve cache from a carrier record, resolving nested ASM subtype
/// references through the active slice's subtype table.
pub fn decode_curve_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsCurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            &mut Vec::new(),
            decode_curve_cache_at,
            int_width,
        )
    })
}

/// Source curve and tail fields decoded from an `offset_int_cur` construction.
pub(crate) type VectorOffsetDefinition = (NurbsCurve, [f64; 2], Vector3, [String; 2], [i64; 2]);

/// Parent curve and retained range decoded from a `subset_int_cur` construction.
pub(crate) type SubsetDefinition = (NurbsCurve, [f64; 2]);

/// Parameter arrays and child curves decoded from a `comp_int_cur` construction.
pub(crate) type CompoundDefinition = (Vec<f64>, Vec<f64>, Vec<NurbsCurve>);

/// A procedural curve cache together with its native subtype and fit contract.
pub struct DecodedProceduralCurve {
    /// The cached B-spline curve (control points scaled centimetre→
    /// millimetre; knots and weights unscaled).
    pub curve: NurbsCurve,
    /// The `intcurve` subtype record name (`exact_int_cur`, `off_int_cur`,
    /// `proj_int_cur`, `int_int_cur`, `helix_int_cur`, `sss_int_cur`, ...).
    pub native_kind: String,
    /// Neutral construction fields decoded from the subtype tail.
    pub definition: Option<cadmpeg_ir::geometry::ProceduralCurveDefinition>,
    /// Source curve and tail fields of an `offset_int_cur` construction.
    pub vector_offset: Option<VectorOffsetDefinition>,
    /// Parent curve and retained range of a `subset_int_cur` construction.
    pub subset: Option<SubsetDefinition>,
    /// Parameter arrays and ordered child curves of a `comp_int_cur` construction.
    pub compound: Option<CompoundDefinition>,
    /// `surface_fit_tolerance` of the cached B-spline block, if present
    /// ([spec §7.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#75-nubsnurbs-blocks-b-spline-curves-and-surfaces)).
    pub cache_fit_tolerance: Option<f64>,
}

/// Decode a procedural 3D curve cache while following subtype-table references.
pub fn decode_procedural_curve_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<DecodedProceduralCurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_procedural_curve_recursive(record_bytes, active_bytes, &mut Vec::new(), int_width)
    })
}

fn decode_procedural_curve_recursive(
    bytes: &[u8],
    active_bytes: &[u8],
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<DecodedProceduralCurve> {
    let mut solved = None;
    for position in marker_positions(bytes) {
        if let Some(decoded) = decode_curve_block(bytes, position, int_width) {
            solved = Some(decoded);
        }
    }
    if let Some(decoded) = solved {
        let cache_fit_tolerance = (bytes.get(decoded.end) == Some(&0x06))
            .then(|| read_f64(bytes, decoded.end + 1).map(|value| value * LEN_TO_MM))
            .flatten();
        let native_kind =
            first_construction_subtype(bytes).unwrap_or_else(|| "intcurve".to_string());
        let definition = if native_kind == "exact_int_cur" {
            Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact)
        } else {
            decode_helix_definition(bytes)
        };
        return Some(DecodedProceduralCurve {
            curve: decoded.curve,
            native_kind,
            definition,
            vector_offset: decode_vector_offset_definition(bytes, int_width),
            subset: decode_subset_definition(bytes, int_width),
            compound: decode_compound_definition(bytes, int_width),
            cache_fit_tolerance,
        });
    }
    let table = subtype_table(active_bytes);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_curve_recursive(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            seen,
            int_width,
        ) {
            return Some(decoded);
        }
    }
    None
}

fn decode_compound_definition(bytes: &[u8], int_width: usize) -> Option<CompoundDefinition> {
    let name = b"comp_int_cur";
    let marker = bytes.windows(name.len() + 3).position(|window| {
        window[0] == 0x0f
            && matches!(window[1], 0x0d | 0x0e)
            && usize::from(window[2]) == name.len()
            && &window[3..] == name
    })?;
    let mut position = marker + name.len() + 3;
    let parameters = take_float_array(bytes, &mut position, int_width)?;
    let count = usize::try_from(take_tagged_int(bytes, &mut position, 0x04, int_width)?).ok()?;
    if count == 0 {
        return None;
    }
    let mut component_parameters = Vec::with_capacity(count);
    for _ in 0..count {
        if bytes.get(position) != Some(&0x06) {
            return None;
        }
        component_parameters.push(read_f64(bytes, position + 1)?);
        position += 9;
    }
    if !matches!(bytes.get(position), Some(0x0a | 0x0b)) {
        return None;
    }
    position += 1;
    let mut components = Vec::with_capacity(count);
    for _ in 0..count {
        let relative = marker_positions(bytes.get(position..)?)
            .into_iter()
            .next()?;
        let decoded = decode_curve_block(bytes, position + relative, int_width)?;
        components.push(decoded.curve);
        position = decoded.end;
    }
    Some((parameters, component_parameters, components))
}

fn take_float_array(bytes: &[u8], position: &mut usize, int_width: usize) -> Option<Vec<f64>> {
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

fn decode_subset_definition(bytes: &[u8], int_width: usize) -> Option<SubsetDefinition> {
    let name = b"subset_int_cur";
    let marker = bytes.windows(name.len() + 3).position(|window| {
        window[0] == 0x0f
            && matches!(window[1], 0x0d | 0x0e)
            && usize::from(window[2]) == name.len()
            && &window[3..] == name
    })?;
    let start = marker + name.len() + 3;
    let source_marker = marker_positions(&bytes[start..]).into_iter().next()? + start;
    let source = decode_curve_block(bytes, source_marker, int_width)?;
    let mut position = source.end;
    let range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    Some((source.curve, range))
}

fn decode_vector_offset_definition(
    bytes: &[u8],
    int_width: usize,
) -> Option<VectorOffsetDefinition> {
    let name = b"offset_int_cur";
    let marker = bytes.windows(name.len() + 3).position(|window| {
        window[0] == 0x0f
            && matches!(window[1], 0x0d | 0x0e)
            && usize::from(window[2]) == name.len()
            && &window[3..] == name
    })?;
    let start = marker + name.len() + 3;
    let source_marker = marker_positions(&bytes[start..]).into_iter().next()? + start;
    let source = decode_curve_block(bytes, source_marker, int_width)?;
    let mut position = source.end;
    if bytes.get(position) != Some(&0x06) || bytes.get(position + 9) != Some(&0x06) {
        return None;
    }
    let parameter_range = [
        read_f64(bytes, position + 1)?,
        read_f64(bytes, position + 10)?,
    ];
    position += 18;
    let offset = take_native_vec3(bytes, &mut position, 0x14)?;
    let first_label = take_native_string(bytes, &mut position)?;
    let first_code = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let second_label = take_native_string(bytes, &mut position)?;
    let second_code = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    Some((
        source.curve,
        parameter_range,
        Vector3::new(
            offset[0] * LEN_TO_MM,
            offset[1] * LEN_TO_MM,
            offset[2] * LEN_TO_MM,
        ),
        [first_label, second_label],
        [first_code, second_code],
    ))
}

fn take_native_string(bytes: &[u8], position: &mut usize) -> Option<String> {
    let (length, header) = match *bytes.get(*position)? {
        0x07 => (usize::from(*bytes.get(*position + 1)?), 2),
        0x08 => (
            usize::from(u16::from_le_bytes(
                bytes.get(*position + 1..*position + 3)?.try_into().ok()?,
            )),
            3,
        ),
        0x09 => (
            usize::try_from(u32::from_le_bytes(
                bytes.get(*position + 1..*position + 5)?.try_into().ok()?,
            ))
            .ok()?,
            5,
        ),
        _ => return None,
    };
    let start = *position + header;
    let end = start.checked_add(length)?;
    let value = String::from_utf8(bytes.get(start..end)?.to_vec()).ok()?;
    *position = end;
    Some(value)
}

fn decode_helix_definition(
    bytes: &[u8],
) -> Option<cadmpeg_ir::geometry::ProceduralCurveDefinition> {
    let name = b"helix_int_cur";
    let marker = bytes.windows(name.len() + 3).position(|window| {
        window[0] == 0x0f
            && matches!(window[1], 0x0d | 0x0e)
            && usize::from(window[2]) == name.len()
            && &window[3..] == name
    })?;
    let mut position = marker + name.len() + 3;
    let lower = take_range_value(bytes, &mut position)?;
    let upper = take_range_value(bytes, &mut position)?;
    let center = take_native_vec3(bytes, &mut position, 0x13)?;
    let major = take_native_vec3(bytes, &mut position, 0x13)?;
    let minor = take_native_vec3(bytes, &mut position, 0x13)?;
    let pitch = take_native_vec3(bytes, &mut position, 0x13)?;
    if bytes.get(position) != Some(&0x06) {
        return None;
    }
    let apex_factor = read_f64(bytes, position + 1)?;
    position += 9;
    let axis = take_native_vec3(bytes, &mut position, 0x14)?;
    Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Helix {
        angle_range: [lower, upper],
        center: Point3::new(
            center[0] * LEN_TO_MM,
            center[1] * LEN_TO_MM,
            center[2] * LEN_TO_MM,
        ),
        major: Vector3::new(
            major[0] * LEN_TO_MM,
            major[1] * LEN_TO_MM,
            major[2] * LEN_TO_MM,
        ),
        minor: Vector3::new(
            minor[0] * LEN_TO_MM,
            minor[1] * LEN_TO_MM,
            minor[2] * LEN_TO_MM,
        ),
        pitch: Vector3::new(
            pitch[0] * LEN_TO_MM,
            pitch[1] * LEN_TO_MM,
            pitch[2] * LEN_TO_MM,
        ),
        apex_factor,
        axis: Vector3::new(axis[0], axis[1], axis[2]),
    })
}

fn take_range_value(bytes: &[u8], position: &mut usize) -> Option<f64> {
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

fn take_native_vec3(bytes: &[u8], position: &mut usize, tag: u8) -> Option<[f64; 3]> {
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

fn first_construction_subtype(bytes: &[u8]) -> Option<String> {
    for pos in 0..bytes.len().saturating_sub(3) {
        if bytes[pos] != 0x0f || !matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            continue;
        }
        let len = usize::from(*bytes.get(pos + 2)?);
        let name = bytes.get(pos + 3..pos + 3 + len)?;
        if name != b"ref" {
            return Some(String::from_utf8_lossy(name).into_owned());
        }
    }
    None
}

fn decode_cache_resolving_refs<T>(
    bytes: &[u8],
    active_bytes: &[u8],
    seen: &mut Vec<usize>,
    decode_inline: fn(&[u8], usize) -> Option<T>,
    int_width: usize,
) -> Option<T> {
    if let Some(decoded) = decode_inline(bytes, int_width) {
        return Some(decoded);
    }
    let table = subtype_table(active_bytes);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_cache_resolving_refs(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            seen,
            decode_inline,
            int_width,
        ) {
            return Some(decoded);
        }
    }
    None
}

fn subtype_table(bytes: &[u8]) -> Vec<usize> {
    let mut table = Vec::new();
    for pos in 0..bytes.len().saturating_sub(4) {
        if bytes[pos] != 0x0f || !matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            continue;
        }
        let len = *bytes.get(pos + 2).unwrap_or(&0) as usize;
        let Some(name) = bytes.get(pos + 3..pos + 3 + len) else {
            continue;
        };
        if name != b"ref" && name.iter().all(|b| (0x21..=0x7e).contains(b)) {
            table.push(pos);
        }
    }
    table
}

fn subtype_refs(bytes: &[u8], int_width: usize) -> Vec<usize> {
    let mut refs = Vec::new();
    let marker = b"\x0f\x0d\x03ref\x04";
    for pos in 0..=bytes.len().saturating_sub(marker.len() + int_width) {
        if bytes[pos..].starts_with(marker) {
            if let Some(index) = read_int(bytes, pos + marker.len(), int_width) {
                if index >= 0 {
                    refs.push(index as usize);
                }
            }
        }
    }
    refs
}

fn subtype_span(bytes: &[u8], start: usize, int_width: usize) -> Option<&[u8]> {
    let mut depth = 0usize;
    let mut pos = start;
    while pos < bytes.len() {
        match bytes[pos] {
            0x0f => depth += 1,
            0x10 => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return bytes.get(start..=pos);
                }
            }
            _ => {}
        }
        pos = next_token(bytes, pos, int_width)?;
    }
    None
}

fn next_token(bytes: &[u8], pos: usize, int_width: usize) -> Option<usize> {
    let tag = *bytes.get(pos)?;
    let fixed = match tag {
        0x02 => 2,
        0x03 => 3,
        0x04 | 0x0c | 0x15 => 1 + int_width,
        0x06 | 0x17 => 9,
        0x05 => 5,
        0x0a | 0x0b | 0x0f | 0x10 | 0x11 => 1,
        0x13 | 0x14 => 25,
        0x16 => 17,
        0x07 | 0x0d | 0x0e => 2 + usize::from(*bytes.get(pos + 1)?),
        0x08 => {
            3 + usize::from(u16::from_le_bytes(
                bytes.get(pos + 1..pos + 3)?.try_into().ok()?,
            ))
        }
        0x09 | 0x12 => {
            5 + usize::try_from(u32::from_le_bytes(
                bytes.get(pos + 1..pos + 5)?.try_into().ok()?,
            ))
            .ok()?
        }
        _ => return None,
    };
    let next = pos.checked_add(fixed)?;
    (next <= bytes.len()).then_some(next)
}

/// Decode the first well-formed 2D `nubs` block in a pcurve record.
pub fn decode_pcurve_cache(record_bytes: &[u8]) -> Option<NurbsPcurve> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| decode_pcurve_cache_at(record_bytes, int_width))
}

fn decode_pcurve_cache_at(record_bytes: &[u8], int_width: usize) -> Option<NurbsPcurve> {
    marker_positions(record_bytes)
        .into_iter()
        .find_map(|pos| decode_pcurve_block(record_bytes, pos, int_width))
}

/// Decode a 2D pcurve cache, resolving a nested ASM subtype-table reference
/// when the pcurve record delegates its UV carrier to an `intcurve` block.
pub fn decode_pcurve_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsPcurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            &mut Vec::new(),
            decode_pcurve_cache_at,
            int_width,
        )
    })
}

/// Decode the UV cache carried by a ref-form pcurve's `intcurve` entity. The
/// first curve-shaped `nubs` block is the 3D edge carrier; the subsequent
/// well-formed 2D block is the pcurve.
pub fn decode_intcurve_pcurve_cache(record_bytes: &[u8]) -> Option<NurbsPcurve> {
    INT_WIDTHS
        .into_iter()
        .find_map(|int_width| decode_intcurve_pcurve_cache_at(record_bytes, int_width))
}

fn decode_intcurve_pcurve_cache_at(record_bytes: &[u8], int_width: usize) -> Option<NurbsPcurve> {
    let mut saw_curve = false;
    for position in marker_positions(record_bytes) {
        if !saw_curve && decode_curve_block(record_bytes, position, int_width).is_some() {
            saw_curve = true;
            continue;
        }
        if saw_curve {
            if let Some(pcurve) = decode_pcurve_block(record_bytes, position, int_width) {
                return Some(pcurve);
            }
        }
    }
    None
}

/// Decode an intcurve-carried UV cache, following its construction subtype
/// reference when the caches live in the subtype table rather than inline.
pub fn decode_intcurve_pcurve_cache_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
) -> Option<NurbsPcurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            &mut Vec::new(),
            decode_intcurve_pcurve_cache_at,
            int_width,
        )
    })
}

#[cfg(test)]
mod width_tests {
    use super::*;

    fn push_int(out: &mut Vec<u8>, tag: u8, value: i64, int_width: usize) {
        out.push(tag);
        if int_width == 4 {
            out.extend_from_slice(
                &i32::try_from(value)
                    .expect("test value fits i32")
                    .to_le_bytes(),
            );
        } else {
            out.extend_from_slice(&value.to_le_bytes());
        }
    }

    fn push_f64(out: &mut Vec<u8>, value: f64) {
        out.push(0x06);
        out.extend_from_slice(&value.to_le_bytes());
    }

    /// A degree-1 two-pole 3D `nubs` curve block over `[0, 1]`.
    fn curve_block(int_width: usize) -> Vec<u8> {
        let mut b = NUBS_MARKER.to_vec();
        push_int(&mut b, 0x04, 1, int_width); // degree
        push_int(&mut b, 0x15, 0, int_width); // open closure
        push_int(&mut b, 0x04, 2, int_width); // unique knot count
        push_f64(&mut b, 0.0);
        push_int(&mut b, 0x04, 1, int_width);
        push_f64(&mut b, 1.0);
        push_int(&mut b, 0x04, 1, int_width);
        for component in [0.0, 0.0, 0.0, 1.0, 2.0, 3.0] {
            push_f64(&mut b, component);
        }
        b
    }

    /// A degree-1 2×2-pole `nubs` surface block over `[0, 1]²`.
    fn surface_block(int_width: usize) -> Vec<u8> {
        let mut b = NUBS_MARKER.to_vec();
        push_int(&mut b, 0x04, 1, int_width); // u degree
        push_int(&mut b, 0x04, 1, int_width); // v degree
        for _ in 0..4 {
            push_int(&mut b, 0x15, 0, int_width); // periodic/singularity enums
        }
        push_int(&mut b, 0x04, 2, int_width); // unique u knots
        push_int(&mut b, 0x04, 2, int_width); // unique v knots
        for _ in 0..2 {
            push_f64(&mut b, 0.0);
            push_int(&mut b, 0x04, 1, int_width);
            push_f64(&mut b, 1.0);
            push_int(&mut b, 0x04, 1, int_width);
        }
        for pole in 0..4 {
            push_f64(&mut b, f64::from(pole));
            push_f64(&mut b, 0.0);
            push_f64(&mut b, 0.0);
        }
        b
    }

    #[test]
    fn curve_cache_decodes_in_both_integer_widths() {
        for int_width in [4usize, 8] {
            let curve = decode_curve_cache(&curve_block(int_width))
                .unwrap_or_else(|| panic!("curve cache at width {int_width}"));
            assert_eq!(curve.degree, 1);
            assert_eq!(curve.control_points.len(), 2);
            assert_eq!(curve.control_points[1].x, 10.0); // cm→mm ×10
            assert_eq!(curve.knots, vec![0.0, 0.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn surface_cache_decodes_in_both_integer_widths() {
        for int_width in [4usize, 8] {
            let surface = decode_surface_cache(&surface_block(int_width))
                .unwrap_or_else(|| panic!("surface cache at width {int_width}"));
            assert_eq!((surface.u_degree, surface.v_degree), (1, 1));
            assert_eq!((surface.u_count, surface.v_count), (2, 2));
        }
    }

    #[test]
    fn surface_cache_resolves_width4_subtype_ref() {
        // Active slice: one named subtype span holding the surface cache.
        let mut active = vec![0x0f, 0x0d, 0x07];
        active.extend_from_slice(b"spl_sur");
        active.extend_from_slice(&surface_block(4));
        active.push(0x10);
        // Record: `ref 0` into the subtype table, 4-byte index payload.
        let mut record = vec![0x0f, 0x0d, 0x03];
        record.extend_from_slice(b"ref");
        push_int(&mut record, 0x04, 0, 4);
        record.push(0x10);
        let surface =
            decode_surface_cache_resolving_refs(&record, &active).expect("resolved width-4 ref");
        assert_eq!((surface.u_count, surface.v_count), (2, 2));
    }
}
