// SPDX-License-Identifier: Apache-2.0
//! Reparameterize spline pcurves onto their edge's range and join each loop's
//! pcurves within one parameter period.

use std::borrow::Cow;
use std::f64::consts::{FRAC_PI_2, TAU};

use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles, WeightedPole2};
use cadmpeg_ir::geometry::{DirectedParameterRange, SolvedSurfaceGeometry};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::transform::Transform2;
use cadmpeg_ir::units::FinitePoint2;

/// Relative tolerance for a use range to state the edge range unchanged.
const EPS_PARAMETER_RANGE_MATCH: f64 = 1.0e-9;

/// Relative tolerance for two consecutive loop pcurves to meet.
const EPS_LOOP_JOIN: f64 = 1.0e-6;

/// Relative slack on a conic use that spans one whole period.
const EPS_CONIC_PERIOD: f64 = 1.0e-9;

/// A circle or ellipse pcurve under a placement the conic writer cannot bake,
/// such as the reflection that makes a circle run clockwise, has no plain STEP
/// conic. It is written as the exact rational quadratic B-spline of the same
/// arc over the use's parameter range, at most a quarter turn per span, with
/// the placement applied to its poles (an affine map keeps a rational spline
/// exact). Each knot is the conic's own parameter at that pole, so the spline
/// meets the conic at every knot and runs in the same direction; the result
/// then takes the same edge-range mapping and loop joins as any spline pcurve.
/// Any other pcurve is left unchanged.
pub(super) fn lower_placed_conic(
    geometry: &PcurveGeometry,
    source_range: Option<DirectedParameterRange>,
) -> Cow<'_, PcurveGeometry> {
    let lowered = (|| {
        let mut transform = Transform2::identity();
        let mut basis = geometry;
        while let PcurveGeometry::Transformed(placed) = basis {
            transform = compose_2d(transform, *placed.transform())?;
            basis = placed.basis();
        }
        let scaled = |axis: Point2, radius: f64| Point2::new(axis.u * radius, axis.v * radius);
        let (center, x, y) = match basis {
            PcurveGeometry::Circle(circle) => (
                circle.center().get(),
                scaled(circle.x_axis().get(), circle.radius().get()),
                scaled(circle.y_axis().get(), circle.radius().get()),
            ),
            PcurveGeometry::Ellipse(ellipse) => (
                ellipse.center().get(),
                scaled(ellipse.x_axis().get(), ellipse.major_radius().get()),
                scaled(ellipse.y_axis().get(), ellipse.minor_radius().get()),
            ),
            _ => return None,
        };
        if crate::geometry::pcurve_placement_bakes(geometry) {
            return None;
        }
        let [start, end] = source_range.map_or([0.0, TAU], |range| {
            let [a, b] = range.endpoints();
            [a.min(b), a.max(b)]
        });
        let span = end - start;
        if !span.is_finite() || span <= 0.0 || span > TAU * (1.0 + EPS_CONIC_PERIOD) {
            return None;
        }
        let spans = (1_u32..=4)
            .find(|spans| span <= FRAC_PI_2 * f64::from(*spans) * (1.0 + EPS_CONIC_PERIOD))?;
        let step = span / f64::from(spans);
        let weight = (step / 2.0).cos();
        let at = |angle: f64, reach: f64| {
            transform.apply_point(Point2::new(
                center.u + reach * (angle.cos() * x.u + angle.sin() * y.u),
                center.v + reach * (angle.cos() * x.v + angle.sin() * y.v),
            ))
        };
        let mut points = vec![at(start, 1.0)];
        let mut weights = vec![1.0];
        let mut knots = vec![start, start, start];
        let mut angle = start;
        for index in 1..=spans {
            let last = index == spans;
            let next = if last {
                end
            } else {
                start + step * f64::from(index)
            };
            points.push(at(angle + step / 2.0, 1.0 / weight));
            points.push(at(next, 1.0));
            weights.extend([weight, 1.0]);
            knots.extend(std::iter::repeat_n(next, if last { 3 } else { 2 }));
            angle = next;
        }
        let poles = PcurveNurbsPoles::from_lanes(points, Some(weights)).ok()?;
        let nurbs = PcurveNurbs::new(2, knots, poles, false).ok()?;
        Some(PcurveGeometry::Nurbs { nurbs })
    })();
    lowered.map_or_else(|| Cow::Borrowed(geometry), Cow::Owned)
}

/// `outer * inner` for column-vector application.
fn compose_2d(outer: Transform2, inner: Transform2) -> Option<Transform2> {
    let left = outer.rows();
    let right = inner.rows();
    let entry = |row: usize, column: usize| {
        (0..3)
            .map(|inner| left[row][inner] * right[inner][column])
            .sum::<f64>()
    };
    Transform2::affine([
        [entry(0, 0), entry(0, 1), entry(0, 2)],
        [entry(1, 0), entry(1, 1), entry(1, 2)],
    ])
}

/// `SURFACE_CURVE` inherits the 3D curve's parameterization, so a spline
/// pcurve must run on its edge's range. `source_range` is the use interval on
/// the pcurve's own parameter, directed from the edge's start to its end; a
/// decreasing interval runs against the pcurve. The affine map from the edge
/// range onto that interval moves the knots, and a decreasing interval also
/// reverses the knot and pole order. The parameter-space locus is unchanged.
/// The mapping is per use: a shared pcurve can serve different edge ranges.
pub(super) fn align_spline_to_edge<'a>(
    geometry: &'a PcurveGeometry,
    source_range: Option<[f64; 2]>,
    edge: &Edge,
) -> Cow<'a, PcurveGeometry> {
    let aligned = (|| {
        let PcurveGeometry::Nurbs { nurbs } = geometry else {
            return None;
        };
        let [source_start, source_end] = source_range?;
        let [target_start, target_end] = edge.param_range()?.get();
        let source_span = source_end - source_start;
        let target_span = target_end - target_start;
        if !source_span.is_finite()
            || !target_span.is_finite()
            || source_span == 0.0
            || target_span <= 0.0
        {
            return None;
        }
        let tolerance = EPS_PARAMETER_RANGE_MATCH * source_span.abs().max(target_span).max(1.0);
        if (source_start - target_start).abs() <= tolerance
            && (source_end - target_end).abs() <= tolerance
        {
            return None;
        }
        let scale = target_span / source_span;
        let to_edge = |knot: &f64| target_start + (knot - source_start) * scale;
        let reversed = source_span < 0.0;
        let knots = if reversed {
            nurbs.knots().iter().rev().map(to_edge).collect::<Vec<_>>()
        } else {
            nurbs.knots().iter().map(to_edge).collect::<Vec<_>>()
        };
        if knots.iter().any(|knot| !knot.is_finite()) {
            return None;
        }
        let poles = if reversed {
            match nurbs.pole_rows() {
                PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
                    points: points.iter().rev().copied().collect(),
                },
                PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
                    points: points.iter().rev().copied().collect(),
                },
            }
        } else {
            nurbs.pole_rows().clone()
        };
        let nurbs = PcurveNurbs::new(nurbs.degree(), knots, poles, nurbs.periodic()).ok()?;
        Some(PcurveGeometry::Nurbs { nurbs })
    })();
    aligned.map_or_else(|| Cow::Borrowed(geometry), Cow::Owned)
}

/// The parameter periods of a surface's u and v directions. An analytic
/// angle parameter has period 2π; a periodic NURBS direction has its knot
/// domain as period.
pub(super) fn surface_periods(surface: &SolvedSurfaceGeometry) -> [Option<f64>; 2] {
    match surface {
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_) => [Some(TAU), None],
        SolvedSurfaceGeometry::Torus(_) => [Some(TAU), Some(TAU)],
        SolvedSurfaceGeometry::Nurbs(nurbs) => [
            nurbs
                .u_periodic()
                .then(|| knot_domain(nurbs.u_knots().as_slice(), nurbs.u_degree()))
                .flatten(),
            nurbs
                .v_periodic()
                .then(|| knot_domain(nurbs.v_knots().as_slice(), nurbs.v_degree()))
                .flatten(),
        ],
        _ => [None, None],
    }
}

fn knot_domain(knots: &[f64], degree: u32) -> Option<f64> {
    let degree = usize::try_from(degree).ok()?;
    let start = *knots.get(degree)?;
    let end = *knots.get(knots.len().checked_sub(degree.checked_add(1)?)?)?;
    let span = end - start;
    (span.is_finite() && span > 0.0).then_some(span)
}

/// The whole-period shift of each pcurve in a loop, given each pcurve's
/// parameter-space start and end in loop order. Each pcurve after the first
/// moves by whole periods so that it starts where the previous one ends. A
/// pcurve that no whole-period shift joins to its predecessor keeps its place.
pub(super) fn loop_period_shifts(
    ends: &[([f64; 2], [f64; 2])],
    periods: [Option<f64>; 2],
) -> Vec<[f64; 2]> {
    let mut shifts = vec![[0.0; 2]; ends.len()];
    for index in 1..ends.len() {
        let previous_end = [
            ends[index - 1].1[0] + shifts[index - 1][0],
            ends[index - 1].1[1] + shifts[index - 1][1],
        ];
        let start = ends[index].0;
        let mut shift = [0.0; 2];
        let mut joined = true;
        for axis in 0..2 {
            let gap = previous_end[axis] - start[axis];
            if let Some(period) = periods[axis] {
                shift[axis] = (gap / period).round() * period;
            }
            let tolerance = EPS_LOOP_JOIN * periods[axis].unwrap_or(1.0).max(1.0);
            joined &= (gap - shift[axis]).abs() <= tolerance;
        }
        if joined {
            shifts[index] = shift;
        }
    }
    shifts
}

/// `geometry` moved by `shift` in parameter space. Only spline pcurves move.
pub(super) fn shifted(geometry: &PcurveGeometry, shift: [f64; 2]) -> Option<PcurveGeometry> {
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        return None;
    };
    let moved = |point: FinitePoint2| {
        let point = point.get();
        FinitePoint2::new(Point2::new(point.u + shift[0], point.v + shift[1]))
    };
    let poles = match nurbs.pole_rows() {
        PcurveNurbsPoles::Polynomial { points } => PcurveNurbsPoles::Polynomial {
            points: points
                .iter()
                .copied()
                .map(moved)
                .collect::<Option<Vec<_>>>()?,
        },
        PcurveNurbsPoles::Rational { points } => PcurveNurbsPoles::Rational {
            points: points
                .iter()
                .map(|pole| {
                    Some(WeightedPole2 {
                        point: moved(pole.point)?,
                        weight: pole.weight,
                    })
                })
                .collect::<Option<Vec<_>>>()?,
        },
    };
    let nurbs = PcurveNurbs::new(
        nurbs.degree(),
        nurbs.knots().as_slice().to_vec(),
        poles,
        nurbs.periodic(),
    )
    .ok()?;
    Some(PcurveGeometry::Nurbs { nurbs })
}

#[cfg(test)]
mod tests;
