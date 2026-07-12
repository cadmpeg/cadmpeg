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

use cadmpeg_ir::geometry::{
    BlendCrossSection, BlendRadiusLaw, NurbsCurve, NurbsSurface, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

use crate::sab::Record;

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
    decode_pcurve_block_with_end(b, marker_pos, int_width).map(|(pcurve, _)| pcurve)
}

fn decode_pcurve_block_with_end(
    b: &[u8],
    marker_pos: usize,
    int_width: usize,
) -> Option<(NurbsPcurve, usize)> {
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
    Some((
        NurbsPcurve {
            degree: degree as u32,
            knots,
            control_points,
            weights,
            periodic: is_periodic(closure),
        },
        pos,
    ))
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
    let caches = marker_positions(record_bytes)
        .into_iter()
        .filter_map(|pos| decode_surface_block(record_bytes, pos, int_width))
        .map(|decoded| decoded.surface);
    if record_bytes
        .windows(b"comp_spl_sur".len())
        .any(|window| window == b"comp_spl_sur")
    {
        caches.into_iter().next()
    } else {
        caches.into_iter().next_back()
    }
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
    /// Exact NURBS construction and retained native U/V intervals.
    Exact {
        /// Ordered U and V intervals.
        parameter_ranges: [[f64; 2]; 2],
        /// Native ASM extension integer.
        extension: i64,
    },
    /// Native compound surface with ordered scalar/component pairs.
    Compound {
        /// Ordered native parameters.
        parameters: Vec<f64>,
        /// Ordered embedded component surfaces.
        components: Vec<SurfaceGeometry>,
    },
    /// Native taper family with shared carriers and subtype tail.
    Taper {
        /// Embedded base surface.
        support: SurfaceGeometry,
        /// Embedded reference curve.
        reference: NurbsCurve,
        /// Embedded UV curve, absent for `nullbs`.
        pcurve: Option<NurbsPcurve>,
        /// Native taper parameter.
        parameter: f64,
        /// Subtype-specific tail.
        taper: cadmpeg_ir::geometry::TaperSurfaceKind,
    },
    /// Native loft construction graph with embedded carriers.
    Loft(EmbeddedLoft),
    /// Native G2 blend construction with embedded carriers.
    G2Blend(Box<EmbeddedG2Blend>),
    /// Ruled interpolation between two ordered profile curves.
    Ruled {
        /// First embedded profile.
        first: NurbsCurve,
        /// Second embedded profile.
        second: NurbsCurve,
    },
    /// Translational sum of two curves around a stored origin.
    Sum {
        /// First embedded curve.
        first: NurbsCurve,
        /// Second embedded curve.
        second: NurbsCurve,
        /// Native model-space origin.
        basepoint: Vector3,
    },
    /// Revolution of an embedded profile around an axis.
    Revolution {
        /// Embedded profile curve.
        directrix: NurbsCurve,
        /// Point on the axis in model space.
        axis_origin: Point3,
        /// Unit axis direction.
        axis_direction: Vector3,
        /// Angular interval from the solved surface cache.
        angular_interval: [f64; 2],
        /// Native profile parameter interval.
        parameter_interval: [f64; 2],
    },
    /// Signed offset from an embedded support surface.
    Offset {
        /// Embedded support surface.
        support: SurfaceGeometry,
        /// Signed model-space distance.
        distance: f64,
        /// Native U sense enum.
        u_sense: i64,
        /// Native V sense enum.
        v_sense: i64,
        /// Ordered conditional ASM flags.
        extension_flags: Vec<bool>,
    },
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
        /// Complete native construction graph when the full layout decoded.
        native: Option<Box<EmbeddedRollingBall>>,
    },
}

pub(crate) struct EmbeddedRollingBallSide {
    pub(crate) label: String,
    pub(crate) surface: Option<SurfaceGeometry>,
    pub(crate) curve: NurbsCurve,
    pub(crate) pcurve: Option<NurbsPcurve>,
    pub(crate) location: Point3,
    pub(crate) secondary_pcurve: Option<NurbsPcurve>,
    pub(crate) exact_support: Option<NurbsSurface>,
}

pub(crate) struct EmbeddedRollingBallThirdSide {
    pub(crate) label: String,
    pub(crate) surface: SurfaceGeometry,
    pub(crate) curve: NurbsCurve,
    pub(crate) pcurve: Option<NurbsPcurve>,
    pub(crate) direction: Vector3,
    pub(crate) secondary_pcurve: Option<NurbsPcurve>,
    pub(crate) extension: i64,
    pub(crate) tertiary_pcurve: Option<NurbsPcurve>,
    pub(crate) flag: bool,
}

pub(crate) enum EmbeddedRollingBallRadiusSelector {
    None,
    Value(f64),
}

/// Embedded native rolling-ball graph before stable IR ids are assigned.
pub struct EmbeddedRollingBall {
    pub(crate) sides: Box<[EmbeddedRollingBallSide; 2]>,
    pub(crate) slice: NurbsCurve,
    pub(crate) offsets: [f64; 2],
    pub(crate) radius_selector: EmbeddedRollingBallRadiusSelector,
    pub(crate) u_range: [f64; 2],
    pub(crate) v_range: [f64; 2],
    pub(crate) parameters: [f64; 3],
    pub(crate) tail: i64,
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) third: Option<Box<EmbeddedRollingBallThirdSide>>,
}

pub(crate) struct EmbeddedG2Side {
    pub(crate) label: String,
    pub(crate) surface: SurfaceGeometry,
    pub(crate) curve: NurbsCurve,
    pub(crate) pcurves: [Option<NurbsPcurve>; 2],
    pub(crate) direction: Vector3,
}

pub(crate) enum EmbeddedG2FirstShape {
    Full {
        surface: Option<NurbsSurface>,
        tolerance: Option<f64>,
    },
    None {
        coefficients: [f64; 9],
        tolerance: f64,
        extension: Option<cadmpeg_ir::geometry::LoftBridgeToken>,
        pcurve: Option<NurbsPcurve>,
    },
}

/// Embedded native G2 blend graph before stable IR ids are assigned.
pub struct EmbeddedG2Blend {
    pub(crate) first: EmbeddedG2Side,
    pub(crate) singularity: i64,
    pub(crate) first_shape: EmbeddedG2FirstShape,
    pub(crate) second: EmbeddedG2Side,
    pub(crate) second_exact_surface: NurbsSurface,
    pub(crate) center_curve: NurbsCurve,
    pub(crate) center_parameters: [f64; 2],
    pub(crate) center_flag: i64,
    pub(crate) parameter_ranges: [[f64; 2]; 2],
    pub(crate) trailing_parameters: [f64; 4],
    pub(crate) discontinuities: [Vec<f64>; 3],
}

#[allow(clippy::option_option)] // Outer None is parse failure; inner None is native nullbs.
fn decode_nullable_embedded_pcurve(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Option<NurbsPcurve>> {
    let saved = *position;
    if take_native_ident(bytes, position).as_deref() == Some("nullbs") {
        return Some(None);
    }
    *position = saved;
    let (pcurve, end) = decode_pcurve_block_with_end(bytes, *position, int_width)?;
    *position = end;
    Some(Some(pcurve))
}

fn decode_g2_side(bytes: &[u8], position: &mut usize, int_width: usize) -> Option<EmbeddedG2Side> {
    let label = take_native_string(bytes, position)?;
    let surface = decode_embedded_surface(bytes, position, int_width)?;
    let curve = decode_curve_block(bytes, *position, int_width)?;
    *position = curve.end;
    let first = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let direction = take_native_vec3(bytes, position, 0x14)?;
    let second = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    Some(EmbeddedG2Side {
        label,
        surface,
        curve: curve.curve,
        pcurves: [first, second],
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

fn take_bridge_token(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::LoftBridgeToken> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    match *bytes.get(*position)? {
        0x0a | 0x0b => Some(LoftBridgeToken::Boolean(take_bool(bytes, position)?)),
        0x04 => Some(LoftBridgeToken::Integer(take_tagged_int(
            bytes, position, 0x04, int_width,
        )?)),
        0x06 => Some(LoftBridgeToken::Double(take_f64(bytes, position)?)),
        0x15 => Some(LoftBridgeToken::Enum(take_tagged_int(
            bytes, position, 0x15, int_width,
        )?)),
        0x07..=0x09 => Some(LoftBridgeToken::Text(take_native_string(bytes, position)?)),
        _ => None,
    }
}

fn decode_g2_blend_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"g2_blend_spl_sur", b"g2blnsur"];
    let (start, name_len) = names.into_iter().find_map(|name| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|start| (start, name.len()))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let first = decode_g2_side(span, &mut position, int_width)?;
    let singularity = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let first_shape = if matches!(span.get(position), Some(0x0d | 0x0e)) {
        let saved = position;
        if take_native_ident(span, &mut position).as_deref() == Some("nullbs") {
            EmbeddedG2FirstShape::Full {
                surface: None,
                tolerance: None,
            }
        } else {
            position = saved;
            let surface = decode_surface_block(span, position, int_width)?;
            position = surface.end;
            EmbeddedG2FirstShape::Full {
                surface: Some(surface.surface),
                tolerance: Some(take_f64(span, &mut position)? * LEN_TO_MM),
            }
        }
    } else {
        let mut coefficients = [0.0; 9];
        for coefficient in &mut coefficients {
            *coefficient = take_f64(span, &mut position)?;
        }
        let tolerance = take_f64(span, &mut position)? * LEN_TO_MM;
        let extension = (!matches!(span.get(position), Some(0x07..=0x09 | 0x0d | 0x0e)))
            .then(|| take_bridge_token(span, &mut position, int_width))
            .flatten();
        let pcurve = decode_nullable_embedded_pcurve(span, &mut position, int_width)?;
        EmbeddedG2FirstShape::None {
            coefficients,
            tolerance,
            extension,
            pcurve,
        }
    };
    let second = decode_g2_side(span, &mut position, int_width)?;
    let second_exact = decode_surface_block(span, position, int_width)?;
    position = second_exact.end;
    let center = decode_curve_block(span, position, int_width)?;
    position = center.end;
    let center_parameters = [
        take_f64(span, &mut position)?,
        take_f64(span, &mut position)?,
    ];
    let center_flag = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let parameter_ranges = [
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
    ];
    let mut trailing_parameters = [0.0; 4];
    for parameter in &mut trailing_parameters {
        *parameter = take_f64(span, &mut position)?;
    }
    let cache = decode_surface_block(span, position, int_width)?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    position = cache.end + 9;
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::G2Blend(Box::new(EmbeddedG2Blend {
            first,
            singularity,
            first_shape,
            second,
            second_exact_surface: second_exact.surface,
            center_curve: center.curve,
            center_parameters,
            center_flag,
            parameter_ranges,
            trailing_parameters,
            discontinuities,
        })),
        cache_fit_tolerance,
    })
}

pub(crate) struct EmbeddedLoftProfileData {
    pub(crate) surface: SurfaceGeometry,
    pub(crate) pcurve: Option<NurbsPcurve>,
    pub(crate) first_flag: bool,
    pub(crate) asm_extension: i64,
    pub(crate) subdata: cadmpeg_ir::geometry::LoftSubdata,
    pub(crate) direction: Option<Vector3>,
}

pub(crate) struct EmbeddedLoftProfileMember {
    pub(crate) type_code: i64,
    pub(crate) curve: NurbsCurve,
    pub(crate) data: EmbeddedLoftProfileData,
}

pub(crate) struct EmbeddedLoftPath {
    pub(crate) curve: NurbsCurve,
    pub(crate) auxiliaries: Vec<NurbsCurve>,
    pub(crate) flag: i64,
}

pub(crate) struct EmbeddedLoftSectionEntry {
    pub(crate) parameter: f64,
    pub(crate) profile: Vec<EmbeddedLoftProfileMember>,
    pub(crate) path: EmbeddedLoftPath,
}

/// Embedded native loft graph before its carriers receive stable IR ids.
pub struct EmbeddedLoft {
    pub(crate) sections: [Vec<EmbeddedLoftSectionEntry>; 2],
    pub(crate) parameter_ranges: [[f64; 2]; 2],
    pub(crate) closures: [i64; 2],
    pub(crate) singularities: [i64; 2],
    pub(crate) mode: i64,
    pub(crate) bridge: Vec<cadmpeg_ir::geometry::LoftBridgeToken>,
}

fn decode_loft_subdata(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::LoftSubdata> {
    use cadmpeg_ir::geometry::{LoftSubdata, LoftSubdataRow};
    let type_code = take_tagged_int(bytes, position, 0x04, int_width)?;
    let row_count = take_tagged_int(bytes, position, 0x04, int_width)?;
    let column_count = take_tagged_int(bytes, position, 0x04, int_width)?;
    let rows_to_read = if type_code == 211 {
        1
    } else {
        usize::try_from(row_count).ok()?
    };
    let columns_to_read = usize::try_from(column_count).ok()?;
    let mut rows = Vec::with_capacity(rows_to_read);
    for _ in 0..rows_to_read {
        let parameters = [take_f64(bytes, position)?, take_f64(bytes, position)?];
        let mut columns = Vec::new();
        if type_code != 211 {
            columns.reserve(columns_to_read);
            for _ in 0..columns_to_read {
                columns.push([take_f64(bytes, position)?, take_f64(bytes, position)?]);
            }
        }
        rows.push(LoftSubdataRow {
            parameters,
            columns,
        });
    }
    Some(LoftSubdata {
        type_code,
        row_count,
        column_count,
        rows,
    })
}

fn decode_loft_profile_data(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<EmbeddedLoftProfileData> {
    let surface = decode_embedded_surface(bytes, position, int_width)?;
    let saved = *position;
    let pcurve = if take_native_ident(bytes, position).as_deref() == Some("nullbs") {
        None
    } else {
        *position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(bytes, *position, int_width)?;
        *position = end;
        Some(pcurve)
    };
    let first_flag = take_bool(bytes, position)?;
    let asm_extension = take_tagged_int(bytes, position, 0x04, int_width)?;
    let subdata = decode_loft_subdata(bytes, position, int_width)?;
    let direction = if take_bool(bytes, position)? {
        let value = take_native_vec3(bytes, position, 0x14)?;
        Some(Vector3::new(value[0], value[1], value[2]))
    } else {
        None
    };
    Some(EmbeddedLoftProfileData {
        surface,
        pcurve,
        first_flag,
        asm_extension,
        subdata,
        direction,
    })
}

fn decode_loft_section(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<Vec<EmbeddedLoftSectionEntry>> {
    let count = usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let parameter = take_f64(bytes, position)?;
        let member_count =
            usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
        let mut profile = Vec::with_capacity(member_count);
        for _ in 0..member_count {
            let type_code = take_tagged_int(bytes, position, 0x04, int_width)?;
            let curve = decode_curve_block(bytes, *position, int_width)?;
            *position = curve.end;
            let data = decode_loft_profile_data(bytes, position, int_width)?;
            profile.push(EmbeddedLoftProfileMember {
                type_code,
                curve: curve.curve,
                data,
            });
        }
        let curve = decode_curve_block(bytes, *position, int_width)?;
        *position = curve.end;
        let auxiliary_count =
            usize::try_from(take_tagged_int(bytes, position, 0x04, int_width)?).ok()?;
        let mut auxiliaries = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let auxiliary = decode_curve_block(bytes, *position, int_width)?;
            *position = auxiliary.end;
            auxiliaries.push(auxiliary.curve);
        }
        let flag = take_tagged_int(bytes, position, 0x04, int_width)?;
        entries.push(EmbeddedLoftSectionEntry {
            parameter,
            profile,
            path: EmbeddedLoftPath {
                curve: curve.curve,
                auxiliaries,
                flag,
            },
        });
    }
    Some(entries)
}

fn decode_loft_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::LoftBridgeToken;
    let names: [&[u8]; 2] = [b"loft_spl_sur", b"loftsur"];
    let (start, name_len) = names.into_iter().find_map(|name| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|start| (start, name.len()))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let sections = [
        decode_loft_section(span, &mut position, int_width)?,
        decode_loft_section(span, &mut position, int_width)?,
    ];
    let parameter_ranges = [
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
        [
            take_f64(span, &mut position)?,
            take_f64(span, &mut position)?,
        ],
    ];
    let closures = [
        take_tagged_int(span, &mut position, 0x15, int_width)?,
        take_tagged_int(span, &mut position, 0x15, int_width)?,
    ];
    let singularities = [
        take_tagged_int(span, &mut position, 0x15, int_width)?,
        take_tagged_int(span, &mut position, 0x15, int_width)?,
    ];
    let mode = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let (cache_at, cache) = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width).map(|cache| (at, cache)))
        .next_back()?;
    let mut bridge = Vec::new();
    while position < cache_at {
        match *span.get(position)? {
            0x0a | 0x0b => bridge.push(LoftBridgeToken::Boolean(take_bool(span, &mut position)?)),
            0x04 => bridge.push(LoftBridgeToken::Integer(take_tagged_int(
                span,
                &mut position,
                0x04,
                int_width,
            )?)),
            0x06 => bridge.push(LoftBridgeToken::Double(take_f64(span, &mut position)?)),
            0x15 => bridge.push(LoftBridgeToken::Enum(take_tagged_int(
                span,
                &mut position,
                0x15,
                int_width,
            )?)),
            0x07..=0x09 => {
                bridge.push(LoftBridgeToken::Text(take_native_string(
                    span,
                    &mut position,
                )?));
            }
            _ => return None,
        }
    }
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Loft(EmbeddedLoft {
            sections,
            parameter_ranges,
            closures,
            singularities,
            mode,
            bridge,
        }),
        cache_fit_tolerance,
    })
}

fn decode_taper_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    use cadmpeg_ir::geometry::TaperSurfaceKind;
    let names: &[(&[u8], u8)] = &[
        (b"taper_spl_sur", 0),
        (b"ortho_spl_sur", 1),
        (b"orthosur", 1),
        (b"edge_tpr_spl_sur", 2),
        (b"shadow_tpr_spl_sur", 3),
        (b"shadowtapersur", 3),
        (b"ruled_tpr_spl_sur", 4),
        (b"ruledtapersur", 4),
        (b"swept_tpr_spl_sur", 5),
        (b"swepttapersur", 5),
    ];
    let (start, name_len, kind) = names.iter().find_map(|(name, kind)| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == *name
            })
            .map(|start| (start, name.len(), *kind))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let support = decode_embedded_surface(span, &mut position, int_width)?;
    let reference = decode_curve_block(span, position, int_width)?;
    position = reference.end;
    let saved = position;
    let pcurve = if take_native_ident(span, &mut position).as_deref() == Some("nullbs") {
        None
    } else {
        position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(span, position, int_width)?;
        position = end;
        Some(pcurve)
    };
    let parameter = take_f64(span, &mut position)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    position = cache.end + 9;
    let take_draft = |position: &mut usize| {
        let draft = take_native_vec3(span, position, 0x14)?;
        Some(Vector3::new(draft[0], draft[1], draft[2]))
    };
    let taper = match kind {
        0 => TaperSurfaceKind::Standard,
        1 => TaperSurfaceKind::Orthogonal {
            sense: take_bool(span, &mut position)?,
        },
        2 => TaperSurfaceKind::Edge {
            draft: take_draft(&mut position)?,
        },
        3 => TaperSurfaceKind::Shadow {
            draft: take_draft(&mut position)?,
            sine: take_f64(span, &mut position)?,
            cosine: take_f64(span, &mut position)?,
        },
        4 => TaperSurfaceKind::Ruled {
            draft: take_draft(&mut position)?,
            sine: take_f64(span, &mut position)?,
            cosine: take_f64(span, &mut position)?,
            factor: take_f64(span, &mut position)?,
        },
        5 => TaperSurfaceKind::Swept {
            draft: take_draft(&mut position)?,
            sine: take_f64(span, &mut position)?,
            cosine: take_f64(span, &mut position)?,
        },
        _ => return None,
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Taper {
            support,
            reference: reference.curve,
            pcurve,
            parameter,
            taper,
        },
        cache_fit_tolerance,
    })
}

fn decode_comp_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let name = b"comp_spl_sur";
    let start = record_bytes.windows(name.len() + 3).position(|window| {
        window[0] == 0x0f
            && matches!(window[1], 0x0d | 0x0e)
            && usize::from(window[2]) == name.len()
            && &window[3..] == name
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let cache = marker_positions(span)
        .into_iter()
        .find_map(|at| decode_surface_block(span, at, int_width))?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    let mut position = cache.end + 9;
    let parameters = take_float_array(span, &mut position, int_width)?;
    let mut components = Vec::with_capacity(parameters.len());
    for _ in 0..parameters.len() {
        components.push(decode_embedded_surface(span, &mut position, int_width)?);
    }
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Compound {
            parameters,
            components,
        },
        cache_fit_tolerance,
    })
}

fn decode_off_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"off_spl_sur", b"offsur"];
    let (start, name_len, modern) = names.into_iter().find_map(|name| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|start| (start, name.len(), name == b"off_spl_sur"))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let support = decode_embedded_surface(span, &mut position, int_width)?;
    let distance = take_f64(span, &mut position)? * LEN_TO_MM;
    let u_sense = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let v_sense = take_tagged_int(span, &mut position, 0x15, int_width)?;
    let mut extension_flags = Vec::new();
    if modern {
        let first = take_bool(span, &mut position)?;
        extension_flags.push(first);
        if first {
            extension_flags.push(take_bool(span, &mut position)?);
            if matches!(span.get(position), Some(0x0a | 0x0b)) {
                extension_flags.push(take_bool(span, &mut position)?);
            }
        }
    }
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Offset {
            support,
            distance,
            u_sense,
            v_sense,
            extension_flags,
        },
        cache_fit_tolerance,
    })
}

fn decode_rot_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"rot_spl_sur", b"rotsur"];
    let start = names.into_iter().find_map(|name| {
        record_bytes.windows(name.len() + 3).position(|window| {
            window[0] == 0x0f
                && matches!(window[1], 0x0d | 0x0e)
                && usize::from(window[2]) == name.len()
                && &window[3..] == name
        })
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let directrix = marker_positions(span)
        .into_iter()
        .find_map(|at| decode_curve_block(span, at, int_width))?;
    let parameter_interval = [
        *directrix.curve.knots.first()?,
        *directrix.curve.knots.last()?,
    ];
    let mut position = directrix.end;
    let origin = take_native_vec3(span, &mut position, 0x13)?;
    let axis_origin = Point3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let axis = take_native_vec3(span, &mut position, 0x14)?;
    let axis_direction = normalized(axis)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let angular_interval = [
        *cache.surface.v_knots.first()?,
        *cache.surface.v_knots.last()?,
    ];
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Revolution {
            directrix: directrix.curve,
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
        },
        cache_fit_tolerance,
    })
}

fn decode_sum_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"sum_spl_sur", b"sumsur"];
    let start = names.into_iter().find_map(|name| {
        record_bytes.windows(name.len() + 3).position(|window| {
            window[0] == 0x0f
                && matches!(window[1], 0x0d | 0x0e)
                && usize::from(window[2]) == name.len()
                && &window[3..] == name
        })
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut decoded_curves = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_curve_block(span, at, int_width));
    let first = decoded_curves.next()?;
    let second = decoded_curves.next()?;
    let mut position = second.end;
    let origin = take_native_vec3(span, &mut position, 0x13)?;
    let basepoint = Vector3::new(
        origin[0] * LEN_TO_MM,
        origin[1] * LEN_TO_MM,
        origin[2] * LEN_TO_MM,
    );
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Sum {
            first: first.curve,
            second: second.curve,
            basepoint,
        },
        cache_fit_tolerance,
    })
}

fn decode_ruled_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"rule_sur", b"rulesur"];
    let start = names.into_iter().find_map(|name| {
        record_bytes.windows(name.len() + 3).position(|window| {
            window[0] == 0x0f
                && matches!(window[1], 0x0d | 0x0e)
                && usize::from(window[2]) == name.len()
                && &window[3..] == name
        })
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut curves = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_curve_block(span, at, int_width).map(|decoded| decoded.curve));
    let first = curves.next()?;
    let second = curves.next()?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Ruled { first, second },
        cache_fit_tolerance,
    })
}

fn decode_exact_spl_sur(record_bytes: &[u8], int_width: usize) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 2] = [b"exact_spl_sur", b"exactsur"];
    let (start, name) = names.into_iter().find_map(|name| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|start| (start, name))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;
    let cache_fit_tolerance = (span.get(cache.end) == Some(&0x06))
        .then(|| read_f64(span, cache.end + 1).map(|value| value * LEN_TO_MM))
        .flatten();
    let mut position = cache.end + 9;
    let parameter_ranges = [
        [
            take_range_value(span, &mut position)?,
            take_range_value(span, &mut position)?,
        ],
        [
            take_range_value(span, &mut position)?,
            take_range_value(span, &mut position)?,
        ],
    ];
    let extension = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let _ = name;
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Exact {
            parameter_ranges,
            extension,
        },
        cache_fit_tolerance,
    })
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

fn decode_rolling_ball_side(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<EmbeddedRollingBallSide> {
    let label = take_native_string(bytes, position)?;
    let saved = *position;
    let surface = if take_native_ident(bytes, position).as_deref() == Some("null_surface") {
        None
    } else {
        *position = saved;
        Some(decode_embedded_surface(bytes, position, int_width)?)
    };
    let curve = decode_curve_block(bytes, *position, int_width)?;
    *position = curve.end;
    let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let location = take_native_vec3(bytes, position, 0x13)?;
    let secondary_pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let saved = *position;
    let exact_support = if matches!(
        take_native_ident(bytes, position).as_deref(),
        Some("nullbs" | "null_surface")
    ) {
        None
    } else {
        *position = saved;
        match decode_embedded_surface(bytes, position, int_width)? {
            SurfaceGeometry::Nurbs(surface) => Some(surface),
            _ => return None,
        }
    };
    Some(EmbeddedRollingBallSide {
        label,
        surface,
        curve: curve.curve,
        pcurve,
        location: Point3::new(
            location[0] * LEN_TO_MM,
            location[1] * LEN_TO_MM,
            location[2] * LEN_TO_MM,
        ),
        secondary_pcurve,
        exact_support,
    })
}

fn decode_rolling_ball_third_side(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<EmbeddedRollingBallThirdSide> {
    let label = take_native_string(bytes, position)?;
    let surface = decode_embedded_surface(bytes, position, int_width)?;
    let curve = decode_curve_block(bytes, *position, int_width)?;
    *position = curve.end;
    let pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let direction = take_native_vec3(bytes, position, 0x14)?;
    let secondary_pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let extension = take_tagged_int(bytes, position, 0x04, int_width)?;
    let tertiary_pcurve = decode_nullable_embedded_pcurve(bytes, position, int_width)?;
    let flag = take_bool(bytes, position)?;
    Some(EmbeddedRollingBallThirdSide {
        label,
        surface,
        curve: curve.curve,
        pcurve,
        direction: Vector3::new(direction[0], direction[1], direction[2]),
        secondary_pcurve,
        extension,
        tertiary_pcurve,
        flag,
    })
}

fn decode_full_rb_blend_spl_sur(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [(&[u8], bool); 4] = [
        (b"rb_blend_spl_sur", false),
        (b"rbblnsur", false),
        (b"sss_blend_spl_sur", true),
        (b"sssblndsur", true),
    ];
    let (start, name_len, has_third) = names.into_iter().find_map(|(name, has_third)| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|start| (start, name.len(), has_third))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let mut position = name_len + 3;
    let sides = Box::new([
        decode_rolling_ball_side(span, &mut position, int_width)?,
        decode_rolling_ball_side(span, &mut position, int_width)?,
    ]);
    let slice = decode_curve_block(span, position, int_width)?;
    position = slice.end;
    let offsets = [
        take_f64(span, &mut position)? * LEN_TO_MM,
        take_f64(span, &mut position)? * LEN_TO_MM,
    ];
    let radius_selector = match span.get(position)? {
        0x15 => {
            if take_tagged_int(span, &mut position, 0x15, int_width)? != -1 {
                return None;
            }
            EmbeddedRollingBallRadiusSelector::None
        }
        0x06 => EmbeddedRollingBallRadiusSelector::Value(take_f64(span, &mut position)?),
        _ => return None,
    };
    let u_range = [
        take_f64(span, &mut position)?,
        take_f64(span, &mut position)?,
    ];
    let v_range = [
        take_f64(span, &mut position)?,
        take_f64(span, &mut position)?,
    ];
    let parameters = [
        take_f64(span, &mut position)?,
        take_f64(span, &mut position)?,
        take_f64(span, &mut position)?,
    ];
    let tail = take_tagged_int(span, &mut position, 0x04, int_width)?;
    let cache = decode_surface_block(span, position, int_width)?;
    position = cache.end;
    let cache_fit_tolerance = Some(take_f64(span, &mut position)? * LEN_TO_MM);
    let discontinuities = [
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
        take_float_array(span, &mut position, int_width)?,
    ];
    let third = if has_third {
        Some(Box::new(decode_rolling_ball_third_side(
            span,
            &mut position,
            int_width,
        )?))
    } else {
        None
    };
    let radius = if offsets[0] == offsets[1] {
        BlendRadiusLaw::Constant {
            signed_radius: offsets[0],
        }
    } else {
        BlendRadiusLaw::Linear {
            start: offsets[0],
            end: offsets[1],
        }
    };
    Some(DecodedProceduralSurface {
        definition: DecodedProceduralSurfaceDefinition::Blend {
            supports: Box::new([None, None]),
            spine: Some(slice.curve.clone()),
            radius,
            cross_section: BlendCrossSection::Circular,
            native: Some(Box::new(EmbeddedRollingBall {
                sides,
                slice: slice.curve,
                offsets,
                radius_selector,
                u_range,
                v_range,
                parameters,
                tail,
                discontinuities,
                third,
            })),
        },
        cache_fit_tolerance,
    })
}

fn decode_rb_blend_spl_sur_fallback(
    record_bytes: &[u8],
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    let names: [&[u8]; 4] = [
        b"rb_blend_spl_sur",
        b"rbblnsur",
        b"pipe_spl_sur",
        b"pipesur",
    ];
    let (start, header_len) = names.into_iter().find_map(|name| {
        record_bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|start| (start, name.len() + 3))
    })?;
    let span = subtype_span(record_bytes, start, int_width)?;
    let cache = marker_positions(span)
        .into_iter()
        .filter_map(|at| decode_surface_block(span, at, int_width))
        .next_back()?;

    let mut support_count = 0usize;
    let mut radius_boundary = None;
    let mut pos = header_len;
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
    let mut pos = header_len;
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
            native: None,
        },
        cache_fit_tolerance,
    })
}

/// Decode a native procedural definition, following nested subtype-table references.
pub fn decode_procedural_surface_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<DecodedProceduralSurface> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_procedural_resolving_refs(
            record_bytes,
            active_bytes,
            tables,
            &mut Vec::new(),
            int_width,
        )
    })
}

fn decode_procedural_resolving_refs(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<DecodedProceduralSurface> {
    if let Some(decoded) = decode_exact_spl_sur(bytes, int_width)
        .or_else(|| decode_comp_spl_sur(bytes, int_width))
        .or_else(|| decode_taper_spl_sur(bytes, int_width))
        .or_else(|| decode_loft_spl_sur(bytes, int_width))
        .or_else(|| decode_g2_blend_spl_sur(bytes, int_width))
        .or_else(|| decode_ruled_spl_sur(bytes, int_width))
        .or_else(|| decode_sum_spl_sur(bytes, int_width))
        .or_else(|| decode_rot_spl_sur(bytes, int_width))
        .or_else(|| decode_off_spl_sur(bytes, int_width))
        .or_else(|| decode_cyl_spl_sur_at(bytes, int_width))
        .or_else(|| decode_full_rb_blend_spl_sur(bytes, int_width))
        .or_else(|| decode_rb_blend_spl_sur_fallback(bytes, int_width))
    {
        return Some(decoded);
    }
    let table = tables.for_width(int_width);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_resolving_refs(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            tables,
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
    tables: &SubtypeTables,
) -> Option<NurbsSurface> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            tables,
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
    tables: &SubtypeTables,
) -> Option<NurbsCurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            tables,
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

/// Embedded freeform support carriers and tail fields of an `off_int_cur`.
pub(crate) struct EmbeddedTwoSidedOffset {
    /// Two ordered embedded support surfaces.
    pub(crate) surfaces: [SurfaceGeometry; 2],
    /// Two ordered embedded NURBS parameter curves.
    pub(crate) pcurves: [NurbsPcurve; 2],
    /// Shared native parameter interval.
    pub(crate) parameter_range: [f64; 2],
    /// Three discontinuity arrays.
    pub(crate) discontinuities: [Vec<f64>; 3],
    /// Signed side offsets in document length units.
    pub(crate) offsets: [f64; 2],
}

/// Embedded support carriers and shared fields of an `int_int_cur`.
pub(crate) struct EmbeddedIntersection {
    pub(crate) surfaces: [SurfaceGeometry; 2],
    pub(crate) pcurves: [NurbsPcurve; 2],
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
}

/// Three ordered support carriers and selector of an `sss_int_cur`.
pub(crate) struct EmbeddedThreeSurfaceIntersection {
    pub(crate) surfaces: [SurfaceGeometry; 3],
    pub(crate) pcurves: [NurbsPcurve; 3],
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) selector: i64,
}

/// Embedded support context, source curve, and tail of a `proj_int_cur`.
pub(crate) struct EmbeddedProjection {
    pub(crate) surfaces: [SurfaceGeometry; 2],
    pub(crate) pcurves: [NurbsPcurve; 2],
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) source: NurbsCurve,
    pub(crate) tail: cadmpeg_ir::geometry::ProjectionTail,
}

/// Shared context and tail fields of a silhouette intcurve.
pub(crate) struct EmbeddedSilhouette {
    pub(crate) context: EmbeddedIntersection,
    pub(crate) silhouette: cadmpeg_ir::geometry::SilhouetteKind,
    pub(crate) cast_surface: SurfaceGeometry,
    pub(crate) light_direction: Vector3,
}

/// Shared context and tail fields of an `off_surf_int_cur`.
pub(crate) struct EmbeddedSurfaceOffset {
    pub(crate) context: EmbeddedIntersection,
    pub(crate) base_u_range: [f64; 2],
    pub(crate) base_v_range: [f64; 2],
    pub(crate) base: NurbsCurve,
    pub(crate) base_range: [f64; 2],
    pub(crate) distance: f64,
    pub(crate) shift: f64,
    pub(crate) scale: f64,
}

/// Spring support context, conditional null-carrier ranges, and direction enum.
pub(crate) struct EmbeddedSpring {
    pub(crate) surfaces: [Option<SurfaceGeometry>; 2],
    pub(crate) pcurves: [Option<NurbsPcurve>; 2],
    pub(crate) surface_parameter_ranges: [Option<[[f64; 2]; 2]>; 2],
    pub(crate) first_pcurve_parameter_range: Option<[f64; 2]>,
    pub(crate) parameter_range: [f64; 2],
    pub(crate) discontinuities: [Vec<f64>; 3],
    pub(crate) direction: i64,
}

pub(crate) enum EmbeddedDeformableData {
    VectorField {
        vectors: [Vector3; 4],
        parameter_pairs: Vec<[f64; 2]>,
    },
    Surface(SurfaceGeometry),
}

pub(crate) struct EmbeddedDeformable {
    pub(crate) extension: i64,
    pub(crate) bend: NurbsCurve,
    pub(crate) data: EmbeddedDeformableData,
}

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
    /// Non-null embedded NURBS support carriers of an `off_int_cur`.
    pub(crate) embedded_two_sided_offset: Option<EmbeddedTwoSidedOffset>,
    /// Embedded support context of an `int_int_cur`.
    pub(crate) embedded_intersection: Option<EmbeddedIntersection>,
    /// Three embedded support pairs of an `sss_int_cur`.
    pub(crate) embedded_three_surface_intersection: Option<EmbeddedThreeSurfaceIntersection>,
    /// Prefix-only surface-curve family and support context.
    pub(crate) embedded_surface_curve: Option<(
        cadmpeg_ir::geometry::SurfaceCurveFamily,
        EmbeddedIntersection,
    )>,
    /// Embedded silhouette support, cast surface, and light vector.
    pub(crate) embedded_silhouette: Option<EmbeddedSilhouette>,
    /// Embedded support context and base curve of an `off_surf_int_cur`.
    pub(crate) embedded_surface_offset: Option<EmbeddedSurfaceOffset>,
    /// Modern non-null `spring_int_cur` construction.
    pub(crate) embedded_spring: Option<EmbeddedSpring>,
    /// Embedded bend curve and discriminator payload of a `defm_int_cur`.
    pub(crate) embedded_deformable: Option<EmbeddedDeformable>,
    /// Embedded support context and source of a `proj_int_cur`.
    pub(crate) embedded_projection: Option<EmbeddedProjection>,
    /// `surface_fit_tolerance` of the cached B-spline block, if present
    /// ([spec §7.5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#75-nubsnurbs-blocks-b-spline-curves-and-surfaces)).
    pub cache_fit_tolerance: Option<f64>,
}

/// Decode a procedural 3D curve cache while following subtype-table references.
pub fn decode_procedural_curve_resolving_refs(
    record_bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
) -> Option<DecodedProceduralCurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_procedural_curve_recursive(
            record_bytes,
            active_bytes,
            tables,
            &mut Vec::new(),
            int_width,
        )
    })
}

fn decode_procedural_curve_recursive(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    int_width: usize,
) -> Option<DecodedProceduralCurve> {
    let vector_offset = decode_vector_offset_definition(bytes, int_width);
    let subset = decode_subset_definition(bytes, int_width);
    let compound = decode_compound_definition(bytes, int_width);
    // Wrapper constructions serialize their source curves before the record's
    // own cache, so the cache is the last decodable curve block. Every other
    // intcurve opens with its cache — the first block, followed by the fit
    // tolerance; later blocks belong to nested construction machinery
    // (support surfaces, blend spines, progenitors) and are not the carrier.
    let positions = marker_positions(bytes);
    let solved = if vector_offset.is_some() || subset.is_some() || compound.is_some() {
        positions
            .into_iter()
            .rev()
            .find_map(|position| decode_curve_block(bytes, position, int_width))
    } else {
        positions
            .into_iter()
            .find_map(|position| decode_curve_block(bytes, position, int_width))
    };
    if let Some(decoded) = solved {
        let cache_fit_tolerance = (bytes.get(decoded.end) == Some(&0x06))
            .then(|| read_f64(bytes, decoded.end + 1).map(|value| value * LEN_TO_MM))
            .flatten();
        let native_kind =
            first_construction_subtype(bytes).unwrap_or_else(|| "intcurve".to_string());
        let definition = if native_kind == "exact_int_cur" {
            Some(cadmpeg_ir::geometry::ProceduralCurveDefinition::Exact)
        } else {
            decode_helix_definition(bytes).or_else(|| decode_two_sided_offset(bytes, int_width))
        };
        return Some(DecodedProceduralCurve {
            curve: decoded.curve,
            native_kind,
            definition,
            vector_offset,
            subset,
            compound,
            embedded_two_sided_offset: decode_embedded_two_sided_offset(bytes, int_width),
            embedded_intersection: decode_embedded_intersection(bytes, int_width),
            embedded_three_surface_intersection: decode_embedded_three_surface_intersection(
                bytes, int_width,
            ),
            embedded_surface_curve: decode_embedded_surface_curve(bytes, int_width),
            embedded_silhouette: decode_embedded_silhouette(bytes, int_width),
            embedded_surface_offset: decode_embedded_surface_offset(bytes, int_width),
            embedded_spring: decode_embedded_spring(bytes, int_width),
            embedded_deformable: decode_embedded_deformable(bytes, int_width),
            embedded_projection: decode_embedded_projection(bytes, int_width),
            cache_fit_tolerance,
        });
    }
    let table = tables.for_width(int_width);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_procedural_curve_recursive(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            tables,
            seen,
            int_width,
        ) {
            return Some(decoded);
        }
    }
    None
}

fn decode_embedded_deformable(bytes: &[u8], int_width: usize) -> Option<EmbeddedDeformable> {
    let name = b"defm_int_cur";
    let marker = bytes.windows(name.len() + 3).position(|window| {
        window[0] == 0x0f
            && matches!(window[1], 0x0d | 0x0e)
            && usize::from(window[2]) == name.len()
            && &window[3..] == name
    })?;
    let mut position = marker + name.len() + 3;
    let extension = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let bend = decode_curve_block(bytes, position, int_width)?;
    position = bend.end;
    let mode = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let data = match mode {
        8 => {
            let mut vectors = [Vector3::new(0.0, 0.0, 0.0); 4];
            for vector in &mut vectors {
                let value = take_native_vec3(bytes, &mut position, 0x14)?;
                *vector = Vector3::new(value[0], value[1], value[2]);
            }
            let count = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
            let count = usize::try_from(count).ok()?;
            let mut parameter_pairs = Vec::with_capacity(count);
            for _ in 0..count {
                parameter_pairs.push([
                    take_f64(bytes, &mut position)?,
                    take_f64(bytes, &mut position)?,
                ]);
            }
            EmbeddedDeformableData::VectorField {
                vectors,
                parameter_pairs,
            }
        }
        5 => EmbeddedDeformableData::Surface(decode_embedded_surface(
            bytes,
            &mut position,
            int_width,
        )?),
        _ => return None,
    };
    Some(EmbeddedDeformable {
        extension,
        bend: bend.curve,
        data,
    })
}

fn decode_embedded_spring(bytes: &[u8], int_width: usize) -> Option<EmbeddedSpring> {
    let name = b"spring_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    let mut surfaces = [None, None];
    let mut surface_parameter_ranges = [None, None];
    for side in 0..2 {
        let saved = position;
        if take_native_ident(bytes, &mut position).as_deref() == Some("null_surface") {
            surface_parameter_ranges[side] = Some([
                [
                    take_range_value(bytes, &mut position)?,
                    take_range_value(bytes, &mut position)?,
                ],
                [
                    take_range_value(bytes, &mut position)?,
                    take_range_value(bytes, &mut position)?,
                ],
            ]);
        } else {
            position = saved;
            surfaces[side] = Some(decode_embedded_surface(bytes, &mut position, int_width)?);
        }
    }
    let first_pcurve;
    let first_pcurve_parameter_range;
    let saved = position;
    if take_native_ident(bytes, &mut position).as_deref() == Some("nullbs") {
        first_pcurve = None;
        first_pcurve_parameter_range = Some([
            take_range_value(bytes, &mut position)?,
            take_range_value(bytes, &mut position)?,
        ]);
    } else {
        position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
        position = end;
        first_pcurve = Some(pcurve);
        first_pcurve_parameter_range = None;
    }
    let saved = position;
    let second_pcurve = if take_native_ident(bytes, &mut position).as_deref() == Some("nullbs") {
        None
    } else {
        position = saved;
        let (pcurve, end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
        position = end;
        Some(pcurve)
    };
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    take_bool(bytes, &mut position)?;
    let direction = take_tagged_int(bytes, &mut position, 0x15, int_width)?;
    Some(EmbeddedSpring {
        surfaces,
        pcurves: [first_pcurve, second_pcurve],
        surface_parameter_ranges,
        first_pcurve_parameter_range,
        parameter_range,
        discontinuities,
        direction,
    })
}

fn decode_embedded_surface_offset(bytes: &[u8], int_width: usize) -> Option<EmbeddedSurfaceOffset> {
    let name = b"off_surf_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    take_bool(bytes, &mut position)?;
    let base_u_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let base_v_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let base = decode_curve_block(bytes, position, int_width)?;
    position = base.end;
    let base_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    Some(EmbeddedSurfaceOffset {
        context: EmbeddedIntersection {
            surfaces,
            pcurves: [first_pcurve, second_pcurve],
            parameter_range,
            discontinuities,
        },
        base_u_range,
        base_v_range,
        base: base.curve,
        base_range,
        distance: take_f64(bytes, &mut position)? * LEN_TO_MM,
        shift: take_f64(bytes, &mut position)?,
        scale: take_f64(bytes, &mut position)?,
    })
}

fn decode_embedded_silhouette(bytes: &[u8], int_width: usize) -> Option<EmbeddedSilhouette> {
    use cadmpeg_ir::geometry::SilhouetteKind;
    let names = [
        (b"silh_int_cur".as_slice(), SilhouetteKind::Standard),
        (b"para_silh_int_cur".as_slice(), SilhouetteKind::Parametric),
        (b"parasil".as_slice(), SilhouetteKind::Parametric),
        (
            b"taper_silh_int_cur".as_slice(),
            SilhouetteKind::Taper { draft_factor: 0.0 },
        ),
    ];
    let (marker, name, mut silhouette) = names.into_iter().find_map(|(name, silhouette)| {
        bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|marker| (marker, name, silhouette))
    })?;
    let mut position = marker + name.len() + 3;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let cast_surface = decode_embedded_surface(bytes, &mut position, int_width)?;
    let light = take_native_vec3(bytes, &mut position, 0x14)?;
    let light_direction = normalized(light)?;
    if matches!(silhouette, SilhouetteKind::Taper { .. }) {
        silhouette = SilhouetteKind::Taper {
            draft_factor: take_f64(bytes, &mut position)?,
        };
    }
    Some(EmbeddedSilhouette {
        context: EmbeddedIntersection {
            surfaces,
            pcurves: [first_pcurve, second_pcurve],
            parameter_range,
            discontinuities,
        },
        silhouette,
        cast_surface,
        light_direction,
    })
}

fn decode_embedded_surface_curve(
    bytes: &[u8],
    int_width: usize,
) -> Option<(
    cadmpeg_ir::geometry::SurfaceCurveFamily,
    EmbeddedIntersection,
)> {
    use cadmpeg_ir::geometry::SurfaceCurveFamily;
    let names = [
        (b"blend_int_cur".as_slice(), SurfaceCurveFamily::Blend),
        (b"bldcur".as_slice(), SurfaceCurveFamily::Blend),
        (
            b"surf_int_cur".as_slice(),
            SurfaceCurveFamily::SurfaceConstrained,
        ),
        (
            b"surfcur".as_slice(),
            SurfaceCurveFamily::SurfaceConstrained,
        ),
        (b"par_int_cur".as_slice(), SurfaceCurveFamily::Parametric),
        (b"parcur".as_slice(), SurfaceCurveFamily::Parametric),
        (b"skin_int_cur".as_slice(), SurfaceCurveFamily::Skin),
        (b"d5c2_cur".as_slice(), SurfaceCurveFamily::Skin),
    ];
    let (marker, name, family) = names.into_iter().find_map(|(name, family)| {
        bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|marker| (marker, name, family))
    })?;
    let mut position = marker + name.len() + 3;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    Some((
        family,
        EmbeddedIntersection {
            surfaces,
            pcurves: [first_pcurve, second_pcurve],
            parameter_range,
            discontinuities,
        },
    ))
}

fn decode_embedded_three_surface_intersection(
    bytes: &[u8],
    int_width: usize,
) -> Option<EmbeddedThreeSurfaceIntersection> {
    let name = b"sss_int_cur";
    let marker = bytes.windows(name.len() + 3).position(|window| {
        window[0] == 0x0f
            && matches!(window[1], 0x0d | 0x0e)
            && usize::from(window[2]) == name.len()
            && &window[3..] == name
    })?;
    let mut position = marker + name.len() + 3;
    let first = decode_embedded_surface(bytes, &mut position, int_width)?;
    let second = decode_embedded_surface(bytes, &mut position, int_width)?;
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    let selector = take_tagged_int(bytes, &mut position, 0x04, int_width)?;
    let third = decode_embedded_surface(bytes, &mut position, int_width)?;
    let (third_pcurve, _) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    Some(EmbeddedThreeSurfaceIntersection {
        surfaces: [first, second, third],
        pcurves: [first_pcurve, second_pcurve, third_pcurve],
        parameter_range,
        discontinuities,
        selector,
    })
}

fn decode_embedded_projection(bytes: &[u8], int_width: usize) -> Option<EmbeddedProjection> {
    let name = b"proj_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    take_bool(bytes, &mut position)?;
    let source = decode_curve_block(bytes, position, int_width)?;
    position = source.end;
    let flag = take_bool(bytes, &mut position)?;
    let tail = if bytes.get(position) == Some(&0x10) {
        cadmpeg_ir::geometry::ProjectionTail::EarlyClose { flag }
    } else {
        cadmpeg_ir::geometry::ProjectionTail::Ranged {
            flag,
            parameter_range: [
                take_range_value(bytes, &mut position)?,
                take_range_value(bytes, &mut position)?,
            ],
            role: take_native_string(bytes, &mut position)?,
        }
    };
    Some(EmbeddedProjection {
        surfaces,
        pcurves: [first_pcurve, second_pcurve],
        parameter_range,
        discontinuities,
        source: source.curve,
        tail,
    })
}

fn decode_embedded_intersection(bytes: &[u8], int_width: usize) -> Option<EmbeddedIntersection> {
    let names: [&[u8]; 3] = [b"int_int_cur", b"surf_surf_int_cur", b"surfintcur"];
    let (marker, name) = names.into_iter().find_map(|name| {
        bytes
            .windows(name.len() + 3)
            .position(|window| {
                window[0] == 0x0f
                    && matches!(window[1], 0x0d | 0x0e)
                    && usize::from(window[2]) == name.len()
                    && &window[3..] == name
            })
            .map(|marker| (marker, name))
    })?;
    let mut position = marker + name.len() + 3;
    let surfaces = [
        decode_embedded_surface(bytes, &mut position, int_width)?,
        decode_embedded_surface(bytes, &mut position, int_width)?,
    ];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    take_bool(bytes, &mut position)?;
    Some(EmbeddedIntersection {
        surfaces,
        pcurves: [first_pcurve, second_pcurve],
        parameter_range,
        discontinuities,
    })
}

fn decode_embedded_two_sided_offset(
    bytes: &[u8],
    int_width: usize,
) -> Option<EmbeddedTwoSidedOffset> {
    let name = b"off_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    let first_surface = decode_embedded_surface(bytes, &mut position, int_width)?;
    let second_surface = decode_embedded_surface(bytes, &mut position, int_width)?;
    let surfaces = [first_surface, second_surface];
    let (first_pcurve, first_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = first_end;
    let (second_pcurve, second_end) = decode_pcurve_block_with_end(bytes, position, int_width)?;
    position = second_end;
    let pcurves = [first_pcurve, second_pcurve];
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    if !matches!(bytes.get(position), Some(0x0a | 0x0b)) {
        return None;
    }
    position += 1;
    let offsets = [
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
    ];
    Some(EmbeddedTwoSidedOffset {
        surfaces,
        pcurves,
        parameter_range,
        discontinuities,
        offsets,
    })
}

fn decode_embedded_surface(
    bytes: &[u8],
    position: &mut usize,
    int_width: usize,
) -> Option<SurfaceGeometry> {
    let kind = take_native_ident(bytes, position)?;
    if kind == "spline" {
        let decoded = decode_surface_block(bytes, *position, int_width)?;
        *position = decoded.end;
        return Some(SurfaceGeometry::Nurbs(decoded.surface));
    }
    let point = take_native_vec3(bytes, position, 0x13)?;
    let point = Point3::new(
        point[0] * LEN_TO_MM,
        point[1] * LEN_TO_MM,
        point[2] * LEN_TO_MM,
    );
    match kind.as_str() {
        "plane" => {
            let normal = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let u_axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            take_bool(bytes, position)?;
            Some(SurfaceGeometry::Plane {
                origin: point,
                normal,
                u_axis,
            })
        }
        "cone" => {
            let axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let major = take_native_vec3(bytes, position, 0x14)?;
            let ref_direction = normalized(major)?;
            take_f64(bytes, position)?;
            take_bool(bytes, position)?;
            take_bool(bytes, position)?;
            let sine = take_f64(bytes, position)?;
            take_f64(bytes, position)?;
            let radius = take_f64(bytes, position)? * LEN_TO_MM;
            for _ in 0..5 {
                take_bool(bytes, position)?;
            }
            if sine.abs() <= f64::EPSILON {
                Some(SurfaceGeometry::Cylinder {
                    origin: point,
                    axis,
                    ref_direction,
                    radius,
                })
            } else {
                Some(SurfaceGeometry::Cone {
                    origin: point,
                    axis,
                    ref_direction,
                    radius,
                    half_angle: sine.abs().asin(),
                })
            }
        }
        "sphere" => {
            let radius = take_f64(bytes, position)? * LEN_TO_MM;
            let ref_direction = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            for _ in 0..5 {
                take_bool(bytes, position)?;
            }
            Some(SurfaceGeometry::Sphere {
                center: point,
                axis,
                ref_direction,
                radius,
            })
        }
        "torus" => {
            let axis = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            let major_radius = take_f64(bytes, position)? * LEN_TO_MM;
            let minor_radius = take_f64(bytes, position)? * LEN_TO_MM;
            let ref_direction = normalized(take_native_vec3(bytes, position, 0x14)?)?;
            for _ in 0..5 {
                take_bool(bytes, position)?;
            }
            Some(SurfaceGeometry::Torus {
                center: point,
                axis,
                ref_direction,
                major_radius,
                minor_radius,
            })
        }
        _ => None,
    }
}

fn take_f64(bytes: &[u8], position: &mut usize) -> Option<f64> {
    if bytes.get(*position) != Some(&0x06) {
        return None;
    }
    let value = read_f64(bytes, *position + 1)?;
    *position += 9;
    Some(value)
}

fn take_bool(bytes: &[u8], position: &mut usize) -> Option<bool> {
    let value = match bytes.get(*position)? {
        0x0a => true,
        0x0b => false,
        _ => return None,
    };
    *position += 1;
    Some(value)
}

fn normalized(value: [f64; 3]) -> Option<Vector3> {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f64>()
        .sqrt();
    (length.is_finite() && length > 0.0)
        .then(|| Vector3::new(value[0] / length, value[1] / length, value[2] / length))
}

fn decode_two_sided_offset(
    bytes: &[u8],
    int_width: usize,
) -> Option<cadmpeg_ir::geometry::ProceduralCurveDefinition> {
    use cadmpeg_ir::geometry::{
        IntcurveSupportContext, IntcurveSupportSide, ProceduralCurveDefinition,
    };

    let name = b"off_int_cur";
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let mut position = marker + name_len + 3;
    for expected in ["null_surface", "null_surface", "nullbs", "nullbs"] {
        if take_native_ident(bytes, &mut position)?.as_str() != expected {
            return None;
        }
    }
    let parameter_range = [
        take_range_value(bytes, &mut position)?,
        take_range_value(bytes, &mut position)?,
    ];
    let discontinuities = [
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
        take_float_array(bytes, &mut position, int_width)?,
    ];
    if !matches!(bytes.get(position), Some(0x0a | 0x0b)) {
        return None;
    }
    position += 1;
    let offsets = [
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
        take_range_value(bytes, &mut position)? * LEN_TO_MM,
    ];
    Some(ProceduralCurveDefinition::TwoSidedOffset {
        context: IntcurveSupportContext {
            sides: [
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                },
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                },
            ],
            parameter_range,
            discontinuities,
        },
        offsets,
    })
}

fn take_native_ident(bytes: &[u8], position: &mut usize) -> Option<String> {
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
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let start = marker + name_len + 3;
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
    let (marker, name_len) = find_intcurve_subtype(bytes, name)?;
    let start = marker + name_len + 3;
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
            return Some(canonical_intcurve_kind(name).into());
        }
    }
    None
}

fn canonical_intcurve_kind(name: &[u8]) -> &str {
    match name {
        b"bldcur" => "blend_int_cur",
        b"blndsprngcur" => "spring_int_cur",
        b"exactcur" => "exact_int_cur",
        b"lawintcur" => "law_int_cur",
        b"offintcur" => "off_int_cur",
        b"offsetintcur" => "offset_int_cur",
        b"offsurfintcur" => "off_surf_int_cur",
        b"parasil" => "para_silh_int_cur",
        b"parcur" => "par_int_cur",
        b"projcur" => "proj_int_cur",
        b"surfcur" => "surf_int_cur",
        b"surfintcur" => "int_int_cur",
        b"d5c2_cur" => "skin_int_cur",
        b"subsetintcur" => "subset_int_cur",
        _ => std::str::from_utf8(name).unwrap_or("intcurve"),
    }
}

fn find_intcurve_subtype(bytes: &[u8], modern: &[u8]) -> Option<(usize, usize)> {
    let legacy: &[u8] = match modern {
        b"blend_int_cur" => b"bldcur",
        b"spring_int_cur" => b"blndsprngcur",
        b"exact_int_cur" => b"exactcur",
        b"law_int_cur" => b"lawintcur",
        b"off_int_cur" => b"offintcur",
        b"offset_int_cur" => b"offsetintcur",
        b"off_surf_int_cur" => b"offsurfintcur",
        b"para_silh_int_cur" => b"parasil",
        b"par_int_cur" => b"parcur",
        b"proj_int_cur" => b"projcur",
        b"surf_int_cur" => b"surfcur",
        b"int_int_cur" => b"surfintcur",
        b"skin_int_cur" => b"d5c2_cur",
        b"subset_int_cur" => b"subsetintcur",
        _ => b"",
    };
    [modern, legacy]
        .into_iter()
        .filter(|name| !name.is_empty())
        .find_map(|name| {
            bytes
                .windows(name.len() + 3)
                .position(|window| {
                    window[0] == 0x0f
                        && matches!(window[1], 0x0d | 0x0e)
                        && usize::from(window[2]) == name.len()
                        && &window[3..] == name
                })
                .map(|marker| (marker, name.len()))
        })
}

fn decode_cache_resolving_refs<T>(
    bytes: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    seen: &mut Vec<usize>,
    decode_inline: fn(&[u8], usize) -> Option<T>,
    int_width: usize,
) -> Option<T> {
    if let Some(decoded) = decode_inline(bytes, int_width) {
        return Some(decoded);
    }
    let table = tables.for_width(int_width);
    for index in subtype_refs(bytes, int_width) {
        if seen.contains(&index) {
            continue;
        }
        let target = *table.get(index)?;
        seen.push(index);
        if let Some(decoded) = decode_cache_resolving_refs(
            subtype_span(active_bytes, target, int_width)?,
            active_bytes,
            tables,
            seen,
            decode_inline,
            int_width,
        ) {
            return Some(decoded);
        }
    }
    None
}

/// Byte positions of the stream's subtype definitions, one table per candidate
/// integer width.
///
/// A subtype definition opens as `0x0f` followed by a `0x0d`/`0x0e` name token
/// other than `ref`; the table indexes definitions in stream order. Definition
/// openings are recognized only at token boundaries — the same byte pattern
/// inside an `f64` payload is data, not a definition — so the table is built by
/// token-walking the framed records, not by scanning raw bytes.
pub struct SubtypeTables {
    tables: [Vec<usize>; INT_WIDTHS.len()],
}

impl SubtypeTables {
    /// Build the tables by token-walking each framed record of `bytes`.
    pub fn from_records(records: &[Record], bytes: &[u8]) -> Self {
        Self {
            tables: INT_WIDTHS.map(|walk_width| {
                let mut table = Vec::new();
                for record in records {
                    collect_defs_in_span(
                        bytes,
                        record.offset,
                        record.offset + record.len,
                        walk_width,
                        &mut table,
                    );
                }
                table
            }),
        }
    }

    /// Build the tables by token-walking `bytes` as one contiguous token run.
    pub fn from_stream(bytes: &[u8]) -> Self {
        Self {
            tables: INT_WIDTHS.map(|walk_width| {
                let mut table = Vec::new();
                collect_defs_in_span(bytes, 0, bytes.len(), walk_width, &mut table);
                table
            }),
        }
    }

    fn for_width(&self, int_width: usize) -> &[usize] {
        INT_WIDTHS
            .iter()
            .position(|&width| width == int_width)
            .map_or(&[], |slot| self.tables[slot].as_slice())
    }
}

/// Append the token-boundary subtype-definition openings in
/// `bytes[start..end]` to `table`. Stops at the first unwalkable token.
fn collect_defs_in_span(
    bytes: &[u8],
    start: usize,
    end: usize,
    int_width: usize,
    table: &mut Vec<usize>,
) {
    let end = end.min(bytes.len());
    let mut pos = start;
    while pos < end {
        if bytes[pos] == 0x0f && matches!(bytes.get(pos + 1), Some(0x0d | 0x0e)) {
            let len = usize::from(*bytes.get(pos + 2).unwrap_or(&0));
            if let Some(name) = bytes.get(pos + 3..pos + 3 + len) {
                if name != b"ref" {
                    table.push(pos);
                }
            }
        }
        match next_token(bytes, pos, int_width) {
            Some(next) => pos = next,
            None => return,
        }
    }
}

/// Subtype-table reference indices in `bytes`, in token order. References are
/// recognized only at token boundaries, mirroring [`SubtypeTables`].
fn subtype_refs(bytes: &[u8], int_width: usize) -> Vec<usize> {
    let mut refs = Vec::new();
    let marker = b"\x0f\x0d\x03ref\x04";
    let mut pos = 0usize;
    while pos < bytes.len() {
        if bytes[pos..].starts_with(marker) {
            if let Some(index) = read_int(bytes, pos + marker.len(), int_width) {
                if index >= 0 {
                    refs.push(index as usize);
                }
            }
        }
        match next_token(bytes, pos, int_width) {
            Some(next) => pos = next,
            None => break,
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
    tables: &SubtypeTables,
) -> Option<NurbsPcurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            tables,
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
    tables: &SubtypeTables,
) -> Option<NurbsPcurve> {
    INT_WIDTHS.into_iter().find_map(|int_width| {
        decode_cache_resolving_refs(
            record_bytes,
            active_bytes,
            tables,
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
        let surface = decode_surface_cache_resolving_refs(
            &record,
            &active,
            &SubtypeTables::from_stream(&active),
        )
        .expect("resolved width-4 ref");
        assert_eq!((surface.u_count, surface.v_count), (2, 2));
    }
}
