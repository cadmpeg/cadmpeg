// SPDX-License-Identifier: Apache-2.0
//! Cached parameter-space curve (pcurve) block decoding, patch layouts, and cache entry points.

use crate::kernel_header::RefWidth;
use crate::nurbs::reader::{
    construction_marker_positions, is_periodic, marker_at, marker_positions, read_knots,
    take_tagged_int, KnotLayout, INT_WIDTHS,
};
use crate::nurbs::toks::{self, Cur};
use crate::sab::Token;
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles, WeightedPole2};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::NonZeroReal;
use cadmpeg_ir::units::FinitePoint2;

macro_rules! propagate_resource {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Some(Err(error)),
        }
    };
}

/// Writable value offsets for one 2D pcurve cache.
pub struct PcurvePatchLayout {
    /// Tagged-integer payload offset for the curve degree.
    pub degree_value_offset: usize,
    control_start: usize,
    rational: bool,
    /// Number of UV control points.
    pub control_count: usize,
    /// Native unique-knot payloads and expanded run lengths.
    pub knots: KnotLayout,
    /// Payload offset for the closure enum.
    pub periodic_value_offset: usize,
}

impl PcurvePatchLayout {
    fn control_stride(&self) -> usize {
        if self.rational {
            27
        } else {
            18
        }
    }

    /// Whether each native pole carries a homogeneous weight.
    pub fn rational(&self) -> bool {
        self.rational
    }

    /// Tagged-double payload offsets in `(u, v)` pole order.
    pub fn control_value_offsets(&self) -> impl ExactSizeIterator<Item = [usize; 2]> + '_ {
        (0..self.control_count).map(|ordinal| {
            let u = self.control_start + ordinal * self.control_stride() + 1;
            [u, u + 9]
        })
    }

    /// Tagged-double payload offsets for homogeneous weights.
    pub fn weight_value_offsets(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        (0..if self.rational { self.control_count } else { 0 })
            .map(|ordinal| self.control_start + ordinal * 27 + 19)
    }

    /// Offset immediately after the final control component.
    pub fn control_end(&self) -> usize {
        self.control_start + self.control_count * self.control_stride()
    }
}

/// Locate the final valid 2D pcurve block at the stream's known integer width.
pub fn final_pcurve_patch_layout(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record: &[u8],
    int_width: RefWidth,
) -> Result<Option<PcurvePatchLayout>, cadmpeg_core::CodecError> {
    let (positions, _marker_storage) = ctx.with_scoped_storage("ASM pcurve patch marker positions", || construction_marker_positions(ctx, record, int_width))?;
    let Some(positions) = positions else {
        return Ok(None);
    };
    ctx.find_map(positions.into_iter().rev(), |marker_pos| {
        (|| -> Option<Result<PcurvePatchLayout, cadmpeg_core::CodecError>> {
            let marker = marker_at(record, marker_pos)?;
            let rational = marker.rational();
            let mut pos = marker_pos + marker.byte_len();
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
            let (_knots, control_count, knot_layout) = read_knots(
                record,
                &mut pos,
                usize::try_from(unique).ok()?,
                degree,
                int_width,
            )?;
            let control_start = pos;
            let components = if rational { 3 } else { 2 };
            let mut visits = 0..control_count * components;
            while propagate_resource!(ctx.next_charged(&mut visits, "ASM pcurve patch control components")).is_some() {
                if record.get(pos) != Some(&0x06) {
                    return None;
                }
                pos += 9;
            }
            Some(Ok(PcurvePatchLayout {
                degree_value_offset,
                control_start,
                rational,
                control_count,
                knots: knot_layout,
                periodic_value_offset,
            }))
        })().transpose()
    }, "ASM pcurve patch candidates")
}

fn decode_pcurve_block(b: &[u8], marker_pos: usize, int_width: RefWidth) -> Option<PcurveNurbs> {
    decode_pcurve_block_with_end(b, marker_pos, int_width).map(|(pcurve, _)| pcurve)
}

pub(super) fn decode_pcurve_block_with_end(
    b: &[u8],
    marker_pos: usize,
    int_width: RefWidth,
) -> Option<(PcurveNurbs, usize)> {
    // Byte-addressed inspection supplies writer patch layouts under its own policy.
    let writer_arena = cadmpeg_core::decode::DecodeArena::new();
    let writer_policy = cadmpeg_core::decode::DecodePolicy::desktop();
    let (writer_ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &writer_arena, &writer_policy)
            .ok()?;
    let marker = marker_at(b, marker_pos)?;
    let rational = marker.rational();
    let mut pos = marker_pos + marker.byte_len();
    let degree = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=20).contains(&degree) {
        return None;
    }
    let closure = take_tagged_int(b, &mut pos, 0x15, int_width)?;
    let n_uniq = take_tagged_int(b, &mut pos, 0x04, int_width)?;
    if !(1..=1000).contains(&n_uniq) {
        return None;
    }
    let (knots, n_poles, _knot_layout) = read_knots(
        b,
        &mut pos,
        usize::try_from(n_uniq).ok()?,
        degree,
        int_width,
    )?;
    // The record states a pole and its weight together, so the reader states
    // rows: there is no pole lane and no weight lane for a reader to pair.
    let mut points = Vec::new();
    let mut weighted = Vec::new();
    for _ in 0..n_poles {
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let u = View::f64_le_at(b, pos + 1)?;
        pos += 9;
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let v = View::f64_le_at(b, pos + 1)?;
        pos += 9;
        let point = Point2::new(u, v);
        if rational {
            if *b.get(pos)? != 0x06 {
                return None;
            }
            let weight = View::f64_le_at(b, pos + 1)?;
            pos += 9;
            weighted.push(WeightedPole2 {
                point,
                weight: NonZeroReal::new(weight)?,
            });
        } else {
            points.push(point);
        }
    }
    let poles = if rational {
        PcurveNurbsPoles::Rational { points: weighted }
    } else {
        PcurveNurbsPoles::Polynomial { points }
    };
    Some((
        PcurveNurbs::new(
            &writer_ctx,
            u32::try_from(degree).ok()?,
            knots,
            poles,
            is_periodic(closure),
        )
        .ok()?
        .ok()?,
        pos,
    ))
}

/// Decode the unique well-formed 2D `nubs` block across both integer widths.
///
/// This generic entry point has no stream-width or owning-scope witness. It
/// therefore withholds when more than one `(width, marker)` candidate decodes.
pub fn decode_pcurve_cache(record_bytes: &[u8]) -> Option<PcurveNurbs> {
    let mut decoded = None;
    for int_width in INT_WIDTHS {
        for position in marker_positions(record_bytes) {
            if let Some(candidate) = decode_pcurve_block(record_bytes, position, int_width) {
                if decoded.is_some() {
                    return None;
                }
                decoded = Some(candidate);
            }
        }
    }
    decoded
}

/// Decode a 2D `nubs`/`nurbs` pcurve block at token `marker_pos`, returning
/// the pcurve and the token index just past the block. Token-space counterpart
/// of [`decode_pcurve_block_with_end`].
pub(super) fn pcurve_block_with_end(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    marker_pos: usize,
) -> Option<Result<(PcurveNurbs, usize), cadmpeg_core::CodecError>> {
    let rational = toks::marker_at(toks, marker_pos)?.rational();
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
    let (knots, n_poles) =
        match toks::take_knot_table(ctx, &mut cur, usize::try_from(n_uniq).ok()?, degree)? {
            Ok(knots) => knots,
            Err(error) => return Some(Err(error)),
        };
    let mut points = Vec::new();
    let mut weighted = Vec::new();
    if rational {
        weighted = match ctx.collection_vec(n_poles, "ASM rational pcurve poles") {
            Ok(weighted) => weighted,
            Err(error) => return Some(Err(error)),
        };
    } else {
        points = match ctx.collection_vec(n_poles, "ASM polynomial pcurve poles") {
            Ok(points) => points,
            Err(error) => return Some(Err(error)),
        };
    }
    let mut visits = 0..n_poles;
    while propagate_resource!(ctx.next_charged(&mut visits, "ASM pcurve block with end entries")).is_some() {
        let u = cur.take_f64()?;
        let v = cur.take_f64()?;
        let point = FinitePoint2::new(Point2::new(u, v))?;
        if rational {
            weighted.push(WeightedPole2 {
                point,
                weight: NonZeroReal::new(cur.take_f64()?)?,
            });
        } else {
            points.push(point);
        }
    }
    let poles = if rational {
        PcurveNurbsPoles::Rational { points: weighted }
    } else {
        PcurveNurbsPoles::Polynomial { points }
    };
    Some(Ok((
        propagate_resource!(PcurveNurbs::new(
            ctx,
            u32::try_from(degree).ok()?,
            knots,
            poles,
            is_periodic(closure),
        ))
        .ok()?,
        cur.pos(),
    )))
}

fn pcurve_block(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    toks: &[Token],
    marker_pos: usize,
) -> Option<Result<PcurveNurbs, cadmpeg_core::CodecError>> {
    pcurve_block_with_end(ctx, toks, marker_pos).map(|result| result.map(|(pcurve, _)| pcurve))
}

/// Decode the BS2 field owned directly by an `exp_par_cur` scope.
///
/// The scope grammar makes its first owned B-spline block the pcurve. Nested
/// support references are not searched because they belong to other fields.
///
/// The argument is the scope, so the marker walk is total: the unbalanced
/// stream the free walk refuses is a state [`toks::SubtypeScope`] cannot hold.
/// Marker positions index the scope's own tokens.
pub fn explicit_pcurve_cache(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
) -> Option<Result<PcurveNurbs, cadmpeg_core::CodecError>> {
    let position = {
        let (positions, _marker_storage) = propagate_resource!(ctx.with_scoped_storage("ASM explicit pcurve marker positions", || scope.owned_marker_positions(ctx)));
        positions.into_iter().next()?
    };
    pcurve_block(ctx, scope.tokens(), position)
}

/// Resolve an explicit pcurve through one subtype-table reference.
pub fn explicit_pcurve_cache_from_subtype_ref(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    index: i64,
    table: &toks::SubtypeTable,
) -> Option<Result<PcurveNurbs, cadmpeg_core::CodecError>> {
    let index = usize::try_from(index).ok()?;
    explicit_pcurve_cache(ctx, table.span(index)?)
}

/// The parameter-space fit tolerance immediately following the final valid 2D
/// pcurve block the scope itself owns.
///
/// The blocks searched are the ones [`explicit_pcurve_cache`] selects from, so
/// the tolerance belongs to the pcurve that function returns. Nested support
/// references are not searched because they belong to other fields.
///
/// The argument is the scope, so the marker walk is total: the unbalanced
/// stream the free walk refuses is a state [`toks::SubtypeScope`] cannot hold.
/// Marker positions index the scope's own tokens.
pub fn pcurve_fit_tolerance(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: toks::SubtypeScope<'_>,
) -> Option<Result<f64, cadmpeg_core::CodecError>> {
    let tokens = scope.tokens();
    let end = {
        let (positions, _marker_storage) = propagate_resource!(ctx.with_scoped_storage("ASM pcurve tolerance marker positions", || scope.owned_marker_positions(ctx)));
        let (decoded, _cache_storage) = propagate_resource!(ctx.with_scoped_storage("ASM pcurve tolerance cache", || ctx.find_map(positions.into_iter().rev(), |pos| pcurve_block_with_end(ctx, tokens, pos).transpose(), "ASM pcurve tolerance candidates")));
        decoded?.1
    };
    match tokens.get(end) {
        Some(Token::Double(value)) => Some(Ok(*value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
