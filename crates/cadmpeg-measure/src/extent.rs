// SPDX-License-Identifier: Apache-2.0
//! Axial range, angular sweep and cross-section radius range of cylinder
//! and cone faces.
//!
//! On a cylinder every `u`-isoline is a full line, and on a cone a ray from
//! the apex; a linear coordinate is monotone along each. So the extremes of
//! the axial coordinate over a face lie on its boundary or at a member apex,
//! and the face's projection onto the circle of directions is the union of
//! its boundary pieces' `u`-ranges. A loop that wraps the axis covers the
//! full circle.

use cadmpeg_ir::math::Point3;

use crate::bbox::curve_range;
use crate::boundary::piece::{Piece, Source};
use crate::boundary::{LoopKind, ResolvedFace};
use crate::canonical::surface::{Axis, Chart};
use crate::context::Eval;
use crate::failure::Measure;
use crate::quadrature::extrema;
use crate::region::Region;

/// The axial coordinate range of a face, in millimetres from the axis
/// origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AxialRange {
    pub(crate) min: f64,
    pub(crate) max: f64,
    pub(crate) numeric_error: f64,
}

/// The angular sweep of a face about the axis.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AngularSweep {
    pub(crate) value: f64,
    pub(crate) full: bool,
    /// Covered intervals, start in `[0, 2π)`, sorted by start.
    pub(crate) intervals: Vec<[f64; 2]>,
    pub(crate) numeric_error: f64,
}

/// The axial coordinate of `point`.
fn axial(axis: &Axis, point: Point3) -> f64 {
    point.vector_from(axis.origin).dot(axis.direction)
}

/// The axial range over every boundary piece and vertex loop, and the apex
/// when it is a member of the region.
pub(crate) fn axial_range(
    eval: &Eval<'_, '_>,
    axis: &Axis,
    resolved: &ResolvedFace<'_>,
    region: &Region,
) -> Measure<AxialRange> {
    let mut range = AxialRange {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        numeric_error: 0.0,
    };
    let mut include = |value: f64| {
        range.min = range.min.min(value);
        range.max = range.max.max(value);
    };
    let base = axis
        .origin
        .vector_from(Point3::new(0.0, 0.0, 0.0))
        .dot(axis.direction);
    let mut numeric_error: f64 = 0.0;
    for resolution in &resolved.loops {
        let resolved_loop = resolution.outcome.as_ref().map_err(Clone::clone)?;
        if let LoopKind::Vertex(point) = resolved_loop.kind {
            include(axial(axis, point));
        }
        for piece in &resolved_loop.pieces {
            match &piece.source {
                Source::Segment { point, .. } => include(axial(axis, *point)),
                Source::Curve(curve) => {
                    let found = curve_range(eval, curve, [piece.t0, piece.t1], axis.direction)?;
                    include(found.min - base);
                    include(found.max - base);
                    numeric_error = numeric_error.max(found.numeric_error);
                }
                Source::Pcurve { chart, .. } => {
                    let mut charge = |units| eval.charge(units);
                    let mut f = |t: f64| -> Measure<(f64, f64)> {
                        let sample = piece.sample(eval, chart, t)?;
                        Ok((axis.h * sample.uv[1], axis.h * sample.duv[1]))
                    };
                    let found = extrema(&mut charge, &mut f, piece.t0, piece.t1, &piece.breaks())?;
                    include(found.min);
                    include(found.max);
                    numeric_error = numeric_error.max(found.error);
                }
            }
        }
    }
    if let Some(apex) = region.apex {
        include(axial(axis, apex));
    }
    range.numeric_error = numeric_error;
    Ok(range)
}

/// The lifted range of parameter `axis` (0 for `u`, 1 for `v`) along one
/// piece.
fn piece_range(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    piece: &Piece<'_>,
    axis: usize,
) -> Measure<([f64; 2], f64)> {
    if piece.synthesized() {
        let start = piece.uv(eval, chart, piece.t0)?[axis];
        let end = piece.uv(eval, chart, piece.t1)?[axis];
        return Ok(([start.min(end), start.max(end)], 0.0));
    }
    let mut charge = |units| eval.charge(units);
    let mut f = |t: f64| -> Measure<(f64, f64)> {
        let sample = piece.sample(eval, chart, t)?;
        Ok((sample.uv[axis], sample.duv[axis]))
    };
    let found = extrema(&mut charge, &mut f, piece.t0, piece.t1, &piece.breaks())?;
    Ok(([found.min, found.max], found.error))
}

/// The parameter ranges of a face on a NURBS carrier: the domain when the
/// region reaches the domain's natural edges, else the range of the
/// boundary pieces.
pub(crate) fn parameter_ranges(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    resolved: &ResolvedFace<'_>,
    region: &Region,
) -> Measure<[AxialRange; 2]> {
    if let (Some(domain), true) = (chart.domain(), region.exterior != 0.0) {
        return Ok(domain.map(|[min, max]| AxialRange {
            min,
            max,
            numeric_error: 0.0,
        }));
    }
    let mut ranges = [0, 1].map(|_| AxialRange {
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        numeric_error: 0.0,
    });
    for resolution in &resolved.loops {
        let resolved_loop = resolution.outcome.as_ref().map_err(Clone::clone)?;
        for piece in &resolved_loop.pieces {
            for (axis, range) in ranges.iter_mut().enumerate() {
                let ([low, high], error) = piece_range(eval, chart, piece, axis)?;
                range.min = range.min.min(low);
                range.max = range.max.max(high);
                range.numeric_error = range.numeric_error.max(error);
            }
        }
    }
    Ok(ranges)
}

/// The angular sweep: the measure of the union, on the circle, of every
/// piece's lifted `u`-range. `tolerance` is the angular allowance.
pub(crate) fn angular_sweep(
    eval: &Eval<'_, '_>,
    chart: &Chart<'_>,
    resolved: &ResolvedFace<'_>,
    tolerance: f64,
) -> Measure<AngularSweep> {
    let period = std::f64::consts::TAU;
    let mut ranges = Vec::new();
    let mut numeric_error: f64 = 0.0;
    let mut wraps = false;
    for resolution in &resolved.loops {
        let resolved_loop = resolution.outcome.as_ref().map_err(Clone::clone)?;
        if resolved_loop.net_shift[0] != 0 {
            wraps = true;
        }
        for piece in &resolved_loop.pieces {
            let (range, error) = piece_range(eval, chart, piece, 0)?;
            ranges.push(range);
            numeric_error = numeric_error.max(error);
        }
    }
    if wraps {
        return Ok(full_sweep(period));
    }
    Ok(union_on_circle(
        &ranges,
        period,
        tolerance,
        2.0 * numeric_error,
    ))
}

fn full_sweep(period: f64) -> AngularSweep {
    AngularSweep {
        value: period,
        full: true,
        intervals: vec![[0.0, period]],
        numeric_error: 0.0,
    }
}

/// The union on a circle of `period` of real intervals, merged across gaps
/// up to `tolerance`.
fn union_on_circle(
    ranges: &[[f64; 2]],
    period: f64,
    tolerance: f64,
    numeric_error: f64,
) -> AngularSweep {
    let mut pieces: Vec<[f64; 2]> = Vec::new();
    for &[low, high] in ranges {
        let width = high - low;
        if width >= period - tolerance {
            return full_sweep(period);
        }
        let mut start = low.rem_euclid(period);
        if start > period - tolerance {
            start = 0.0;
        }
        let end = start + width;
        if end > period {
            pieces.push([start, period]);
            pieces.push([0.0, end - period]);
        } else {
            pieces.push([start, end]);
        }
    }
    pieces.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut merged: Vec<[f64; 2]> = Vec::new();
    for piece in pieces {
        match merged.last_mut() {
            Some(last) if piece[0] <= last[1] + tolerance => last[1] = last[1].max(piece[1]),
            _ => merged.push(piece),
        }
    }
    let (Some(first), Some(last)) = (merged.first().copied(), merged.last().copied()) else {
        return AngularSweep {
            value: 0.0,
            full: false,
            intervals: Vec::new(),
            numeric_error,
        };
    };
    let wrap_gap = period - last[1] + first[0];
    if merged.len() == 1 && wrap_gap <= tolerance {
        return full_sweep(period);
    }
    if merged.len() > 1 && wrap_gap <= tolerance {
        merged.remove(0);
        if let Some(last) = merged.last_mut() {
            last[1] = period + first[1];
        }
    }
    let value = merged.iter().map(|[start, end]| end - start).sum();
    AngularSweep {
        value,
        full: false,
        intervals: merged,
        numeric_error,
    }
}

#[cfg(test)]
mod tests {
    use super::union_on_circle;

    const PI: f64 = std::f64::consts::PI;
    const TAU: f64 = std::f64::consts::TAU;

    #[test]
    fn a_quarter_patch_sweeps_its_interval() {
        let sweep = union_on_circle(&[[0.0, PI / 2.0], [PI / 2.0, PI / 2.0]], TAU, 1.0e-9, 0.0);
        assert!(!sweep.full);
        assert_eq!(sweep.intervals, [[0.0, PI / 2.0]]);
        assert_eq!(sweep.value, PI / 2.0);
    }

    #[test]
    fn intervals_across_zero_merge_into_one() {
        let sweep = union_on_circle(&[[-PI / 4.0, PI / 4.0]], TAU, 1.0e-9, 0.0);
        assert!(!sweep.full);
        assert_eq!(sweep.intervals.len(), 1);
        let [start, end] = sweep.intervals[0];
        assert!((start - 7.0 * PI / 4.0).abs() < 1.0e-12);
        assert!((end - 9.0 * PI / 4.0).abs() < 1.0e-12);
        assert!((sweep.value - PI / 2.0).abs() < 1.0e-12);
    }

    #[test]
    fn covering_pieces_give_a_full_sweep() {
        let sweep = union_on_circle(&[[0.0, PI], [PI, TAU]], TAU, 1.0e-9, 0.0);
        assert!(sweep.full);
        assert_eq!(sweep.value, TAU);
        let sweep = union_on_circle(&[[0.0, PI], [-PI, 0.0]], TAU, 1.0e-9, 0.0);
        assert!(sweep.full);
    }

    #[test]
    fn separate_lunes_sum() {
        let sweep = union_on_circle(&[[0.0, 1.0], [2.0, 3.0]], TAU, 1.0e-9, 0.0);
        assert!(!sweep.full);
        assert_eq!(sweep.intervals, [[0.0, 1.0], [2.0, 3.0]]);
        assert_eq!(sweep.value, 2.0);
    }
}
