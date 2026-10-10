// SPDX-License-Identifier: Apache-2.0
//! Edge parameter ranges for lines, NURBS, and conics.

use crate::vecmath::normalize;
use crate::vecmath::unit_length;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;

use super::super::surfaces::intersection_resolve::curve_contains_points;

use super::planes::valid_positive_nurbs_curve;
use super::vertices::finite_model_point;
use crate::vecmath::{cross, dot};

macro_rules! require_some {
    ($value:expr) => {
        match $value {
            Some(value) => value,
            None => return Ok(None),
        }
    };
}

const EPS_ON_CONIC: f64 = 1.0e-7;
const EPS_AGREE: f64 = 1.0e-9;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

pub(in crate::decode) fn orient_line_edge_carrier(
    geometry: &mut CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) = geometry else {
        return None;
    };
    let delta: [f64; 3] = std::array::from_fn(|index| points[1][index] - points[0][index]);
    let length = dot(delta, delta).sqrt();
    let oriented = normalize(delta)?;
    *line_curve = cadmpeg_ir::geometry::analytic::LineCurve::try_new(
        Point3::from(points[0]),
        Vector3::from(oriented),
    )
    .ok()?;
    Some([0.0, length])
}

pub(in crate::decode) fn exact_line_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    if !curve_contains_points(geometry, points) {
        return None;
    }
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) = geometry else {
        return None;
    };
    let origin = line_curve.origin().get();
    let direction = *line_curve.direction().as_raw();
    let direction = [direction.x, direction.y, direction.z];
    let denominator = dot(direction, direction);
    let origin = [origin.x, origin.y, origin.z];
    let parameters = points.map(|point| {
        dot(
            std::array::from_fn(|index| point[index] - origin[index]),
            direction,
        ) / denominator
    });
    parameters
        .into_iter()
        .all(f64::is_finite)
        .then_some(if parameters[0] <= parameters[1] {
            parameters
        } else {
            [parameters[1], parameters[0]]
        })
}

/// Whether the mapped pair aligns with the target pair in the same order and
/// in reverse order, within a tolerance relative to the largest coordinate.
/// Every point is finite, so the tolerance is finite.
pub(super) fn point_pair_alignments(
    mapped: [FinitePoint3; 2],
    target: [FinitePoint3; 2],
) -> [bool; 2] {
    let coordinates = |point: FinitePoint3| <[f64; 3]>::from(point.get());
    let (mapped, target) = (mapped.map(coordinates), target.map(coordinates));
    let mismatch = |left: [f64; 3], right: [f64; 3]| {
        dot(
            std::array::from_fn(|index| left[index] - right[index]),
            std::array::from_fn(|index| left[index] - right[index]),
        )
        .sqrt()
    };
    let scale = mapped
        .into_iter()
        .chain(target)
        .flatten()
        .map(f64::abs)
        .fold(1.0, f64::max);
    let tolerance = EPS_AGREE * scale;
    [
        mismatch(mapped[0], target[0]).max(mismatch(mapped[1], target[1])) <= tolerance,
        mismatch(mapped[0], target[1]).max(mismatch(mapped[1], target[0])) <= tolerance,
    ]
}

pub(super) fn nurbs_control_extent(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nurbs: &NurbsCurve,
) -> Result<f64, cadmpeg_core::CodecError> {
    let initial = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    let extend = |mut bounds: [[f64; 3]; 2], point: FinitePoint3| {
        let point = point.get();
        for (index, coordinate) in [point.x, point.y, point.z].into_iter().enumerate() {
            bounds[0][index] = bounds[0][index].min(coordinate);
            bounds[1][index] = bounds[1][index].max(coordinate);
        }
        Ok(bounds)
    };
    let bounds = match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => ctx.fold(
            points, initial, |bounds, point| extend(bounds, *point),
            "creo NURBS polynomial poles",
        )?,
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => ctx.fold(
            points, initial, |bounds, pole| extend(bounds, pole.point),
            "creo NURBS rational poles",
        )?,
    };
    Ok((0..3)
        .map(|index| bounds[1][index] - bounds[0][index])
        .fold(1.0, f64::max))
}

pub(super) fn nurbs_weights_positive(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    nurbs: &NurbsCurve,
) -> Result<bool, cadmpeg_core::CodecError> {
    match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { .. } => Ok(true),
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => Ok(ctx.all_by(
            points,
            |pole| Ok(pole.weight.get() > 0.0),
            "creo NURBS pole weights",
        )?),
    }
}

pub(in crate::decode) fn nurbs_intrinsic_parameter_range(
    nurbs: &NurbsCurve,
) -> Option<[FiniteReal; 2]> {
    let degree = usize::try_from(nurbs.degree()).ok()?;
    let range = [
        nurbs.knots().finite_knot(degree)?,
        nurbs.knots().finite_knot(nurbs.pole_count())?,
    ];
    (range[0] < range[1]).then_some(range)
}

pub(super) fn nonperiodic_nurbs_endpoint_points(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
) -> Result<Option<[[f64; 3]; 2]>, cadmpeg_core::CodecError> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = geometry else {
        return Ok(None);
    };
    require_some!((!nurbs.periodic()).then_some(()));
    require_some!(valid_positive_nurbs_curve(ctx, nurbs)?);
    let range = require_some!(nurbs_intrinsic_parameter_range(nurbs));
    let [first, second] =
        range.map(|parameter| {
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
                ctx,
                geometry,
                parameter.get(),
            ))
            .and_then(|value| {
                Ok(cadmpeg_ir::eval::finite_or_refusal(value)?
                    .map(|point| [point.x, point.y, point.z]))
            })
        });
    let points = [first?, second?];
    let [Some(first), Some(second)] = points else {
        return Ok(None);
    };
    Ok(Some([first, second]))
}

fn nonperiodic_nurbs_edge_parameter_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = geometry else {
        return Ok(None);
    };
    if nurbs.periodic() {
        return Ok(None);
    }
    let degree = require_some!(usize::try_from(nurbs.degree()).ok());
    let range = FiniteReal::raw_array(require_some!(nurbs_intrinsic_parameter_range(nurbs)));

    if degree == 1 {
        require_some!(nurbs_weights_positive(ctx, nurbs)?.then_some(()));
        let scale = nurbs_control_extent(ctx, nurbs)?;
        let tolerance = EPS_AGREE * scale;
        let first = require_some!(degree_one_nurbs_point_parameter(
            ctx, geometry, nurbs, points[0], range, tolerance
        )?);
        let second = require_some!(degree_one_nurbs_point_parameter(
            ctx, geometry, nurbs, points[1], range, tolerance
        )?);
        let parameters = if first <= second {
            [first, second]
        } else {
            [second, first]
        };
        return Ok(
            (cadmpeg_ir::math::parameter_fraction(parameters[0], range[0], range[1])
                .zip(cadmpeg_ir::math::parameter_fraction(
                    parameters[1],
                    range[0],
                    range[1],
                ))
                .is_some_and(|(first, second)| second.get() - first.get() > EPS_NEAR_ZERO))
            .then_some(parameters),
        );
    }

    let [first, second] = range.map(|parameter| {
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
            ctx, geometry, parameter,
        ))
    });
    let mapped = [
        cadmpeg_ir::eval::finite_or_refusal(first?)?,
        cadmpeg_ir::eval::finite_or_refusal(second?)?,
    ];
    // An edge endpoint outside the finite range aligns with no carrier end.
    let ([Some(first), Some(second)], [Some(start), Some(end)]) =
        (mapped, points.map(finite_model_point))
    else {
        return Ok(None);
    };
    Ok(match point_pair_alignments([first, second], [start, end]) {
        [true, false] | [false, true] => Some(range),
        _ => None,
    })
}

/// Orient a non-periodic NURBS carrier to the topological edge direction.
///
/// Edge parameter ranges are canonical and therefore increasing. When the
/// native edge reverses the carrier direction, reverse the NURBS definition
/// and keep the same geometric parameter domain.
pub(in crate::decode) fn orient_nonperiodic_nurbs_edge_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &mut CurveGeometry,
    points: [[f64; 3]; 2],
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let range = require_some!(nonperiodic_nurbs_edge_parameter_range(
        ctx, geometry, points
    )?);
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &*geometry else {
        return Ok(None);
    };
    let degree = require_some!(usize::try_from(nurbs.degree()).ok());
    let intrinsic_range = require_some!(nurbs_intrinsic_parameter_range(nurbs));
    if degree == 1 {
        let (first, second) = {
            let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &*geometry else {
                return Ok(None);
            };
            let tolerance = EPS_AGREE * nurbs_control_extent(ctx, nurbs)?;
            let first = require_some!(degree_one_nurbs_point_parameter(
                ctx,
                &*geometry,
                nurbs,
                points[0],
                FiniteReal::raw_array(intrinsic_range),
                tolerance,
            )?);
            let second = require_some!(degree_one_nurbs_point_parameter(
                ctx,
                &*geometry,
                nurbs,
                points[1],
                FiniteReal::raw_array(intrinsic_range),
                tolerance,
            )?);
            (first, second)
        };
        if first <= second {
            return Ok(Some([first, second]));
        }
        let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = geometry else {
            return Ok(None);
        };
        require_some!(nurbs.reverse_parameterization_in_range(
            ctx,
            intrinsic_range[0],
            intrinsic_range[1]
        )?);
        let [Some(first), Some(second)] = [first, second].map(FiniteReal::new) else {
            return Ok(None);
        };
        let [start, end] = intrinsic_range;
        return Ok(Some([
            require_some!(cadmpeg_ir::math::reflect_parameter(first, start, end)).get(),
            require_some!(cadmpeg_ir::math::reflect_parameter(second, start, end)).get(),
        ]));
    }

    let [first, second] = intrinsic_range.map(|parameter| {
        cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
            ctx,
            &*geometry,
            parameter.get(),
        ))
    });
    let mapped = [
        cadmpeg_ir::eval::finite_or_refusal(first?)?,
        cadmpeg_ir::eval::finite_or_refusal(second?)?,
    ];
    // An edge endpoint outside the finite range aligns with no carrier end.
    let ([Some(first), Some(second)], [Some(start), Some(end)]) =
        (mapped, points.map(finite_model_point))
    else {
        return Ok(None);
    };
    Ok(match point_pair_alignments([first, second], [start, end]) {
        [true, false] => Some(range),
        [false, true] => {
            let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = geometry else {
                return Ok(None);
            };
            require_some!(nurbs.reverse_parameterization_in_range(
                ctx,
                intrinsic_range[0],
                intrinsic_range[1]
            )?);
            Some(range)
        }
        _ => None,
    })
}

pub(in crate::decode) fn full_periodic_nurbs_edge_parameter_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = geometry else {
        return Ok(None);
    };
    require_some!(nurbs.periodic().then_some(()));
    require_some!(nurbs_weights_positive(ctx, nurbs)?.then_some(()));
    let range = FiniteReal::raw_array(require_some!(nurbs_intrinsic_parameter_range(nurbs)));
    let [first, second] =
        range.map(|parameter| {
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
                ctx, geometry, parameter,
            ))
            .and_then(|value| {
                Ok(cadmpeg_ir::eval::finite_or_refusal(value)?
                    .map(|point| [point.x, point.y, point.z]))
            })
        });
    let mapped = [first?, second?];
    let [Some(first), Some(second)] = mapped else {
        return Ok(None);
    };
    let tolerance = EPS_AGREE * nurbs_control_extent(ctx, nurbs)?;
    Ok([first, second]
        .into_iter()
        .all(|mapped| {
            let delta: [f64; 3] = std::array::from_fn(|index| mapped[index] - point[index]);
            dot(delta, delta).sqrt() <= tolerance
        })
        .then_some(range))
}

fn degree_one_nurbs_point_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
    nurbs: &NurbsCurve,
    point: [f64; 3],
    range: [f64; 2],
    tolerance: f64,
) -> Result<Option<f64>, cadmpeg_core::CodecError> {
    let parameter_tolerance = (EPS_AGREE * range[1] - EPS_AGREE * range[0]).abs();
    let mut candidate: Option<f64> = None;
    match nurbs.pole_rows() {
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Polynomial { points } => {
            let mut previous = None;
            let mut traversal = (points).iter().enumerate();
            while let Some((span, second)) =
                ctx.next_charged(&mut traversal, "creo degree-one polynomial NURBS spans")?
            {
                let second = *second;
                let Some(first) = previous.replace(second) else {
                    continue;
                };
                match degree_one_nurbs_span_parameter(
                    ctx,
                    geometry,
                    nurbs,
                    DegreeOneSpan {
                        point,
                        tolerance,
                        index: span,
                        endpoints: [first, second],
                        weights: [1.0, 1.0],
                    },
                )? {
                    DegreeOneSpanParameter::Ambiguous => return Ok(None),
                    DegreeOneSpanParameter::Skipped => {}
                    DegreeOneSpanParameter::Parameter(parameter) => {
                        if !retain_degree_one_parameter(
                            &mut candidate,
                            parameter,
                            parameter_tolerance,
                        ) {
                            return Ok(None);
                        }
                    }
                }
            }
        }
        cadmpeg_ir::geometry::nurbs::NurbsPoles3::Rational { points } => {
            let mut previous = None;
            let mut traversal = (points).iter().enumerate();
            while let Some((span, second)) =
                ctx.next_charged(&mut traversal, "creo degree-one rational NURBS spans")?
            {
                let Some((first, first_weight)) =
                    previous.replace((second.point, second.weight.get()))
                else {
                    continue;
                };
                match degree_one_nurbs_span_parameter(
                    ctx,
                    geometry,
                    nurbs,
                    DegreeOneSpan {
                        point,
                        tolerance,
                        index: span,
                        endpoints: [first, second.point],
                        weights: [first_weight, second.weight.get()],
                    },
                )? {
                    DegreeOneSpanParameter::Ambiguous => return Ok(None),
                    DegreeOneSpanParameter::Skipped => {}
                    DegreeOneSpanParameter::Parameter(parameter) => {
                        if !retain_degree_one_parameter(
                            &mut candidate,
                            parameter,
                            parameter_tolerance,
                        ) {
                            return Ok(None);
                        }
                    }
                }
            }
        }
    }
    Ok(candidate)
}

enum DegreeOneSpanParameter {
    Ambiguous,
    Skipped,
    Parameter(f64),
}

fn retain_degree_one_parameter(
    candidate: &mut Option<f64>,
    parameter: f64,
    tolerance: f64,
) -> bool {
    if let Some(first) = candidate {
        matches!(
            (parameter - *first).abs().partial_cmp(&tolerance),
            Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
        )
    } else {
        *candidate = Some(parameter);
        true
    }
}

#[derive(Clone, Copy)]
struct DegreeOneSpan {
    point: [f64; 3],
    tolerance: f64,
    index: usize,
    endpoints: [FinitePoint3; 2],
    weights: [f64; 2],
}

fn degree_one_nurbs_span_parameter(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
    nurbs: &NurbsCurve,
    span: DegreeOneSpan,
) -> Result<DegreeOneSpanParameter, cadmpeg_core::CodecError> {
    let DegreeOneSpan {
        point,
        tolerance,
        index: span_index,
        endpoints: [first, second],
        weights: [first_weight, second_weight],
    } = span;
    let lower = nurbs.knots()[span_index];
    let upper = nurbs.knots()[span_index + 1];
    if upper <= lower {
        return Ok(DegreeOneSpanParameter::Skipped);
    }
    let first = first.get();
    let second = second.get();
    let delta = [second.x - first.x, second.y - first.y, second.z - first.z];
    let denominator = dot(delta, delta);
    if !denominator.is_finite() {
        return Ok(DegreeOneSpanParameter::Skipped);
    }
    let relative = [point[0] - first.x, point[1] - first.y, point[2] - first.z];
    if denominator <= tolerance * tolerance {
        return Ok(if dot(relative, relative).sqrt() <= tolerance {
            DegreeOneSpanParameter::Ambiguous
        } else {
            DegreeOneSpanParameter::Skipped
        });
    }
    let fraction = dot(relative, delta) / denominator;
    if !(-EPS_AGREE..=1.0 + EPS_AGREE).contains(&fraction) {
        return Ok(DegreeOneSpanParameter::Skipped);
    }
    let fraction = match fraction.partial_cmp(&0.0) {
        Some(std::cmp::Ordering::Less) => 0.0,
        _ => match fraction.partial_cmp(&1.0) {
            Some(std::cmp::Ordering::Greater) => 1.0,
            _ => fraction,
        },
    };
    let projected = [
        first.x + fraction * delta[0],
        first.y + fraction * delta[1],
        first.z + fraction * delta[2],
    ];
    let mismatch: [f64; 3] = std::array::from_fn(|index| projected[index] - point[index]);
    if dot(mismatch, mismatch).sqrt() > tolerance {
        return Ok(DegreeOneSpanParameter::Skipped);
    }
    let rational_denominator = second_weight * (1.0 - fraction) + fraction * first_weight;
    if rational_denominator <= 0.0 || !rational_denominator.is_finite() {
        return Ok(DegreeOneSpanParameter::Skipped);
    }
    let local = fraction * first_weight / rational_denominator;
    let knot_span = upper - lower;
    let parameter = if knot_span.is_finite() {
        lower + local * knot_span
    } else {
        let Some(parameter) = cadmpeg_ir::math::interpolate(lower, upper, local) else {
            return Ok(DegreeOneSpanParameter::Ambiguous);
        };
        parameter.get()
    };
    let Some(mapped) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, geometry, parameter),
        )?)?
    else {
        return Ok(DegreeOneSpanParameter::Skipped);
    };
    let mismatch = [
        mapped.x - point[0],
        mapped.y - point[1],
        mapped.z - point[2],
    ];
    Ok(if dot(mismatch, mismatch).sqrt() <= tolerance {
        DegreeOneSpanParameter::Parameter(parameter)
    } else {
        DegreeOneSpanParameter::Skipped
    })
}

#[derive(Clone, Copy)]
pub(in crate::decode) struct PeriodicConicFrame {
    pub(in crate::decode) center: [f64; 3],
    pub(in crate::decode) normal: [f64; 3],
    pub(in crate::decode) x_axis: [f64; 3],
    pub(in crate::decode) y_axis: [f64; 3],
    pub(in crate::decode) radii: [f64; 2],
}

#[derive(Clone, Copy)]
pub(super) struct PlanarConicEquation {
    pub(super) origin: [f64; 3],
    pub(super) normal: [f64; 3],
    pub(super) x_axis: [f64; 3],
    pub(super) y_axis: [f64; 3],
    pub(super) quadratic: [f64; 2],
    pub(super) linear: [f64; 2],
    pub(super) constant: f64,
    pub(super) scale: f64,
}

#[derive(Clone, Copy)]
enum NonperiodicConicFamily {
    Parabola,
    Hyperbola,
}

#[derive(Clone, Copy)]
struct NonperiodicConicFrame {
    origin: [f64; 3],
    normal: [f64; 3],
    x_axis: [f64; 3],
    y_axis: [f64; 3],
    x_scale: f64,
    y_scale: f64,
    family: NonperiodicConicFamily,
}

pub(super) fn planar_conic_equation(geometry: &CurveGeometry) -> Option<PlanarConicEquation> {
    if let Some(frame) = periodic_conic_frame(geometry) {
        return Some(PlanarConicEquation {
            origin: frame.center,
            normal: frame.normal,
            x_axis: frame.x_axis,
            y_axis: frame.y_axis,
            quadratic: [1.0 / frame.radii[0].powi(2), 1.0 / frame.radii[1].powi(2)],
            linear: [0.0, 0.0],
            constant: -1.0,
            scale: frame.radii.into_iter().fold(1.0, f64::max),
        });
    }
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let (quadratic, linear, constant) = match family {
        NonperiodicConicFamily::Parabola => ([0.0, -1.0 / (2.0 * y_scale)], [1.0, 0.0], 0.0),
        NonperiodicConicFamily::Hyperbola => (
            [1.0 / x_scale.powi(2), -1.0 / y_scale.powi(2)],
            [0.0, 0.0],
            -1.0,
        ),
    };
    Some(PlanarConicEquation {
        origin,
        normal,
        x_axis,
        y_axis,
        quadratic,
        linear,
        constant,
        scale: x_scale.max(y_scale),
    })
}

fn nonperiodic_conic_frame(geometry: &CurveGeometry) -> Option<NonperiodicConicFrame> {
    let (origin, normal, x_axis, x_scale, y_scale, family) = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Parabola(parabola_curve)) => {
            let vertex = parabola_curve.vertex().get();
            let focal_distance = parabola_curve.focal_distance().get();
            (
                [vertex.x, vertex.y, vertex.z],
                unit_length(*parabola_curve.frame().axis()),
                unit_length(*parabola_curve.frame().reference()),
                focal_distance,
                2.0 * focal_distance,
                NonperiodicConicFamily::Parabola,
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(hyperbola_curve)) => {
            let center = hyperbola_curve.center().get();
            let major_radius = hyperbola_curve.major_radius().get();
            let minor_radius = hyperbola_curve.minor_radius().get();
            (
                [center.x, center.y, center.z],
                unit_length(*hyperbola_curve.frame().axis()),
                unit_length(*hyperbola_curve.frame().reference()),
                major_radius,
                minor_radius,
                NonperiodicConicFamily::Hyperbola,
            )
        }
        _ => return None,
    };
    (dot(normal, x_axis).abs() <= EPS_AGREE).then_some(())?;
    let y_axis = normalize(cross(normal, x_axis))?;
    (y_scale > 0.0 && y_scale.is_finite()).then_some(())?;
    Some(NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    })
}

pub(in crate::decode) fn periodic_conic_frame(
    geometry: &CurveGeometry,
) -> Option<PeriodicConicFrame> {
    let (center, axis, x_axis, radii) = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let center = circle_curve.center().get();
            let radius = circle_curve.radius().get();
            (
                [center.x, center.y, center.z],
                unit_length(*circle_curve.frame().axis()),
                unit_length(*circle_curve.frame().reference()),
                [radius, radius],
            )
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let center = ellipse_curve.center().get();
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            (
                [center.x, center.y, center.z],
                unit_length(*ellipse_curve.frame().axis()),
                unit_length(*ellipse_curve.frame().reference()),
                [major_radius, minor_radius],
            )
        }
        _ => return None,
    };
    (dot(axis, x_axis).abs() <= EPS_AGREE).then_some(())?;
    let y_axis = normalize(cross(axis, x_axis))?;
    Some(PeriodicConicFrame {
        center,
        normal: axis,
        x_axis,
        y_axis,
        radii,
    })
}

pub(in crate::decode) fn nonperiodic_conic_parameter(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<f64> {
    let NonperiodicConicFrame {
        origin,
        normal,
        x_axis,
        y_axis,
        x_scale,
        y_scale,
        family,
    } = nonperiodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - origin[index]);
    let scale = dot(relative, relative)
        .sqrt()
        .max(x_scale)
        .max(y_scale)
        .max(1.0);
    (dot(relative, normal).abs() <= EPS_ON_CONIC * scale).then_some(())?;
    let x = dot(relative, x_axis);
    let y = dot(relative, y_axis);
    let parameter = match family {
        NonperiodicConicFamily::Parabola => y / y_scale,
        NonperiodicConicFamily::Hyperbola => (y / y_scale).asinh(),
    };
    let expected_x = match family {
        NonperiodicConicFamily::Parabola => x_scale * parameter * parameter,
        NonperiodicConicFamily::Hyperbola => x_scale * parameter.cosh(),
    };
    (parameter.is_finite() && (x - expected_x).abs() <= EPS_ON_CONIC * scale).then_some(parameter)
}

pub(in crate::decode) fn nonperiodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
) -> Option<[f64; 2]> {
    let [Some(first), Some(second)] =
        points.map(|point| nonperiodic_conic_parameter(geometry, point))
    else {
        return None;
    };
    let parameters = if first <= second {
        [first, second]
    } else {
        [second, first]
    };
    (parameters[1] - parameters[0] > EPS_NEAR_ZERO).then_some(parameters)
}

pub(super) fn periodic_conic_edge_parameter_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    geometry: &CurveGeometry,
    points: [[f64; 3]; 2],
    interior: [f64; 3],
) -> Result<Option<[f64; 2]>, cadmpeg_core::CodecError> {
    if !curve_contains_points(geometry, points)
        || !curve_contains_points(geometry, [interior, interior])
    {
        return Ok(None);
    }
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = require_some!(periodic_conic_frame(geometry));
    let parameter = |point: [f64; 3]| {
        let relative = std::array::from_fn(|index| point[index] - center[index]);
        (dot(relative, y_axis) / radii[1])
            .atan2(dot(relative, x_axis) / radii[0])
            .rem_euclid(std::f64::consts::TAU)
    };
    let [first, second] = points.map(parameter);
    let increasing = |start: f64, end: f64| {
        [
            start,
            if end < start {
                end + std::f64::consts::TAU
            } else {
                end
            },
        ]
    };
    let first_arc = increasing(first, second);
    let second_arc = if (first - second).abs() <= EPS_NEAR_ZERO {
        [first, first + std::f64::consts::TAU]
    } else {
        increasing(second, first)
    };
    let scale = radii.into_iter().fold(1.0, f64::max);
    let matches_interior = |range: [f64; 2]| -> Result<bool, cadmpeg_core::CodecError> {
        Ok(
            cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::curve_point(
                    ctx,
                    geometry,
                    f64::midpoint(range[0], range[1]),
                ),
            )?)?
            .is_some_and(|point| {
                let point = [point.x, point.y, point.z];
                dot(
                    std::array::from_fn(|index| point[index] - interior[index]),
                    std::array::from_fn(|index| point[index] - interior[index]),
                )
                .sqrt()
                    <= EPS_AGREE * scale
            }),
        )
    };
    let selected = match (matches_interior(first_arc)?, matches_interior(second_arc)?) {
        (true, false) => first_arc,
        (false, true) => second_arc,
        _ => return Ok(None),
    };
    Ok((selected[1] - selected[0] > EPS_NEAR_ZERO).then_some(selected))
}

pub(in crate::decode) fn full_periodic_conic_edge_parameter_range(
    geometry: &CurveGeometry,
    point: [f64; 3],
) -> Option<[f64; 2]> {
    curve_contains_points(geometry, [point, point]).then_some(())?;
    let PeriodicConicFrame {
        center,
        x_axis,
        y_axis,
        radii,
        ..
    } = periodic_conic_frame(geometry)?;
    let relative = std::array::from_fn(|index| point[index] - center[index]);
    let start = (dot(relative, y_axis) / radii[1])
        .atan2(dot(relative, x_axis) / radii[0])
        .rem_euclid(std::f64::consts::TAU);
    Some([start, start + std::f64::consts::TAU])
}

#[cfg(test)]
mod native_parameter_tests;

#[cfg(test)]
mod tests {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn reversing_nurbs_rejects_overflow_without_mutating_the_carrier() {
        let original = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("finite NURBS fixture");
        let mut reversed = original.clone();

        let range = [cadmpeg_ir::scalar::FiniteReal::new(f64::MAX).expect("finite range"); 2];
        assert!(reversed
            .reverse_parameterization_in_range(
                &cadmpeg_test_support::service_decode_context(),
                range[0],
                range[1]
            )
            .expect("range reversal admission")
            .is_none());
        assert_eq!(reversed, original);
    }
}

#[cfg(test)]
mod numerical_range_tests;

#[cfg(test)]
mod evaluation_tests;
