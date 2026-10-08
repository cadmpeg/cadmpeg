// SPDX-License-Identifier: Apache-2.0
//! Core cached B-spline surface and curve block decoding and their writer-facing patch layouts.
//!
//! The token-space functions decode model values from framed payload tokens
//! and serve the decode path. The byte-space functions additionally record
//! native payload offsets and serve the retained-source patch writer, which
//! edits the original binary stream in place and is therefore byte-addressed
//! by nature.

use crate::kernel_header::RefWidth;
use crate::nurbs::reader::{
    construction_marker_positions, is_periodic, marker_at, marker_positions, read_control_points,
    read_knots, take_tagged_int, BsplineMarker, KnotLayout, ReadPoles3, INT_WIDTHS, LEN_TO_MM,
};
use crate::nurbs::subtypes;
use crate::nurbs::toks;
use crate::nurbs::toks::Cur;
use crate::sab::Token;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis};
use cadmpeg_ir::math::Point3;

use crate::nurbs::toks::take_knot_table as knots;

const MAX_RECOVERY_UNIQUE_KNOTS: u64 = 1_000;
const MAX_RECOVERY_SURFACE_POLES: u64 = 200_000;

macro_rules! propagate_resource {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        }
    };
}

/// Read `count` control points in the marker-selected form, scaling positions to
/// millimetres. Token-space counterpart of [`read_control_points`].
fn control_points(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    cur: &mut Cur<'_>,
    count: usize,
    marker: BsplineMarker,
) -> Option<Result<ReadPoles3, cadmpeg_core::CodecError>> {
    let mut poles = propagate_resource!(ReadPoles3::with_counted_capacity(
        ctx,
        count,
        marker.rational(),
    ));
    for _ in 0..count {
        let mut comps = [0.0f64; 4];
        for comp in comps.iter_mut().take(marker.cp_dims()) {
            *comp = cur.take_f64()?;
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
    Some(Ok(poles))
}

/// Decode a surface `nubs`/`nurbs` block at token `marker_pos`, returning the
/// surface and the token index just past the block. Token-space counterpart of
/// [`decode_surface_block`].
pub(super) fn surface_block(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    marker_pos: usize,
) -> Option<Result<(NurbsSurface, usize), cadmpeg_core::CodecError>> {
    let marker = toks::marker_at(toks, marker_pos)?;
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
    if n_uniq_u < 1 || n_uniq_v < 1 {
        return None;
    }
    for count in [n_uniq_u, n_uniq_v] {
        let count = u64::try_from(count).ok()?;
        if count > MAX_RECOVERY_UNIQUE_KNOTS {
            return Some(Err(ctx.refuse_codec_limit(
                "ASM unique knot recovery",
                MAX_RECOVERY_UNIQUE_KNOTS,
                count,
            )));
        }
    }

    let (u_knots, n_poles_u) = propagate_resource!(knots(
        ctx,
        &mut cur,
        usize::try_from(n_uniq_u).ok()?,
        degree_u
    )?);
    let (v_knots, n_poles_v) = propagate_resource!(knots(
        ctx,
        &mut cur,
        usize::try_from(n_uniq_v).ok()?,
        degree_v
    )?);
    let Some(pole_count) = n_poles_u.checked_mul(n_poles_v) else {
        return Some(Err(ctx.refuse_codec_limit(
            "ASM surface pole recovery",
            MAX_RECOVERY_SURFACE_POLES,
            u64::MAX,
        )));
    };
    let pole_population = cadmpeg_core::decode::u64_from_index(pole_count);
    if pole_population > MAX_RECOVERY_SURFACE_POLES {
        return Some(Err(ctx.refuse_codec_limit(
            "ASM surface pole recovery",
            MAX_RECOVERY_SURFACE_POLES,
            pole_population,
        )));
    }

    // Grid is stored v-major (v outer, u inner); transpose to the IR's u-major
    // order where index `u * v_count + v` is pole `(u, v)`.
    let poles = propagate_resource!(control_points(ctx, &mut cur, pole_count, marker)?);
    let grid = propagate_resource!(poles.into_counted_transposed_grid(ctx, n_poles_u, n_poles_v)?);
    let surface = propagate_resource!(NurbsSurface::new(
        ctx,
        NurbsSurfaceAxis::new(
            u32::try_from(degree_u).ok()?,
            u_knots,
            is_periodic(enums[0]),
        ),
        NurbsSurfaceAxis::new(
            u32::try_from(degree_v).ok()?,
            v_knots,
            is_periodic(enums[1]),
        ),
        grid,
        false,
    ))
    .ok()?;
    Some(Ok((surface, cur.pos())))
}

/// Decode a curve `nubs`/`nurbs` block at token `marker_pos`, returning the
/// curve and the token index just past the block. Token-space counterpart of
/// [`decode_curve_block`].
pub(super) fn curve_block(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    marker_pos: usize,
) -> Option<Result<(NurbsCurve, usize), cadmpeg_core::CodecError>> {
    let marker = toks::marker_at(toks, marker_pos)?;
    let mut cur = Cur::at(toks, marker_pos + 1);

    let degree = cur.take_long()?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = cur.take_enum()?;
    let n_uniq = cur.take_long()?;
    if n_uniq < 1 {
        return None;
    }
    let knot_count = u64::try_from(n_uniq).ok()?;
    if knot_count > MAX_RECOVERY_UNIQUE_KNOTS {
        return Some(Err(ctx.refuse_codec_limit(
            "ASM unique knot recovery",
            MAX_RECOVERY_UNIQUE_KNOTS,
            knot_count,
        )));
    }
    let (knot_vector, n_poles) =
        propagate_resource!(knots(ctx, &mut cur, usize::try_from(n_uniq).ok()?, degree)?);
    let poles = propagate_resource!(control_points(ctx, &mut cur, n_poles, marker)?);

    let curve = propagate_resource!(NurbsCurve::new(
        ctx,
        u32::try_from(degree).ok()?,
        knot_vector,
        poles.into_lane(),
        is_periodic(closure),
    ))
    .ok()?;
    Some(Ok((curve, cur.pos())))
}

/// Decode the face-surface cache of a spline surface record from its payload
/// tokens: the LAST valid surface block (the final `setSurfaceShape` cache;
/// earlier blocks are support surfaces or 2D pcurves), except in a
/// `comp_spl_sur` compound, whose own cache comes first.
pub(super) fn surface_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
) -> Option<Result<NurbsSurface, cadmpeg_core::CodecError>> {
    let scope = propagate_resource!(toks::cache_scope(ctx, toks)?);
    let positions = propagate_resource!(toks::owned_marker_positions(ctx, scope)?);
    let compound = scope.iter().any(|token| {
        matches!(token, Token::Ident(name) | Token::SubIdent(name) if name == "comp_spl_sur")
    });
    if compound {
        positions.into_iter().find_map(|pos| {
            surface_block(ctx, scope, pos).map(|result| result.map(|(surface, _)| surface))
        })
    } else {
        positions.into_iter().rev().find_map(|pos| {
            surface_block(ctx, scope, pos).map(|result| result.map(|(surface, _)| surface))
        })
    }
}

/// Decode the surface cache a subtype scope itself owns: the first surface
/// block outside every construction the scope nests.
pub(super) fn owned_surface_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
) -> Option<Result<NurbsSurface, cadmpeg_core::CodecError>> {
    let tokens = scope.tokens();
    propagate_resource!(scope.owned_marker_positions(ctx))
        .into_iter()
        .find_map(|pos| {
            surface_block(ctx, tokens, pos).map(|result| result.map(|(surface, _)| surface))
        })
}

/// Decode the 3D curve cache of a procedural curve record from its payload
/// tokens: the FIRST valid curve block (surface and 2D pcurve blocks do not
/// parse as a 3D curve block).
pub(super) fn curve_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
) -> Option<Result<NurbsCurve, cadmpeg_core::CodecError>> {
    let scope = propagate_resource!(toks::cache_scope(ctx, toks)?);
    propagate_resource!(toks::owned_marker_positions(ctx, scope)?)
        .into_iter()
        .find_map(|pos| curve_block(ctx, scope, pos).map(|result| result.map(|(curve, _)| curve)))
}

/// Decode the 3D curve cache a subtype scope itself owns: the first curve
/// block outside every construction the scope nests.
pub(super) fn owned_curve_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
) -> Option<Result<NurbsCurve, cadmpeg_core::CodecError>> {
    let tokens = scope.tokens();
    propagate_resource!(scope.owned_marker_positions(ctx))
        .into_iter()
        .find_map(|pos| curve_block(ctx, tokens, pos).map(|result| result.map(|(curve, _)| curve)))
}

/// Decode the cache of each scope the `{ref N}` references in `toks` reach,
/// depth first in stream order. A visited set breaks reference cycles.
///
/// [`toks::SubtypeTable::span`] answers with the balanced scope the reference
/// names, so `decode_scope` reads a proven scope and needs no walk of its own
/// to establish one.
fn cache_from_subtype_refs<T, D>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    table: &toks::SubtypeTable,
    decode_scope: D,
) -> Option<Result<T, cadmpeg_core::CodecError>>
where
    D: Fn(
        &cadmpeg_core::decode::DecodeContext<'_>,
        toks::SubtypeScope<'_>,
    ) -> Option<Result<T, cadmpeg_core::CodecError>>,
{
    let mut seen = std::collections::HashSet::new();
    let mut pending = propagate_resource!(ctx.collection_vec(1, "ASM subtype search stack"));
    pending.push(toks::subtype_refs(toks));
    while let Some(references) = pending.last_mut() {
        let Some(index) = references.next() else {
            pending.pop();
            continue;
        };
        if !propagate_resource!(ctx.insert_hash_set(&mut seen, index, "ASM subtype search visited"))
        {
            continue;
        }
        // The doc states what the index means. `docs/formats/asm.md`: "A named
        // `ref N` scope or compact `0x0F LONG N 0x10` scope nested inside a
        // surface, curve, or pcurve body indexes a per-file subtype table, not
        // a byte offset. Each subtype definition -- a `0x0F` opening followed
        // by a `0x0d`/`0x0e` name token other than `ref` -- contributes one
        // table entry in stream order." An index at or beyond the table's
        // length therefore names no definition the stream states. What the
        // decoder does about it is the decoder's decision: the search refuses
        // the stream rather than skipping the reference and reading the one
        // behind it.
        let target = table.span(index)?;
        if let Some(decoded) = decode_scope(ctx, target) {
            return Some(decoded);
        }
        propagate_resource!(ctx.push_vec(
            &mut pending,
            toks::subtype_refs(target.tokens()),
            "ASM subtype search stack"
        ));
    }
    None
}

/// Decode a surface cache, following subtype-table references.
pub fn surface_cache_resolving_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    table: &toks::SubtypeTable,
) -> Option<Result<NurbsSurface, cadmpeg_core::CodecError>> {
    surface_cache(ctx, toks).or_else(|| {
        cache_from_subtype_refs(ctx, toks, table, |ctx, scope| {
            surface_cache(ctx, scope.tokens())
        })
    })
}

/// Read the owned cache, resolving a direct subtype-table alias.
pub(super) fn owned_surface_cache_resolving_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
    table: &toks::SubtypeTable,
) -> Option<Result<NurbsSurface, cadmpeg_core::CodecError>> {
    if let Some(cache) = owned_surface_cache(ctx, scope) {
        return Some(cache);
    }
    // Only a reference scope aliases another construction. References inside
    // a named construction are its supports or guides, whose caches use their
    // own parameter charts and cannot stand in for the owner's missing cache.
    let index = match scope.interior() {
        [Token::Ident(name), Token::Long(index)] if name == "ref" => *index,
        [Token::Long(index)] => *index,
        _ => return None,
    };
    let target = table.span(usize::try_from(index).ok()?)?;
    owned_surface_cache(ctx, target)
}

/// Decode a curve cache, following subtype-table references.
pub fn curve_cache_resolving_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    table: &toks::SubtypeTable,
) -> Option<Result<NurbsCurve, cadmpeg_core::CodecError>> {
    curve_cache(ctx, toks).or_else(|| {
        cache_from_subtype_refs(ctx, toks, table, |ctx, scope| {
            curve_cache(ctx, scope.tokens())
        })
    })
}

/// [`owned_curve_cache`], following subtype-table references.
pub(super) fn owned_curve_cache_resolving_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
    table: &toks::SubtypeTable,
) -> Option<Result<NurbsCurve, cadmpeg_core::CodecError>> {
    owned_curve_cache(ctx, scope)
        .or_else(|| cache_from_subtype_refs(ctx, scope.tokens(), table, owned_curve_cache))
}

/// Decode a surface `nubs`/`nurbs` block at `marker_pos`, or `None` if the bytes
/// there are not a well-formed surface block.
pub struct SurfacePatchLayout {
    /// Decoded surface cache.
    pub surface: NurbsSurface,
    control_start: usize,
    /// Native payload offsets for U knots.
    pub u_knots: KnotLayout,
    /// Native payload offsets for V knots.
    pub v_knots: KnotLayout,
    /// Payload offsets for the U/V closure enums.
    pub periodic_value_offsets: [usize; 2],
    /// Payload offsets for the U/V degree integers.
    pub degree_value_offsets: [usize; 2],
}

impl SurfacePatchLayout {
    /// Offset immediately after the final control component.
    pub fn end(&self) -> usize {
        self.control_start + self.control_value_offsets().len() * 9
    }

    /// Native v-major tagged-double payload offsets, excluding each tag byte.
    pub fn control_value_offsets(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        let components = if self.surface.weights().is_some() {
            4
        } else {
            3
        };
        (0..self.surface.poles().len() * components)
            .map(|ordinal| self.control_start + ordinal * 9 + 1)
    }
}

pub(super) fn decode_surface_block(
    b: &[u8],
    marker_pos: usize,
    int_width: RefWidth,
) -> Option<SurfacePatchLayout> {
    // Byte-addressed patch inspection uses an independent desktop writer policy.
    let writer_arena = cadmpeg_core::decode::DecodeArena::new();
    let writer_policy = cadmpeg_core::decode::DecodePolicy::desktop();
    let (writer_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &writer_arena, &writer_policy)
            .ok()?;
    let marker = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker.byte_len();

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
        let len = usize::from(*b.get(pos + 1)?);
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

    let (u_knots, n_poles_u, u_knot_layout) = read_knots(
        b,
        &mut pos,
        usize::try_from(n_uniq_u).ok()?,
        degree_u,
        int_width,
    )?;
    let (v_knots, n_poles_v, v_knot_layout) = read_knots(
        b,
        &mut pos,
        usize::try_from(n_uniq_v).ok()?,
        degree_v,
        int_width,
    )?;
    if n_poles_u.checked_mul(n_poles_v).is_none_or(|n| n > 200_000) {
        return None;
    }

    // Grid is stored v-major (v outer, u inner); transpose to the IR's u-major
    // order where index `u * v_count + v` is pole `(u, v)`.
    let control_start = pos;
    let poles = read_control_points(b, &mut pos, n_poles_u * n_poles_v, marker)?;
    let grid = poles.into_transposed_grid(n_poles_u, n_poles_v)?;
    let surface = NurbsSurface::new(
        &writer_ctx,
        NurbsSurfaceAxis::new(
            u32::try_from(degree_u).ok()?,
            u_knots,
            is_periodic(enums[0]),
        ),
        NurbsSurfaceAxis::new(
            u32::try_from(degree_v).ok()?,
            v_knots,
            is_periodic(enums[1]),
        ),
        grid,
        false,
    )
    .ok()?
    .ok()?;
    Some(SurfacePatchLayout {
        surface,
        control_start,
        u_knots: u_knot_layout,
        v_knots: v_knot_layout,
        periodic_value_offsets: [enum_value_offsets[0], enum_value_offsets[1]],
        degree_value_offsets: [degree_u_offset, degree_v_offset],
    })
}

/// Locate the final valid `nubs`/`nurbs` surface block at the stream's known
/// integer width.
pub fn final_surface_patch_layout(
    record: &[u8],
    int_width: RefWidth,
) -> Option<SurfacePatchLayout> {
    construction_marker_positions(record, int_width)?
        .into_iter()
        .filter_map(|position| decode_surface_block(record, position, int_width))
        .next_back()
}

/// Locate the surface block at `ordinal` among valid surface caches at the
/// stream's known integer width.
pub fn surface_patch_layout_at(
    record: &[u8],
    ordinal: usize,
    int_width: RefWidth,
) -> Option<SurfacePatchLayout> {
    construction_marker_positions(record, int_width)?
        .into_iter()
        .filter_map(|position| decode_surface_block(record, position, int_width))
        .nth(ordinal)
}

/// Decode a curve `nubs`/`nurbs` block at `marker_pos`, or `None` if the bytes
/// there are not a well-formed 3D curve block.
pub struct CurvePatchLayout {
    /// Decoded curve cache.
    pub curve: NurbsCurve,
    control_start: usize,
    /// Native unique-knot payloads.
    pub knots: KnotLayout,
    /// Payload offset for the closure enum.
    pub periodic_value_offset: usize,
    /// Payload offset for the degree integer.
    pub degree_value_offset: usize,
}

impl CurvePatchLayout {
    /// Offset immediately after the final control component.
    pub fn end(&self) -> usize {
        self.control_start + self.control_value_offsets().len() * 9
    }

    /// Tagged-double payload offsets in pole/component order.
    pub fn control_value_offsets(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        let components = if self.curve.weights().is_some() { 4 } else { 3 };
        (0..self.curve.control_points().len() * components)
            .map(|ordinal| self.control_start + ordinal * 9 + 1)
    }
}

pub(super) fn decode_curve_block(
    b: &[u8],
    marker_pos: usize,
    int_width: RefWidth,
) -> Option<CurvePatchLayout> {
    // Byte-addressed patch inspection uses an independent desktop writer policy.
    let writer_arena = cadmpeg_core::decode::DecodeArena::new();
    let writer_policy = cadmpeg_core::decode::DecodePolicy::desktop();
    let (writer_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &writer_arena, &writer_policy)
            .ok()?;
    let marker = marker_at(b, marker_pos)?;
    let mut pos = marker_pos + marker.byte_len();

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
    let (knots, n_poles, knot_layout) = read_knots(
        b,
        &mut pos,
        usize::try_from(n_uniq).ok()?,
        degree,
        int_width,
    )?;
    let control_start = pos;
    let poles = read_control_points(b, &mut pos, n_poles, marker)?;

    let curve = NurbsCurve::new(
        &writer_ctx,
        u32::try_from(degree).ok()?,
        knots,
        poles.into_lane(),
        is_periodic(closure),
    )
    .ok()?
    .ok()?;
    Some(CurvePatchLayout {
        curve,
        control_start,
        knots: knot_layout,
        periodic_value_offset,
        degree_value_offset,
    })
}

/// Locate the first valid 3D curve cache at the stream's known integer width.
pub fn first_curve_patch_layout(record: &[u8], int_width: RefWidth) -> Option<CurvePatchLayout> {
    construction_marker_positions(record, int_width)?
        .into_iter()
        .find_map(|position| decode_curve_block(record, position, int_width))
}

/// Locate the final valid 3D curve cache at the stream's known integer width.
pub fn final_curve_patch_layout(record: &[u8], int_width: RefWidth) -> Option<CurvePatchLayout> {
    construction_marker_positions(record, int_width)?
        .into_iter()
        .filter_map(|position| decode_curve_block(record, position, int_width))
        .next_back()
}

/// Decode the unique well-formed surface cache across both integer widths.
///
/// This generic entry point has no stream-width, owning-scope, or family-role
/// witness. It therefore withholds when more than one `(width, marker)`
/// candidate decodes.
pub fn decode_surface_cache(record_bytes: &[u8]) -> Option<NurbsSurface> {
    decode_unique_cache(record_bytes, |bytes, position, width| {
        decode_surface_block(bytes, position, width).map(|candidate| candidate.surface)
    })
}

/// Decode the surface cache a subtype scope itself owns: the first surface
/// block outside every construction the scope nests. A scope whose supports are
/// nested constructions carries their caches too, and those are not its own.
pub(super) fn decode_owned_surface_cache_at(
    scope: subtypes::SubtypeScope<'_>,
    int_width: RefWidth,
) -> Option<NurbsSurface> {
    let bytes = scope.bytes();
    scope
        .owned_marker_positions(int_width)
        .into_iter()
        .find_map(|pos| decode_surface_block(bytes, pos, int_width).map(|decoded| decoded.surface))
}

/// Decode the unique well-formed 3D curve cache across both integer widths.
///
/// This generic entry point has no stream-width or owning-scope witness. It
/// therefore withholds when more than one `(width, marker)` candidate decodes.
pub fn decode_curve_cache(record_bytes: &[u8]) -> Option<NurbsCurve> {
    decode_unique_cache(record_bytes, |bytes, position, width| {
        decode_curve_block(bytes, position, width).map(|candidate| candidate.curve)
    })
}

/// Decode the 3D curve cache a subtype scope itself owns: the first curve block
/// outside every construction the scope nests.
pub fn decode_owned_curve_cache_at(
    scope: subtypes::SubtypeScope<'_>,
    int_width: RefWidth,
) -> Option<NurbsCurve> {
    let bytes = scope.bytes();
    scope
        .owned_marker_positions(int_width)
        .into_iter()
        .find_map(|pos| decode_curve_block(bytes, pos, int_width).map(|decoded| decoded.curve))
}

fn decode_unique_cache<T>(
    record_bytes: &[u8],
    decode: impl Fn(&[u8], usize, RefWidth) -> Option<T>,
) -> Option<T> {
    let positions = marker_positions(record_bytes);
    let mut decoded = None;
    for width in INT_WIDTHS {
        for &position in &positions {
            if let Some(candidate) = decode(record_bytes, position, width) {
                if decoded.is_some() {
                    return None;
                }
                decoded = Some(candidate);
            }
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::cache_from_subtype_refs;
    use crate::nurbs::toks::SubtypeTable;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    #[test]
    fn cacheless_surface_does_not_borrow_a_referenced_support_cache() {
        use crate::sab::{Record, Token};
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        let mut tokens = vec![
            Token::SubtypeOpen,
            Token::Ident("exact_spl_sur".into()),
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Long(1),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Long(2),
            Token::Long(2),
        ];
        for _ in 0..2 {
            tokens.extend([
                Token::Double(0.0),
                Token::Long(1),
                Token::Double(1.0),
                Token::Long(1),
            ]);
        }
        for point in [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]] {
            tokens.extend(point.map(Token::Double));
        }
        tokens.push(Token::SubtypeClose);
        let record = Record {
            index: 0,
            name: "spline-surface".into(),
            tokens: tokens.into(),
            offset: 0,
            len: 0,
        };
        let table = SubtypeTable::from_records(&ctx, &[record]).unwrap();
        for reference in [
            vec![
                Token::SubtypeOpen,
                Token::Ident("ref".into()),
                Token::Long(0),
                Token::SubtypeClose,
            ],
            vec![Token::SubtypeOpen, Token::Long(0), Token::SubtypeClose],
        ] {
            let scope = crate::nurbs::toks::subtype_span(&reference, 0).unwrap();
            assert!(
                super::owned_surface_cache_resolving_refs(&ctx, scope, &table)
                    .unwrap()
                    .is_ok()
            );
            let mut owner = vec![
                Token::SubtypeOpen,
                Token::Ident("srf_srf_v_bl_spl_sur".into()),
            ];
            owner.extend(reference);
            owner.push(Token::SubtypeClose);
            let scope = crate::nurbs::toks::subtype_span(&owner, 0).unwrap();
            assert!(super::owned_surface_cache_resolving_refs(&ctx, scope, &table).is_none());
        }
    }

    #[test]
    fn cached_nurbs_recovery_caps_preserve_resource_refusals() {
        use crate::sab::Token;
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
        let mut curve = vec![
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Enum(0),
            Token::Long(1_001),
        ];
        for index in 0_u32..1_001 {
            curve.extend([Token::Double(f64::from(index)), Token::Long(1)]);
        }
        curve.extend(std::iter::repeat_n(Token::Double(0.0), 1_001 * 3));
        let error = super::curve_block(&ctx, &curve, 0)
            .expect("ceiling is a recognized cache refusal")
            .expect_err("knot ceiling");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if matches!(limit.dimension, ResourceDimension::Codec("ASM unique knot recovery")))
        );
        let mut surface = vec![
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Long(1),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Long(2),
            Token::Long(1_001),
        ];
        for count in [2_u32, 1_001] {
            for index in 0..count {
                surface.extend([Token::Double(f64::from(index)), Token::Long(1)]);
            }
        }
        surface.extend(std::iter::repeat_n(Token::Double(0.0), 2 * 1_001 * 3));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
        let error = super::surface_block(&ctx, &surface, 0)
            .expect("ceiling is a recognized cache refusal")
            .expect_err("knot ceiling");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if matches!(limit.dimension, ResourceDimension::Codec("ASM unique knot recovery")))
        );
        let mut grid = vec![
            Token::Ident("nubs".into()),
            Token::Long(1),
            Token::Long(1),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Long(449),
            Token::Long(449),
        ];
        for _ in 0..2 {
            for index in 0_u32..449 {
                grid.extend([Token::Double(f64::from(index)), Token::Long(1)]);
            }
        }
        grid.extend(std::iter::repeat_n(Token::Double(0.0), 449 * 449 * 3));
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("root");
        let error = super::surface_block(&ctx, &grid, 0)
            .expect("pole ceiling is a recognized cache refusal")
            .expect_err("pole ceiling");
        assert!(
            matches!(error, CodecError::ResourceLimit(limit) if matches!(limit.dimension, ResourceDimension::Codec("ASM surface pole recovery")))
        );
    }

    #[test]
    fn subtype_search_stack_refuses_collection_limit() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let table = SubtypeTable::from_records(&ctx, &[]).unwrap();
        let error = cache_from_subtype_refs::<(), _>(&ctx, &[], &table, |_, _| None)
            .expect("stack allocation must refuse")
            .expect_err("stack allocation must refuse");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected collection refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    }

    #[test]
    fn subtype_search_visited_refuses_collection_limit() {
        use crate::sab::Token;

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let table = SubtypeTable::from_records(&ctx, &[]).unwrap();
        let tokens = [Token::SubtypeOpen, Token::Long(0), Token::SubtypeClose];
        let error = cache_from_subtype_refs::<(), _>(&ctx, &tokens, &table, |_, _| None)
            .expect("visited allocation must refuse")
            .expect_err("visited allocation must refuse");
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected collection refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    }
}
