// SPDX-License-Identifier: Apache-2.0
//! Regions on closed domains: spheres and ring tori.
//!
//! - A face with no ring loop, or whose ring loops hold only seam pieces
//!   and synthesized pieces, covers the whole carrier (`whole_surface`).
//! - Otherwise the loops alone do not choose a side, because both sides are
//!   bounded. The region lies on the left of every loop after each loop is
//!   reversed when the declared normal is reversed (`declared_orientation`).
//!
//! The domain is the strip `u ∈ [0, 2π)` by `v ∈ [base, base + span]`. On a
//! sphere `base = −π/2` and both ends are poles. A torus is cut at the level
//! `base` midway in the widest gap of the loops' `v` values; each piece is
//! split where it crosses the cut and its `v` is taken in
//! `[base, base + 2π)`. Green's theorem on the strip adds the parts of the
//! upper edge in the region to the loops' integrals: on a sphere the whole
//! edge when the region reaches the north pole beyond every boundary point;
//! on a torus the cut-circle intervals that the crossing directions bound. A
//! crossing in `−v` starts an interval in `+u`, a crossing in `+v` ends it.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use cadmpeg_ir::topology::Face;

use super::lifts;
use super::membership::{Indicator, Oriented};
use super::{
    degenerate, invalid, ring_loops, ClosedRegion, LoopRegion, LoopShape, Membership, Region,
    RegionInput, Wrap,
};
use crate::boundary::{ResolvedFace, ResolvedLoop};
use crate::canonical::surface::Chart;
use crate::context::Eval;
use crate::failure::{Measure, MeasureFailure};
use crate::quadrature::root;
use crate::reason::UnavailableReason;
use crate::report::{Classification, LoopRole, NormalReading, RegionRule};
use crate::tolerance::{MEMBER_OFFSET_RELATIVE, NESTING_SAMPLE_RELATIVE_DEVIATION};

/// Reference points one membership query tries.
const MAX_MEMBERSHIP_PATHS: usize = 8;

/// One ring loop that bounds the region.
struct Active<'a, 'ir> {
    index: usize,
    resolved: &'a ResolvedLoop<'ir>,
    shape: &'a LoopShape,
}

/// A loop's crossing of the cut circle: its `u` in `[0, 2π)` and whether
/// the oriented loop runs in `−v` there.
struct Crossing {
    u: f64,
    downward: bool,
}

/// The region of a face on a sphere or ring torus.
pub(super) fn classify(
    face: &Face,
    chart: &Chart<'_>,
    resolved: &ResolvedFace<'_>,
    shapes: &[Option<LoopShape>],
    input: &RegionInput<'_, '_>,
) -> Measure<Region> {
    if chart.self_intersecting() {
        return Err(MeasureFailure::unavailable(
            UnavailableReason::RegionDomainUnsupported,
            Some(face.id.as_str()),
            "regions on horn and spindle tori are not classified",
        ));
    }
    let torus = matches!(chart, Chart::Torus(_));
    let lower = if torus { -PI } else { -FRAC_PI_2 };
    let mut loops = vec![
        LoopRegion {
            role: LoopRole::Vertex,
            factor: 0.0,
        };
        resolved.loops.len()
    ];
    let mut active = Vec::new();
    for (index, resolved_loop) in ring_loops(resolved)? {
        loops[index].role = LoopRole::Domain;
        let seams = input.seams.get(index);
        let bounds = resolved_loop
            .pieces
            .iter()
            .enumerate()
            .any(|(position, piece)| {
                !piece.synthesized()
                    && !seams
                        .and_then(|seams| seams.get(position))
                        .copied()
                        .unwrap_or(false)
            });
        if !bounds {
            continue;
        }
        let Some(Some(shape)) = shapes.get(index) else {
            return Err(invalid("a loop has no parameter-plane polyline"));
        };
        if resolved_loop.net_shift == [0, 0] && degenerate(shape) {
            return Err(MeasureFailure::unavailable(
                UnavailableReason::BoundaryLoopDegenerate,
                Some(resolved.loops[index].face_loop.id.as_str()),
                "a ring loop encloses no area relative to its own size",
            ));
        }
        active.push(Active {
            index,
            resolved: resolved_loop,
            shape,
        });
    }
    if active.is_empty() {
        return Ok(Region {
            rule: RegionRule::WholeSurface,
            loops,
            base: lower,
            classification: Classification::Unspecified,
            apex: None,
            exterior: 0.0,
            closed: Some(ClosedRegion {
                top: vec![[0.0, TAU]],
                wrap: None,
                membership: Membership::Whole,
            }),
        });
    }
    let sign = match input.context.declared {
        NormalReading::Reversed => -1.0,
        _ => 1.0,
    };
    let mut shift = [0_i32; 2];
    for ring in &active {
        loops[ring.index].factor = sign;
        for (axis, total) in shift.iter_mut().enumerate() {
            *total += ring.resolved.net_shift[axis];
        }
    }
    if torus && shift != [0, 0] {
        return Err(invalid("the loops do not bound a region on the torus"));
    }
    let oriented: Vec<Oriented> = active
        .iter()
        .map(|ring| {
            let mut points = ring.shape.points.clone();
            if sign < 0.0 {
                points.reverse();
            }
            Oriented {
                points,
                closed: ring.resolved.net_shift == [0, 0],
            }
        })
        .collect();
    let indicator = reference(chart, &oriented, torus)?;
    let (base, top, wrap) = if torus {
        let level = cut_level(&oriented);
        let cut = Cut {
            level,
            sign,
            loops: resolved.loops.len(),
        };
        let (wrap, crossings) = cut_crossings(input, chart, &active, &cut)?;
        let top = cut_intervals(crossings, &indicator, level)?;
        (level, top, Some(wrap))
    } else {
        let top = if indicator.beyond(1.0)? == 1 {
            vec![[0.0, TAU]]
        } else {
            Vec::new()
        };
        (lower, top, None)
    };
    Ok(Region {
        rule: RegionRule::DeclaredOrientation,
        loops,
        base,
        classification: Classification::Unspecified,
        apex: None,
        exterior: 0.0,
        closed: Some(ClosedRegion {
            top,
            wrap,
            membership: Membership::Loops(indicator),
        }),
    })
}

/// The box diagonal of a parameter-plane polyline.
fn size(points: &[[f64; 2]]) -> f64 {
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for point in points {
        for axis in 0..2 {
            low[axis] = low[axis].min(point[axis]);
            high[axis] = high[axis].max(point[axis]);
        }
    }
    (high[0] - low[0]).hypot(high[1] - low[1])
}

/// The region indicator, referenced at a point just left of one of the
/// longest loop segments. A reference whose indicator does not read 0 or 1
/// beyond the loops is rejected and the next segment is tried. A segment
/// whose lifts are refused refuses every reference.
fn reference(chart: &Chart<'_>, oriented: &[Oriented], torus: bool) -> Measure<Indicator> {
    let mut segments: Vec<([f64; 2], [f64; 2], f64, f64)> = Vec::new();
    for polyline in oriented {
        let offset = MEMBER_OFFSET_RELATIVE * size(&polyline.points);
        for (a, b) in polyline.segments() {
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            if length > 0.0 {
                segments.push((a, b, length, offset));
            }
        }
    }
    segments.sort_by(|left, right| right.2.total_cmp(&left.2));
    for (a, b, length, offset) in segments.into_iter().take(MAX_MEMBERSHIP_PATHS) {
        let left = [-(b[1] - a[1]) / length, (b[0] - a[0]) / length];
        let point = [
            a[0].midpoint(b[0]) + left[0] * offset,
            a[1].midpoint(b[1]) + left[1] * offset,
        ];
        if !torus && point[1].abs() >= FRAC_PI_2 {
            continue;
        }
        let indicator = Indicator {
            loops: oriented.to_vec(),
            periods: chart.periods(),
            reference: point,
        };
        let readable = |value: i64| value == 0 || value == 1;
        let accepted = if torus {
            readable(indicator.at([point[0], point[1] + PI])?)
        } else {
            readable(indicator.beyond(1.0)?) && readable(indicator.beyond(-1.0)?)
        };
        if accepted {
            return Ok(indicator);
        }
    }
    Err(MeasureFailure::unavailable(
        UnavailableReason::EvaluationWorkExhausted,
        None,
        "no membership path gave a consistent region indicator",
    ))
}

/// The cut level of a torus: the middle of the widest gap between the
/// loops' `v` values modulo the period.
fn cut_level(oriented: &[Oriented]) -> f64 {
    let mut values: Vec<f64> = oriented
        .iter()
        .flat_map(|polyline| polyline.points.iter().map(|point| point[1].rem_euclid(TAU)))
        .collect();
    values.sort_by(f64::total_cmp);
    values.dedup();
    let (Some(first), Some(last)) = (values.first().copied(), values.last().copied()) else {
        return -PI;
    };
    let mut best = (first + TAU - last, last);
    for pair in values.windows(2) {
        let gap = pair[1] - pair[0];
        if gap > best.0 {
            best = (gap, pair[0]);
        }
    }
    let level = best.1 + best.0 / 2.0;
    (level + PI).rem_euclid(TAU) - PI
}

/// The cut of a torus face.
#[derive(Clone, Copy)]
struct Cut {
    level: f64,
    /// `−1` when the declared normal reverses the loops.
    sign: f64,
    /// The number of loops of the face.
    loops: usize,
}

/// The parameters where each piece crosses the cut, and the crossings of
/// the oriented non-seam pieces.
fn cut_crossings(
    input: &RegionInput<'_, '_>,
    chart: &Chart<'_>,
    active: &[Active<'_, '_>],
    cut: &Cut,
) -> Measure<(Wrap, Vec<Crossing>)> {
    let eval: &Eval<'_, '_> = &input.eval;
    let Cut { level, sign, loops } = *cut;
    let mut breaks: Vec<Vec<Vec<f64>>> = (0..loops).map(|_| Vec::new()).collect();
    let mut crossings = Vec::new();
    for ring in active {
        let deviation = NESTING_SAMPLE_RELATIVE_DEVIATION * size(&ring.shape.points);
        let mut loop_breaks = Vec::with_capacity(ring.resolved.pieces.len());
        for (position, piece) in ring.resolved.pieces.iter().enumerate() {
            let seam = input
                .seams
                .get(ring.index)
                .and_then(|seams| seams.get(position))
                .copied()
                .unwrap_or(false);
            let mut piece_breaks = Vec::new();
            let polyline = piece.parameter_polyline(eval, chart, deviation)?;
            for pair in polyline.windows(2) {
                let ((before, from_uv), (after, to_uv)) = (pair[0], pair[1]);
                let sheet = |v: f64| ((v - level) / TAU).floor();
                let (first, last) = (sheet(from_uv[1]), sheet(to_uv[1]));
                if first == last {
                    continue;
                }
                lifts::resolved(&[from_uv[1], to_uv[1], level], TAU)?;
                let sheets = lifts::bounded((last - first).abs())?;
                let mut cut_sheet = first.min(last) + 1.0;
                for _ in 0..sheets {
                    let height = level + cut_sheet * TAU;
                    let mut charge = |units| eval.charge(units);
                    let mut offset = |parameter: f64| -> Measure<f64> {
                        Ok(piece.uv(eval, chart, parameter)?[1] - height)
                    };
                    let parameter = root(
                        &mut charge,
                        &mut offset,
                        [before, after],
                        piece.t1 - piece.t0,
                    )?;
                    piece_breaks.push(parameter);
                    if !seam {
                        let angle = piece.uv(eval, chart, parameter)?[0].rem_euclid(TAU);
                        let rising = (to_uv[1] > from_uv[1]) == (sign > 0.0);
                        crossings.push(Crossing {
                            u: angle,
                            downward: !rising,
                        });
                    }
                    cut_sheet += 1.0;
                }
            }
            loop_breaks.push(piece_breaks);
        }
        if let Some(slot) = breaks.get_mut(ring.index) {
            *slot = loop_breaks;
        }
    }
    Ok((
        Wrap {
            period: TAU,
            breaks,
        },
        crossings,
    ))
}

/// The cut-circle intervals in the region: from each `−v` crossing to the
/// next `+v` crossing in `+u`. Without crossings the whole circle is in the
/// region or out of it.
fn cut_intervals(
    mut crossings: Vec<Crossing>,
    indicator: &Indicator,
    level: f64,
) -> Measure<Vec<[f64; 2]>> {
    if crossings.is_empty() {
        let probe = [indicator.reference[0], level];
        return Ok(match indicator.at(probe)? {
            1 => vec![[0.0, TAU]],
            0 => Vec::new(),
            _ => return Err(invalid("the loops' declared sides contradict each other")),
        });
    }
    crossings.sort_by(|left, right| left.u.total_cmp(&right.u));
    let Some(start) = crossings.iter().position(|crossing| crossing.downward) else {
        return Err(invalid("the loops cross the cut in one direction only"));
    };
    crossings.rotate_left(start);
    if !crossings.len().is_multiple_of(2) {
        return Err(invalid("the loops cross the cut an odd number of times"));
    }
    let mut intervals = Vec::new();
    for pair in crossings.chunks(2) {
        let [enter, leave] = pair else {
            return Err(invalid("the loops cross the cut an odd number of times"));
        };
        if !enter.downward || leave.downward {
            return Err(invalid("the loops cross the cut out of order"));
        }
        let end = if leave.u < enter.u {
            leave.u + TAU
        } else {
            leave.u
        };
        intervals.push([enter.u, end]);
    }
    Ok(intervals)
}
