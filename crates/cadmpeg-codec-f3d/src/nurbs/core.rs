// SPDX-License-Identifier: Apache-2.0
//! Core cached B-spline surface and curve block decoding and their writer-facing patch layouts.

use crate::nurbs::reader::{
    is_periodic, marker_at, marker_positions, read_control_points, read_knots, take_tagged_int,
    KnotLayout, INT_WIDTHS,
};
use crate::nurbs::subtypes::{decode_cache_resolving_refs, SubtypeTables};
use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::Point3;

/// Decode a surface `nubs`/`nurbs` block at `marker_pos`, or `None` if the bytes
/// there are not a well-formed surface block.
pub(crate) struct DecodedSurfaceBlock {
    pub(crate) surface: NurbsSurface,
    pub(crate) end: usize,
    control_value_offsets: Vec<usize>,
    rational: bool,
    u_knot_layout: KnotLayout,
    v_knot_layout: KnotLayout,
    periodic_value_offsets: [usize; 2],
    degree_value_offsets: [usize; 2],
    int_width: usize,
}

pub(crate) fn decode_surface_block(
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
        int_width,
    })
}

/// Writable value offsets for the final valid surface cache in one carrier record.
pub(crate) struct SurfacePatchLayout {
    /// Payload width of integer and enum fields.
    pub(crate) int_width: usize,
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
        int_width: decoded.int_width,
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
        int_width: decoded.int_width,
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
pub(crate) struct DecodedCurveBlock {
    pub(crate) curve: NurbsCurve,
    pub(crate) end: usize,
    control_value_offsets: Vec<usize>,
    rational: bool,
    knot_layout: KnotLayout,
    periodic_value_offset: usize,
    degree_value_offset: usize,
    int_width: usize,
}

pub(crate) fn decode_curve_block(
    b: &[u8],
    marker_pos: usize,
    int_width: usize,
) -> Option<DecodedCurveBlock> {
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
        int_width,
    })
}

/// Writable value offsets for a 3D curve cache in one carrier record.
pub(crate) struct CurvePatchLayout {
    /// Payload width of integer and enum fields.
    pub(crate) int_width: usize,
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
        int_width: decoded.int_width,
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
        int_width: decoded.int_width,
        control_count: decoded.curve.control_points.len(),
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        knots: decoded.knot_layout.into(),
        end: decoded.end,
        periodic_value_offset: decoded.periodic_value_offset,
        degree_value_offset: decoded.degree_value_offset,
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

pub(crate) fn decode_curve_cache_at(record_bytes: &[u8], int_width: usize) -> Option<NurbsCurve> {
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
