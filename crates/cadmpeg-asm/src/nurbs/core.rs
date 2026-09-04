// SPDX-License-Identifier: Apache-2.0
//! Core cached B-spline surface and curve block decoding and their writer-facing patch layouts.
//!
//! The token-space functions decode model values from framed payload tokens
//! and serve the decode path. The byte-space functions additionally record
//! native payload offsets and serve the retained-source patch writer, which
//! edits the original binary stream in place and is therefore byte-addressed
//! by nature.

use crate::nurbs::reader::{
    construction_marker_positions, is_periodic, marker_at, marker_positions,
    owned_marker_positions, read_control_points, read_knots, take_tagged_int, KnotLayout,
    INT_WIDTHS, LEN_TO_MM,
};
use crate::nurbs::subtypes::{decode_cache_resolving_refs, SubtypeTables};
use crate::nurbs::toks;
use crate::nurbs::toks::Cur;
use crate::sab::Token;
use cadmpeg_ir::geometry::{NurbsCurve, NurbsSurface};
use cadmpeg_ir::math::Point3;

use crate::nurbs::toks::take_knot_table as knots;
use cadmpeg_core::decode::alloc_filled;

/// Read `count` control points of `cp_dims` doubles each, scaling positions to
/// millimetres. Token-space counterpart of [`read_control_points`].
fn control_points(
    cur: &mut Cur<'_>,
    count: usize,
    cp_dims: usize,
) -> Option<(Vec<Point3>, Option<Vec<f64>>)> {
    let mut points = Vec::new();
    let mut weights = (cp_dims == 4).then(Vec::new);
    for _ in 0..count {
        let mut comps = [0.0f64; 4];
        for comp in comps.iter_mut().take(cp_dims) {
            *comp = cur.take_f64()?;
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

/// Decode a surface `nubs`/`nurbs` block at token `marker_pos`, returning the
/// surface and the token index just past the block. Token-space counterpart of
/// [`decode_surface_block`].
pub(crate) fn surface_block(toks: &[Token], marker_pos: usize) -> Option<(NurbsSurface, usize)> {
    let cp_dims = toks::marker_at(toks, marker_pos)?.cp_dims();
    let mut cur = Cur::at(toks, marker_pos + 1);

    let degree_u = cur.take_long()?;
    let degree_v = cur.take_long()?;
    if !(1..=20).contains(&degree_u) || !(1..=20).contains(&degree_v) {
        return None;
    }
    // Some caches carry an optional scope identifier (`u`/`v`/`both`) before
    // the enum block; skip it so knot counts stay aligned.
    if matches!(cur.peek(), Some(Token::Ident(_))) {
        cur.bump();
    }
    let mut enums = [0i64; 4];
    for e in &mut enums {
        *e = cur.take_enum()?;
    }
    let n_uniq_u = cur.take_long()?;
    let n_uniq_v = cur.take_long()?;
    if !(1..=1000).contains(&n_uniq_u) || !(1..=1000).contains(&n_uniq_v) {
        return None;
    }

    let (u_knots, n_poles_u) = knots(&mut cur, n_uniq_u as usize, degree_u)?;
    let (v_knots, n_poles_v) = knots(&mut cur, n_uniq_v as usize, degree_v)?;
    if n_poles_u.checked_mul(n_poles_v).is_none_or(|n| n > 200_000) {
        return None;
    }

    // Grid is stored v-major (v outer, u inner); transpose to the IR's u-major
    // order where index `u * v_count + v` is pole `(u, v)`.
    let (flat, flat_w) = control_points(&mut cur, n_poles_u * n_poles_v, cp_dims)?;
    let pole_count = n_poles_u * n_poles_v;
    let mut grid = alloc_filled(pole_count, Point3::new(0.0, 0.0, 0.0), "asm_nurbs_poles").ok()?;
    let mut weights = match &flat_w {
        Some(_) => Some(alloc_filled(pole_count, 0.0f64, "asm_nurbs_weights").ok()?),
        None => None,
    };
    for v in 0..n_poles_v {
        for u in 0..n_poles_u {
            let file_idx = v * n_poles_u + u;
            let ir_idx = u * n_poles_v + v;
            grid[ir_idx] = flat[file_idx];
            if let (Some(w), Some(fw)) = (weights.as_mut(), flat_w.as_ref()) {
                w[ir_idx] = fw[file_idx];
            }
        }
    }

    let surface = NurbsSurface::new(
        degree_u as u32,
        degree_v as u32,
        u_knots,
        v_knots,
        n_poles_u as u32,
        n_poles_v as u32,
        grid,
        weights,
        false,
        is_periodic(enums[0]),
        is_periodic(enums[1]),
    )
    .ok()?;
    Some((surface, cur.pos()))
}

/// Decode a curve `nubs`/`nurbs` block at token `marker_pos`, returning the
/// curve and the token index just past the block. Token-space counterpart of
/// [`decode_curve_block`].
pub(crate) fn curve_block(toks: &[Token], marker_pos: usize) -> Option<(NurbsCurve, usize)> {
    let cp_dims = toks::marker_at(toks, marker_pos)?.cp_dims();
    let mut cur = Cur::at(toks, marker_pos + 1);

    let degree = cur.take_long()?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = cur.take_enum()?;
    let n_uniq = cur.take_long()?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knot_vector, n_poles) = knots(&mut cur, n_uniq as usize, degree)?;
    let (points, weights) = control_points(&mut cur, n_poles, cp_dims)?;

    let curve = NurbsCurve::new(
        degree as u32,
        knot_vector,
        points,
        weights,
        is_periodic(closure),
    )
    .ok()?;
    Some((curve, cur.pos()))
}

/// Decode the face-surface cache of a spline surface record from its payload
/// tokens: the LAST valid surface block (the final `setSurfaceShape` cache;
/// earlier blocks are support surfaces or 2D pcurves), except in a
/// `comp_spl_sur` compound, whose own cache comes first.
pub(crate) fn surface_cache(toks: &[Token]) -> Option<NurbsSurface> {
    let scope = toks::owned_cache_scope(toks).unwrap_or(toks);
    let mut caches = toks::owned_marker_positions(scope)
        .into_iter()
        .filter_map(|pos| surface_block(scope, pos).map(|(surface, _)| surface));
    let compound = scope.iter().any(|token| {
        matches!(token, Token::Ident(name) | Token::SubIdent(name) if name == "comp_spl_sur")
    });
    if compound {
        caches.next()
    } else {
        caches.next_back()
    }
}

/// Decode the surface cache a subtype scope itself owns: the first surface
/// block outside every construction the scope nests.
pub(crate) fn owned_surface_cache(scope: &[Token]) -> Option<NurbsSurface> {
    toks::owned_marker_positions(scope)
        .into_iter()
        .find_map(|pos| surface_block(scope, pos).map(|(surface, _)| surface))
}

/// Decode the 3D curve cache of a procedural curve record from its payload
/// tokens: the FIRST valid curve block (surface and 2D pcurve blocks do not
/// parse as a 3D curve block).
pub(crate) fn curve_cache(toks: &[Token]) -> Option<NurbsCurve> {
    let scope = toks::owned_cache_scope(toks).unwrap_or(toks);
    toks::owned_marker_positions(scope)
        .into_iter()
        .find_map(|pos| curve_block(scope, pos).map(|(curve, _)| curve))
}

/// Decode the 3D curve cache a subtype scope itself owns: the first curve
/// block outside every construction the scope nests.
pub(crate) fn owned_curve_cache(scope: &[Token]) -> Option<NurbsCurve> {
    toks::owned_marker_positions(scope)
        .into_iter()
        .find_map(|pos| curve_block(scope, pos).map(|(curve, _)| curve))
}

/// Resolve a cache through `{ref N}` subtype references: decode inline first,
/// then follow each reference into the stream's subtype table. `seen` breaks
/// reference cycles. Token-space counterpart of [`decode_cache_resolving_refs`].
pub(crate) fn cache_resolving_refs<T>(
    toks: &[Token],
    table: &toks::SubtypeTable,
    seen: &mut Vec<usize>,
    decode_inline: fn(&[Token]) -> Option<T>,
) -> Option<T> {
    if let Some(decoded) = decode_inline(toks) {
        return Some(decoded);
    }
    for index in toks::subtype_refs(toks) {
        if seen.contains(&index) {
            continue;
        }
        seen.push(index);
        let target = table.span(index)?;
        if let Some(decoded) = cache_resolving_refs(target, table, seen, decode_inline) {
            return Some(decoded);
        }
    }
    None
}

/// [`surface_cache`], following subtype-table references.
pub fn surface_cache_resolving_refs(
    toks: &[Token],
    table: &toks::SubtypeTable,
) -> Option<NurbsSurface> {
    cache_resolving_refs(toks, table, &mut Vec::new(), surface_cache)
}

/// [`owned_surface_cache`], following subtype-table references.
pub(crate) fn owned_surface_cache_resolving_refs(
    toks: &[Token],
    table: &toks::SubtypeTable,
) -> Option<NurbsSurface> {
    cache_resolving_refs(toks, table, &mut Vec::new(), owned_surface_cache)
}

/// [`curve_cache`], following subtype-table references.
pub fn curve_cache_resolving_refs(
    toks: &[Token],
    table: &toks::SubtypeTable,
) -> Option<NurbsCurve> {
    cache_resolving_refs(toks, table, &mut Vec::new(), curve_cache)
}

/// [`owned_curve_cache`], following subtype-table references.
pub(crate) fn owned_curve_cache_resolving_refs(
    toks: &[Token],
    table: &toks::SubtypeTable,
) -> Option<NurbsCurve> {
    cache_resolving_refs(toks, table, &mut Vec::new(), owned_curve_cache)
}

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
    let pole_count = n_poles_u * n_poles_v;
    let mut control_points =
        alloc_filled(pole_count, Point3::new(0.0, 0.0, 0.0), "asm_nurbs_poles").ok()?;
    let mut weights = match &flat_w {
        Some(_) => Some(alloc_filled(pole_count, 0.0f64, "asm_nurbs_weights").ok()?),
        None => None,
    };
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

    let surface = NurbsSurface::new(
        degree_u as u32,
        degree_v as u32,
        u_knots,
        v_knots,
        n_poles_u as u32,
        n_poles_v as u32,
        control_points,
        weights,
        false,
        is_periodic(enums[0]),
        is_periodic(enums[1]),
    )
    .ok()?;
    Some(DecodedSurfaceBlock {
        surface,
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
pub struct SurfacePatchLayout {
    /// Payload width of integer and enum fields.
    pub int_width: usize,
    /// Native v-major tagged-double payload offsets, excluding each tag byte.
    pub control_value_offsets: Vec<usize>,
    /// Whether every pole includes a fourth rational weight component.
    pub rational: bool,
    /// Pole count in the u direction.
    pub u_count: usize,
    /// Pole count in the v direction.
    pub v_count: usize,
    /// Native payload offsets and expanded run lengths for U knots.
    pub u_knots: KnotPatchLayout,
    /// Native payload offsets and expanded run lengths for V knots.
    pub v_knots: KnotPatchLayout,
    /// Offset immediately after the final control component.
    pub end: usize,
    /// Payload offsets for the U/V closure enums.
    pub periodic_value_offsets: [usize; 2],
    /// Payload offsets for the U/V degree integers.
    pub degree_value_offsets: [usize; 2],
}

/// Unique native knot payload offsets.
pub struct KnotPatchLayout {
    /// Payload offsets for unique knot values.
    pub value_offsets: Vec<usize>,
    /// Payload offsets for stored multiplicities.
    pub multiplicity_offsets: Vec<usize>,
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

/// Locate the final valid `nubs`/`nurbs` surface block at the stream's known
/// integer width.
pub fn final_surface_patch_layout(record: &[u8], int_width: usize) -> Option<SurfacePatchLayout> {
    let decoded = construction_marker_positions(record, int_width)
        .into_iter()
        .filter_map(|position| decode_surface_block(record, position, int_width))
        .next_back()?;
    Some(SurfacePatchLayout {
        int_width: decoded.int_width,
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        u_count: decoded.surface.u_count() as usize,
        v_count: decoded.surface.v_count() as usize,
        u_knots: decoded.u_knot_layout.into(),
        v_knots: decoded.v_knot_layout.into(),
        end: decoded.end,
        periodic_value_offsets: decoded.periodic_value_offsets,
        degree_value_offsets: decoded.degree_value_offsets,
    })
}

/// Locate the surface block at `ordinal` among valid surface caches at the
/// stream's known integer width.
pub fn surface_patch_layout_at(
    record: &[u8],
    ordinal: usize,
    int_width: usize,
) -> Option<SurfacePatchLayout> {
    let decoded = construction_marker_positions(record, int_width)
        .into_iter()
        .filter_map(|position| decode_surface_block(record, position, int_width))
        .nth(ordinal)?;
    Some(SurfacePatchLayout {
        int_width: decoded.int_width,
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        u_count: decoded.surface.u_count() as usize,
        v_count: decoded.surface.v_count() as usize,
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

    let curve = NurbsCurve::new(
        degree as u32,
        knots,
        control_points,
        weights,
        is_periodic(closure),
    )
    .ok()?;
    Some(DecodedCurveBlock {
        curve,
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
pub struct CurvePatchLayout {
    /// Payload width of integer and enum fields.
    pub int_width: usize,
    /// Tagged-double payload offsets in pole/component order.
    pub control_value_offsets: Vec<usize>,
    /// Whether every pole includes a fourth rational weight component.
    pub rational: bool,
    /// Number of control points.
    pub control_count: usize,
    /// Native unique-knot payloads and expanded run lengths.
    pub knots: KnotPatchLayout,
    /// Offset immediately after the final control component.
    pub end: usize,
    /// Payload offset for the closure enum.
    pub periodic_value_offset: usize,
    /// Payload offset for the degree integer.
    pub degree_value_offset: usize,
}

/// Locate the first valid 3D curve cache at the stream's known integer width.
pub fn first_curve_patch_layout(record: &[u8], int_width: usize) -> Option<CurvePatchLayout> {
    let decoded = construction_marker_positions(record, int_width)
        .into_iter()
        .find_map(|position| decode_curve_block(record, position, int_width))?;
    Some(CurvePatchLayout {
        int_width: decoded.int_width,
        control_count: decoded.curve.control_points().len(),
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        knots: decoded.knot_layout.into(),
        end: decoded.end,
        periodic_value_offset: decoded.periodic_value_offset,
        degree_value_offset: decoded.degree_value_offset,
    })
}

/// Locate the final valid 3D curve cache at the stream's known integer width.
pub fn final_curve_patch_layout(record: &[u8], int_width: usize) -> Option<CurvePatchLayout> {
    let decoded = construction_marker_positions(record, int_width)
        .into_iter()
        .filter_map(|position| decode_curve_block(record, position, int_width))
        .next_back()?;
    Some(CurvePatchLayout {
        int_width: decoded.int_width,
        control_count: decoded.curve.control_points().len(),
        control_value_offsets: decoded.control_value_offsets,
        rational: decoded.rational,
        knots: decoded.knot_layout.into(),
        end: decoded.end,
        periodic_value_offset: decoded.periodic_value_offset,
        degree_value_offset: decoded.degree_value_offset,
    })
}

/// Decode the unique well-formed surface cache across both integer widths.
///
/// This generic entry point has no stream-width, owning-scope, or family-role
/// witness. It therefore withholds when more than one `(width, marker)`
/// candidate decodes.
pub fn decode_surface_cache(record_bytes: &[u8]) -> Option<NurbsSurface> {
    let mut decoded = None;
    for int_width in INT_WIDTHS {
        for position in marker_positions(record_bytes) {
            if let Some(candidate) = decode_surface_block(record_bytes, position, int_width) {
                if decoded.is_some() {
                    return None;
                }
                decoded = Some(candidate.surface);
            }
        }
    }
    decoded
}

/// Decode the surface cache a subtype scope itself owns: the first surface
/// block outside every construction the scope nests. A scope whose supports are
/// nested constructions carries their caches too, and those are not its own.
pub(crate) fn decode_owned_surface_cache_at(
    scope: &[u8],
    int_width: usize,
) -> Option<NurbsSurface> {
    owned_marker_positions(scope, int_width)
        .into_iter()
        .find_map(|pos| decode_surface_block(scope, pos, int_width).map(|decoded| decoded.surface))
}

/// [`decode_owned_surface_cache_at`], following subtype-table references at the
/// stream's integer width. Every caller reaches a scope through a walk that
/// already read the width, and the subtype table indexes different offsets at
/// each width, so probing the other one walks the reference graph a second time
/// against a table built for a stream this is not.
pub(crate) fn decode_owned_surface_cache_resolving_refs_at(
    scope: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    int_width: usize,
) -> Option<NurbsSurface> {
    decode_cache_resolving_refs(
        scope,
        active_bytes,
        tables,
        &mut Vec::new(),
        decode_owned_surface_cache_at,
        int_width,
    )
}

/// Decode the unique well-formed 3D curve cache across both integer widths.
///
/// This generic entry point has no stream-width or owning-scope witness. It
/// therefore withholds when more than one `(width, marker)` candidate decodes.
pub fn decode_curve_cache(record_bytes: &[u8]) -> Option<NurbsCurve> {
    let mut decoded = None;
    for int_width in INT_WIDTHS {
        for position in marker_positions(record_bytes) {
            if let Some(candidate) = decode_curve_block(record_bytes, position, int_width) {
                if decoded.is_some() {
                    return None;
                }
                decoded = Some(candidate.curve);
            }
        }
    }
    decoded
}

/// Decode the 3D curve cache a subtype scope itself owns: the first curve block
/// outside every construction the scope nests.
pub fn decode_owned_curve_cache_at(scope: &[u8], int_width: usize) -> Option<NurbsCurve> {
    owned_marker_positions(scope, int_width)
        .into_iter()
        .find_map(|pos| decode_curve_block(scope, pos, int_width).map(|decoded| decoded.curve))
}

/// [`decode_owned_curve_cache_at`], following subtype-table references at the
/// stream's integer width.
pub(crate) fn decode_owned_curve_cache_resolving_refs_at(
    scope: &[u8],
    active_bytes: &[u8],
    tables: &SubtypeTables,
    int_width: usize,
) -> Option<NurbsCurve> {
    decode_cache_resolving_refs(
        scope,
        active_bytes,
        tables,
        &mut Vec::new(),
        decode_owned_curve_cache_at,
        int_width,
    )
}
