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
use cadmpeg_ir::geometry::{PcurveGeometry, PcurveNurbs};
use cadmpeg_ir::math::Point2;

/// The decoded payload of a 2D `nubs` or `nurbs` pcurve block.
#[derive(Clone)]
pub struct NurbsPcurve(PcurveNurbs);

impl NurbsPcurve {
    pub(crate) fn new(
        degree: u32,
        knots: Vec<f64>,
        control_points: Vec<Point2>,
        weights: Option<Vec<f64>>,
        periodic: bool,
    ) -> Option<Self> {
        PcurveNurbs::new(degree, knots, control_points, weights, periodic)
            .ok()
            .map(Self)
    }

    /// Polynomial degree.
    pub fn degree(&self) -> u32 {
        self.0.degree()
    }

    /// Expanded knot vector.
    pub fn knots(&self) -> &[f64] {
        self.0.knots()
    }

    pub(crate) fn knots_mut(&mut self) -> &mut [f64] {
        self.0.knots_mut()
    }

    /// Parameter-space control points.
    pub fn control_points(&self) -> &[Point2] {
        self.0.control_points()
    }

    pub(crate) fn control_points_mut(&mut self) -> &mut [Point2] {
        self.0.control_points_mut()
    }

    /// Optional rational weights.
    pub fn weights(&self) -> Option<&[f64]> {
        self.0.weights()
    }

    pub(crate) fn weights_mut(&mut self) -> Option<&mut [f64]> {
        self.0.weights_mut()
    }

    /// Convert the parsed cache to its cardinality-checked IR form.
    pub(crate) fn into_geometry(self) -> PcurveGeometry {
        PcurveGeometry::Nurbs { nurbs: self.0 }
    }
}

/// Writable value offsets for one 2D pcurve cache.
pub struct PcurvePatchLayout {
    /// Payload width of integer and enum fields.
    pub int_width: RefWidth,
    /// Tagged-integer payload offset for the curve degree.
    pub degree_value_offset: usize,
    /// Tagged-double payload offsets in `(u, v)` pole order.
    pub control_value_offsets: Vec<usize>,
    /// Tagged-double payload offsets for homogeneous weights.
    pub weight_value_offsets: Vec<usize>,
    /// Number of UV control points.
    pub control_count: usize,
    /// Native unique-knot payloads and expanded run lengths.
    pub knots: KnotLayout,
    /// Payload offset for the closure enum.
    pub periodic_value_offset: usize,
    /// Offset immediately after the final UV control component.
    pub control_end: usize,
}

/// Locate the final valid 2D pcurve block at the stream's known integer width.
pub fn final_pcurve_patch_layout(record: &[u8], int_width: RefWidth) -> Option<PcurvePatchLayout> {
    final_pcurve_patch_layout_at(record, int_width)
}

fn final_pcurve_patch_layout_at(record: &[u8], int_width: RefWidth) -> Option<PcurvePatchLayout> {
    construction_marker_positions(record, int_width)
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
                int_width,
                degree_value_offset,
                control_value_offsets: offsets,
                weight_value_offsets: weight_offsets,
                control_count,
                knots: knot_layout,
                periodic_value_offset,
                control_end: pos,
            })
        })
        .next_back()
}

fn decode_pcurve_block(b: &[u8], marker_pos: usize, int_width: RefWidth) -> Option<NurbsPcurve> {
    decode_pcurve_block_with_end(b, marker_pos, int_width).map(|(pcurve, _)| pcurve)
}

pub(crate) fn decode_pcurve_block_with_end(
    b: &[u8],
    marker_pos: usize,
    int_width: RefWidth,
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
        let u = View::f64_le_at(b, pos + 1)?;
        pos += 9;
        if *b.get(pos)? != 0x06 {
            return None;
        }
        let v = View::f64_le_at(b, pos + 1)?;
        pos += 9;
        control_points.push(Point2::new(u, v));
        if let Some(weights) = weights.as_mut() {
            if *b.get(pos)? != 0x06 {
                return None;
            }
            weights.push(View::f64_le_at(b, pos + 1)?);
            pos += 9;
        }
    }
    Some((
        NurbsPcurve::new(
            degree as u32,
            knots,
            control_points,
            weights,
            is_periodic(closure),
        )?,
        pos,
    ))
}

/// Decode the unique well-formed 2D `nubs` block across both integer widths.
///
/// This generic entry point has no stream-width or owning-scope witness. It
/// therefore withholds when more than one `(width, marker)` candidate decodes.
pub fn decode_pcurve_cache(record_bytes: &[u8]) -> Option<NurbsPcurve> {
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
pub(crate) fn pcurve_block_with_end(
    toks: &[Token],
    marker_pos: usize,
) -> Option<(NurbsPcurve, usize)> {
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
    let (knots, n_poles) = toks::take_knot_table(&mut cur, n_uniq as usize, degree)?;
    let mut control_points = Vec::new();
    let mut weights = rational.then(Vec::new);
    for _ in 0..n_poles {
        let u = cur.take_f64()?;
        let v = cur.take_f64()?;
        control_points.push(Point2::new(u, v));
        if let Some(weights) = weights.as_mut() {
            weights.push(cur.take_f64()?);
        }
    }
    Some((
        NurbsPcurve::new(
            degree as u32,
            knots,
            control_points,
            weights,
            is_periodic(closure),
        )?,
        cur.pos(),
    ))
}

fn pcurve_block(toks: &[Token], marker_pos: usize) -> Option<NurbsPcurve> {
    pcurve_block_with_end(toks, marker_pos).map(|(pcurve, _)| pcurve)
}

/// Decode the BS2 field owned directly by an `exp_par_cur` scope.
///
/// The scope grammar makes its first owned B-spline block the pcurve. Nested
/// support references are not searched because they belong to other fields.
pub fn explicit_pcurve_cache(toks: &[Token]) -> Option<NurbsPcurve> {
    let position = toks::owned_marker_positions(toks).into_iter().next()?;
    pcurve_block(toks, position)
}

/// Resolve an explicit pcurve through one subtype-table reference.
pub fn explicit_pcurve_cache_from_subtype_ref(
    index: i64,
    table: &toks::SubtypeTable,
) -> Option<NurbsPcurve> {
    let index = usize::try_from(index).ok()?;
    explicit_pcurve_cache(table.span(index)?)
}

/// The parameter-space fit tolerance immediately following the final valid 2D
/// pcurve block in `toks`. Token-space counterpart of
/// [`decode_pcurve_fit_tolerance`].
pub fn pcurve_fit_tolerance(toks: &[Token]) -> Option<f64> {
    let scope = toks::owned_cache_scope(toks).unwrap_or(toks);
    let (_, end) = toks::owned_marker_positions(scope)
        .into_iter()
        .filter_map(|pos| pcurve_block_with_end(scope, pos))
        .next_back()?;
    match scope.get(end) {
        Some(Token::Double(value)) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
