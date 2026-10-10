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
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::nurbs::{
    NurbsCurve, NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis, WeightedPole3,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::NonZeroReal;

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
    let mut visits = 0..count;
    while (!visits.is_empty() || ctx.resource_refusal().is_some())
        && propagate_resource!(ctx.next_charged(&mut visits, "ASM control points entries"))
            .is_some()
    {
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
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
    let grid = if marker.rational() {
        let mut rows = propagate_resource!(ctx.collect_indexed_vec(
            n_poles_u,
            "ASM NURBS grid rows",
            |_| ctx.collection_vec(n_poles_v, "ASM NURBS grid row poles"),
        ));
        let mut columns = 0..n_poles_v;
        while (!columns.is_empty() || ctx.resource_refusal().is_some())
            && propagate_resource!(ctx.next_charged(&mut columns, "ASM surface control columns"))
                .is_some()
        {
            let mut rows = rows.iter_mut();
            while rows.len() != 0 || ctx.resource_refusal().is_some() {
                let Some(row) =
                    propagate_resource!(ctx.next_charged(&mut rows, "ASM surface control points"))
                else {
                    break;
                };
                let point = FinitePoint3::new(Point3::new(
                    cur.take_f64()? * LEN_TO_MM,
                    cur.take_f64()? * LEN_TO_MM,
                    cur.take_f64()? * LEN_TO_MM,
                ))?;
                row.push(WeightedPole3 {
                    point,
                    weight: NonZeroReal::new(cur.take_f64()?)?,
                });
            }
        }
        NurbsPoleGrid::Rational { rows }
    } else {
        let mut rows = propagate_resource!(ctx.collect_indexed_vec(
            n_poles_u,
            "ASM NURBS grid rows",
            |_| ctx.collection_vec(n_poles_v, "ASM NURBS grid row poles"),
        ));
        let mut columns = 0..n_poles_v;
        while (!columns.is_empty() || ctx.resource_refusal().is_some())
            && propagate_resource!(ctx.next_charged(&mut columns, "ASM surface control columns"))
                .is_some()
        {
            let mut rows = rows.iter_mut();
            while rows.len() != 0 || ctx.resource_refusal().is_some() {
                let Some(row) =
                    propagate_resource!(ctx.next_charged(&mut rows, "ASM surface control points"))
                else {
                    break;
                };
                row.push(FinitePoint3::new(Point3::new(
                    cur.take_f64()? * LEN_TO_MM,
                    cur.take_f64()? * LEN_TO_MM,
                    cur.take_f64()? * LEN_TO_MM,
                ))?);
            }
        }
        NurbsPoleGrid::Polynomial { rows }
    };
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
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
    let (poles, _pole_storage) = propagate_resource!(ctx.with_scoped_storage(
        "ASM raw curve poles",
        || control_points(ctx, &mut cur, n_poles, marker).transpose()
    ));
    let poles = poles?;

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
    let (positions, _marker_storage) = propagate_resource!(ctx
        .with_scoped_storage("ASM cache marker positions", || {
            toks::owned_marker_positions(ctx, scope).transpose()
        }));
    let positions = positions?;
    let compound =
        propagate_resource!(ctx.any_by(scope, |token| Ok(
        matches!(token, Token::Ident(name) | Token::SubIdent(name) if name == "comp_spl_sur")
    ), "ASM compound surface cache search"));
    let decode = |pos| {
        let (candidate, storage) = ctx
            .with_scoped_storage("ASM surface cache candidate", || {
                surface_block(ctx, scope, pos).transpose()
            })?;
        match candidate {
            Some((surface, _)) => storage.commit_value(surface).map(Some),
            None => Ok(None),
        }
    };
    let found = if compound {
        propagate_resource!(ctx.find_map(positions, decode, "ASM surface cache candidates"))
    } else {
        propagate_resource!(ctx.find_map(
            positions.into_iter().rev(),
            decode,
            "ASM surface cache candidates"
        ))
    };
    found.map(Ok)
}

/// Decode the surface cache a subtype scope itself owns: the first surface
/// block outside every construction the scope nests.
pub(super) fn owned_surface_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
) -> Option<Result<NurbsSurface, cadmpeg_core::CodecError>> {
    let tokens = scope.tokens();
    let (positions, _marker_storage) = propagate_resource!(ctx
        .with_scoped_storage("ASM cache marker positions", || scope
            .owned_marker_positions(ctx)));
    propagate_resource!(ctx.find_map(
        positions,
        |pos| {
            let (candidate, storage) = ctx
                .with_scoped_storage("ASM owned surface cache candidate", || {
                    surface_block(ctx, tokens, pos).transpose()
                })?;
            match candidate {
                Some((cache, _)) => storage.commit_value(cache).map(Some),
                None => Ok(None),
            }
        },
        "ASM owned_surface_cache candidates"
    ))
    .map(Ok)
}

/// Decode the 3D curve cache of a procedural curve record from its payload
/// tokens: the FIRST valid curve block (surface and 2D pcurve blocks do not
/// parse as a 3D curve block).
pub(super) fn curve_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
) -> Option<Result<NurbsCurve, cadmpeg_core::CodecError>> {
    let scope = propagate_resource!(toks::cache_scope(ctx, toks)?);
    let (positions, _marker_storage) = propagate_resource!(ctx
        .with_scoped_storage("ASM cache marker positions", || {
            toks::owned_marker_positions(ctx, scope).transpose()
        }));
    let positions = positions?;
    propagate_resource!(ctx.find_map(
        positions,
        |pos| {
            let (candidate, storage) = ctx
                .with_scoped_storage("ASM curve cache candidate", || {
                    curve_block(ctx, scope, pos).transpose()
                })?;
            match candidate {
                Some((cache, _)) => storage.commit_value(cache).map(Some),
                None => Ok(None),
            }
        },
        "ASM curve_cache candidates"
    ))
    .map(Ok)
}

/// Decode the 3D curve cache a subtype scope itself owns: the first curve
/// block outside every construction the scope nests.
pub(super) fn owned_curve_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
) -> Option<Result<NurbsCurve, cadmpeg_core::CodecError>> {
    let tokens = scope.tokens();
    let (positions, _marker_storage) = propagate_resource!(ctx
        .with_scoped_storage("ASM cache marker positions", || scope
            .owned_marker_positions(ctx)));
    propagate_resource!(ctx.find_map(
        positions,
        |pos| {
            let (candidate, storage) = ctx
                .with_scoped_storage("ASM owned curve cache candidate", || {
                    curve_block(ctx, tokens, pos).transpose()
                })?;
            match candidate {
                Some((cache, _)) => storage.commit_value(cache).map(Some),
                None => Ok(None),
            }
        },
        "ASM owned_curve_cache candidates"
    ))
    .map(Ok)
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Some(Err(refusal.into()));
    }
    (|| -> Result<Option<T>, cadmpeg_core::CodecError> {
        if toks.is_empty() {
            return Ok(None);
        }
        let mut visited_storage = ctx.reserve_scoped(0, "ASM subtype search visited")?;
        let mut seen = std::collections::HashSet::new();
        let mut pending_storage;
        let (mut pending, result_pending_storage) =
            ctx.temporary_vec(1, "ASM subtype search stack")?;
        pending_storage = result_pending_storage;
        pending.push((toks, 0, ctx.enter_nested("resolve ASM cache search root")?));
        while let Some((tokens, position, _guard)) = pending.last_mut() {
            let Some(index) = subtypes::next_subtype_reference(ctx, tokens, position)? else {
                pending.pop();
                continue;
            };
            if !visited_storage.with_storage(|| {
                ctx.insert_hash_set(&mut seen, index, "ASM subtype search visited")
            })? {
                continue;
            }
            let Some(target) = table.span(index) else {
                return Ok(None);
            };
            let guard = ctx.enter_nested("resolve ASM cache reference")?;
            if let Some(decoded) = decode_scope(ctx, target) {
                return decoded.map(Some);
            }
            let frame = (target.tokens(), 0, guard);
            ctx.push_scoped_vec(
                &mut pending_storage,
                &mut pending,
                frame,
                "ASM subtype search stack",
            )?;
        }
        Ok(None)
    })()
    .transpose()
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

/// [`owned_surface_cache`], following subtype-table references.
pub(super) fn owned_surface_cache_resolving_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
    table: &toks::SubtypeTable,
) -> Option<Result<NurbsSurface, cadmpeg_core::CodecError>> {
    owned_surface_cache(ctx, scope)
        .or_else(|| cache_from_subtype_refs(ctx, scope.tokens(), table, owned_surface_cache))
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
        let components = if matches!(self.surface.pole_grid(), NurbsPoleGrid::Rational { .. }) {
            4
        } else {
            3
        };
        (0..self.surface.u_count() * self.surface.v_count() * components)
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &[u8],
    int_width: RefWidth,
) -> Result<Option<SurfacePatchLayout>, cadmpeg_core::CodecError> {
    let (positions, _marker_storage) = ctx
        .with_scoped_storage("ASM patch marker positions", || {
            construction_marker_positions(ctx, record, int_width)
        })?;
    let Some(positions) = positions else {
        return Ok(None);
    };
    ctx.find_map(
        positions.into_iter().rev(),
        |position| Ok(decode_surface_block(record, position, int_width)),
        "ASM final surface patch candidates",
    )
}

/// Locate the surface block at `ordinal` among valid surface caches at the
/// stream's known integer width.
pub fn surface_patch_layout_at(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &[u8],
    ordinal: usize,
    int_width: RefWidth,
) -> Result<Option<SurfacePatchLayout>, cadmpeg_core::CodecError> {
    let (positions, _marker_storage) = ctx
        .with_scoped_storage("ASM patch marker positions", || {
            construction_marker_positions(ctx, record, int_width)
        })?;
    let Some(positions) = positions else {
        return Ok(None);
    };
    let mut valid = 0;
    ctx.find_map(
        positions,
        |position| {
            let Some(candidate) = decode_surface_block(record, position, int_width) else {
                return Ok(None);
            };
            if valid == ordinal {
                return Ok(Some(candidate));
            }
            valid += 1;
            Ok(None)
        },
        "ASM surface patch ordinal candidates",
    )
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
        let components = if matches!(
            self.curve.pole_rows(),
            cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { .. }
        ) {
            4
        } else {
            3
        };
        (0..self.curve.pole_count() * components)
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
pub fn first_curve_patch_layout(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &[u8],
    int_width: RefWidth,
) -> Result<Option<CurvePatchLayout>, cadmpeg_core::CodecError> {
    let (positions, _marker_storage) = ctx
        .with_scoped_storage("ASM patch marker positions", || {
            construction_marker_positions(ctx, record, int_width)
        })?;
    let Some(positions) = positions else {
        return Ok(None);
    };
    ctx.find_map(
        positions,
        |position| Ok(decode_curve_block(record, position, int_width)),
        "ASM first curve patch candidates",
    )
}

/// Locate the final valid 3D curve cache at the stream's known integer width.
pub fn final_curve_patch_layout(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &[u8],
    int_width: RefWidth,
) -> Result<Option<CurvePatchLayout>, cadmpeg_core::CodecError> {
    let (positions, _marker_storage) = ctx
        .with_scoped_storage("ASM patch marker positions", || {
            construction_marker_positions(ctx, record, int_width)
        })?;
    let Some(positions) = positions else {
        return Ok(None);
    };
    ctx.find_map(
        positions.into_iter().rev(),
        |position| Ok(decode_curve_block(record, position, int_width)),
        "ASM final curve patch candidates",
    )
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: subtypes::SubtypeScope<'_>,
    int_width: RefWidth,
) -> Option<Result<NurbsSurface, cadmpeg_core::CodecError>> {
    let bytes = scope.bytes();
    let (positions, _marker_storage) = match ctx
        .with_scoped_storage("ASM byte cache marker positions", || {
            scope.owned_marker_positions(ctx, int_width)
        }) {
        Ok(positions) => positions,
        Err(error) => return Some(Err(error)),
    };
    propagate_resource!(ctx.find_map(
        positions,
        |pos| Ok(decode_surface_block(bytes, pos, int_width).map(|decoded| decoded.surface)),
        "ASM owned surface byte cache candidates"
    ))
    .map(Ok)
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: subtypes::SubtypeScope<'_>,
    int_width: RefWidth,
) -> Option<Result<NurbsCurve, cadmpeg_core::CodecError>> {
    let bytes = scope.bytes();
    let (positions, _marker_storage) = match ctx
        .with_scoped_storage("ASM byte cache marker positions", || {
            scope.owned_marker_positions(ctx, int_width)
        }) {
        Ok(positions) => positions,
        Err(error) => return Some(Err(error)),
    };
    propagate_resource!(ctx.find_map(
        positions,
        |pos| Ok(decode_curve_block(bytes, pos, int_width).map(|decoded| decoded.curve)),
        "ASM owned curve byte cache candidates"
    ))
    .map(Ok)
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

    fn rectangular_surface_tokens(rational: bool) -> Vec<crate::sab::Token> {
        use crate::sab::Token;
        let mut tokens = vec![
            Token::Ident(if rational { "nurbs" } else { "nubs" }.into()),
            Token::Long(1),
            Token::Long(1),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Enum(0),
            Token::Long(2),
            Token::Long(3),
            Token::Double(0.0),
            Token::Long(1),
            Token::Double(1.0),
            Token::Long(1),
            Token::Double(0.0),
            Token::Long(1),
            Token::Double(1.0),
            Token::Long(1),
            Token::Double(2.0),
            Token::Long(1),
        ];
        for ordinal in 0..6 {
            tokens.extend([
                Token::Double(f64::from(ordinal)),
                Token::Double(0.0),
                Token::Double(0.0),
            ]);
            if rational {
                tokens.push(Token::Double(f64::from(ordinal + 1)));
            }
        }
        tokens
    }

    #[test]
    fn surface_control_grid_preserves_rectangular_stream_order_and_weights() {
        for rational in [false, true] {
            let ctx = cadmpeg_test_support::service_decode_context();
            let tokens = rectangular_surface_tokens(rational);
            let (surface, end) = super::surface_block(&ctx, &tokens, 0).unwrap().unwrap();
            assert_eq!(end, tokens.len());
            assert_eq!((surface.u_count(), surface.v_count()), (2, 3));
            for u in 0..2 {
                for v in 0..3 {
                    assert_eq!(
                        surface.pole(u, v).unwrap().x,
                        f64::from(u32::try_from(v * 2 + u).unwrap()) * 10.0
                    );
                    assert_eq!(
                        surface
                            .weight(u, v)
                            .map(cadmpeg_ir::scalar::NonZeroReal::get),
                        rational.then_some(f64::from(u32::try_from(v * 2 + u + 1).unwrap()))
                    );
                }
            }
        }
    }

    #[test]
    fn surface_control_grid_refuses_before_row_allocation() {
        for rational in [false, true] {
            let tokens = rectangular_surface_tokens(rational);
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::CollectionItems,
                "ASM NURBS grid row poles",
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_collection_items = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    super::surface_block(&ctx, &tokens, 0).unwrap()
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(_)));
        }
    }

    #[test]
    fn invalid_first_surface_control_skips_unvisited_rows() {
        use crate::sab::Token;
        for (rational, invalid_weight) in [(false, false), (true, false), (true, true)] {
            let mut tokens = rectangular_surface_tokens(rational);
            if invalid_weight {
                tokens[22] = Token::Double(0.0);
            } else {
                tokens[19] = Token::Double(f64::NAN);
            }
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::WorkUnits,
                "ASM surface control points",
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_work_units = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    super::surface_block(&ctx, &tokens, 0)
                        .transpose()
                        .map(|_| ())
                },
            );
            let CodecError::ResourceLimit(limit) = error else {
                panic!("expected first-control work refusal");
            };
            assert_eq!(limit.additional, 1);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            // Admit the first control step. Its invalid value ends the block,
            // so the second row and later columns need no work allowance.
            policy.limits.max_work_units = limit.used + 1;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(super::surface_block(&ctx, &tokens, 0).is_none());
            ctx.finish_session().unwrap();
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
    fn subtype_search_refuses_work_before_reference_probe() {
        use crate::sab::Token;

        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "ASM subtype reference search",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let table = SubtypeTable::from_records(&ctx, &[]).unwrap();
                let tokens = [Token::SubtypeOpen, Token::Long(0), Token::SubtypeClose];
                cache_from_subtype_refs::<(), _>(&ctx, &tokens, &table, |_, _| None)
                    .expect("recognized refusal route")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected work refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "ASM subtype reference search");
    }

    #[test]
    fn empty_subtype_search_has_no_scratch_allocation() {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let table = SubtypeTable::from_records(&ctx, &[]).unwrap();
        assert!(cache_from_subtype_refs::<(), _>(&ctx, &[], &table, |_, _| None).is_none());
        ctx.finish_session().unwrap();
    }

    #[test]
    fn subtype_search_refuses_before_entering_a_nested_reference() {
        use crate::sab::Token;
        let table_ctx = cadmpeg_test_support::service_decode_context();
        let record = crate::test_support::sab::record(
            0,
            "spline".into(),
            vec![
                Token::SubtypeOpen,
                Token::Ident("construction".into()),
                Token::SubtypeClose,
            ]
            .into(),
            0,
            0,
        );
        let table = SubtypeTable::from_records(&table_ctx, &[record]).unwrap();
        let tokens = [Token::SubtypeOpen, Token::Long(0), Token::SubtypeClose];
        for returns_cache in [false, true] {
            let error = cadmpeg_test_support::refusal::resource_limit_at(
                ResourceDimension::RecursionDepth,
                "resolve ASM cache reference",
                |cap| {
                    let arena = DecodeArena::new();
                    let mut policy = DecodePolicy::service();
                    policy.limits.max_recursion_depth = cap;
                    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
                    cache_from_subtype_refs::<(), _>(&ctx, &tokens, &table, |_, _| {
                        returns_cache.then_some(Ok(()))
                    })
                    .transpose()
                },
            );
            assert!(matches!(error, CodecError::ResourceLimit(_)));
        }
    }

    #[test]
    fn subtype_search_visited_refuses_collection_limit() {
        use crate::sab::Token;

        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::CollectionItems,
            "ASM subtype search visited",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let table = SubtypeTable::from_records(&ctx, &[]).unwrap();
                let tokens = [Token::SubtypeOpen, Token::Long(0), Token::SubtypeClose];
                cache_from_subtype_refs::<(), _>(&ctx, &tokens, &table, |_, _| None)
                    .expect("visited allocation must refuse")
            },
        );
        let CodecError::ResourceLimit(limit) = error else {
            panic!("expected collection refusal: {error:?}");
        };
        assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    }

    mod attempt_storage;
    mod entry_refusal;
}
