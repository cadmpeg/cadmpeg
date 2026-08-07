// SPDX-License-Identifier: Apache-2.0
//! Point evaluation of geometry carriers.
//!
//! Evaluators map carrier parameters to model-space (or parameter-space)
//! points using the carriers' own parameterizations: conic parameters are
//! angles from the reference/major direction, line parameters are signed
//! distances along the unit direction, and B-splines evaluate by Cox–de Boor
//! over their stored knot vectors. [`model_surface_point`] resolves construction-
//! backed carriers that require other model entities. Carriers without a typed
//! parameterization ([`CurveGeometry::Unknown`], [`CurveGeometry::Composite`],
//! [`SurfaceGeometry::Unknown`], parabolas, and hyperbolas) evaluate to `None`.
//! [`model_curve_point_by_id`] resolves construction-backed curves whose
//! parameterization is established by model entities.

use crate::geometry::{
    CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, ProceduralSurfaceDefinition,
    SurfaceGeometry, SurfaceParameterAxis,
};
use crate::math::{Point2, Point3, Vector3};
use crate::sketches::SpatialSketchGeometry;
use crate::transform::Transform;
use crate::CadIr;

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// Signed separation of two parallel model-space sketch lines in one plane.
///
/// Positive distance is along the left normal selected by `plane_normal`
/// and the source line's stored traversal.
pub fn spatial_line_offset(
    source: &SpatialSketchGeometry,
    result: &SpatialSketchGeometry,
    plane_normal: Vector3,
) -> Option<f64> {
    let (
        SpatialSketchGeometry::Line {
            start: source_start,
            end: source_end,
        },
        SpatialSketchGeometry::Line {
            start: result_start,
            end: result_end,
        },
    ) = (source, result)
    else {
        return None;
    };
    let tangent = Vector3::new(
        source_end.x - source_start.x,
        source_end.y - source_start.y,
        source_end.z - source_start.z,
    );
    let result_tangent = Vector3::new(
        result_end.x - result_start.x,
        result_end.y - result_start.y,
        result_end.z - result_start.z,
    );
    let length = tangent.norm();
    let result_length = result_tangent.norm();
    if length <= 1.0e-12
        || result_length <= 1.0e-12
        || cross(tangent, result_tangent).norm() > 1.0e-9 * length * result_length
    {
        return None;
    }
    let left = cross(plane_normal, tangent);
    let left_length = left.norm();
    if left_length <= 1.0e-12 {
        return None;
    }
    let offset = Vector3::new(
        result_start.x - source_start.x,
        result_start.y - source_start.y,
        result_start.z - source_start.z,
    );
    Some((offset.x * left.x + offset.y * left.y + offset.z * left.z) / left_length)
}

/// Test whether two model-space points are reflections across a line carrier.
///
/// The line is unbounded for the reflection operation but its two stored
/// endpoints must define a finite, nondegenerate direction.
pub fn spatial_points_are_reflections(
    first: Point3,
    second: Point3,
    axis_start: Point3,
    axis_end: Point3,
) -> bool {
    let axis = Vector3::new(
        axis_end.x - axis_start.x,
        axis_end.y - axis_start.y,
        axis_end.z - axis_start.z,
    );
    let axis_length = axis.norm();
    if !axis_length.is_finite() || axis_length <= 1.0e-12 {
        return false;
    }
    let midpoint = Point3::new(
        0.5 * (first.x + second.x),
        0.5 * (first.y + second.y),
        0.5 * (first.z + second.z),
    );
    let from_axis = Vector3::new(
        midpoint.x - axis_start.x,
        midpoint.y - axis_start.y,
        midpoint.z - axis_start.z,
    );
    let separation = Vector3::new(second.x - first.x, second.y - first.y, second.z - first.z);
    let scale = 1.0
        + axis_length
            .max(from_axis.norm())
            .max(separation.norm())
            .max(first.x.abs())
            .max(first.y.abs())
            .max(first.z.abs())
            .max(second.x.abs())
            .max(second.y.abs())
            .max(second.z.abs());
    axis.cross(from_axis).norm() <= 1.0e-9 * axis_length * scale
        && axis.dot(separation).abs() <= 1.0e-9 * axis_length * scale
}

/// Recover native parameters for an analytic surface point.
pub fn analytic_surface_parameters(geometry: &SurfaceGeometry, point: Point3) -> Option<Point2> {
    let components = |origin: Point3, axis: Vector3, reference: Vector3| {
        let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        let transverse = cross(axis, reference);
        (
            delta.x * reference.x + delta.y * reference.y + delta.z * reference.z,
            delta.x * transverse.x + delta.y * transverse.y + delta.z * transverse.z,
            delta.x * axis.x + delta.y * axis.y + delta.z * axis.z,
        )
    };
    let result = match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let (u, v, _) = components(*origin, *normal, *u_axis);
            Point2::new(u, v)
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            if *radius == 0.0 {
                return None;
            }
            let (x, y, v) = components(*origin, *axis, *ref_direction);
            Point2::new((y / radius).atan2(x / radius), v)
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let (x, y, v) = components(*origin, *axis, *ref_direction);
            let local_radius = radius + v * half_angle.tan();
            if local_radius == 0.0 || *ratio == 0.0 {
                return None;
            }
            Point2::new((y / (local_radius * ratio)).atan2(x / local_radius), v)
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if *radius == 0.0 {
                return None;
            }
            let (x, y, z) = components(*center, *axis, *ref_direction);
            Point2::new(y.atan2(x), z.atan2(x.hypot(y)))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            if *minor_radius == 0.0 {
                return None;
            }
            let (x, y, z) = components(*center, *axis, *ref_direction);
            Point2::new(
                y.atan2(x),
                (z / minor_radius).atan2((x.hypot(y) - major_radius) / minor_radius),
            )
        }
        _ => return None,
    };
    (result.u.is_finite() && result.v.is_finite()).then_some(result)
}

/// `base + Σ factorᵢ · directionᵢ` in model space.
fn offset(base: Point3, terms: &[(f64, Vector3)]) -> Point3 {
    let mut out = base;
    for (factor, direction) in terms {
        out.x += factor * direction.x;
        out.y += factor * direction.y;
        out.z += factor * direction.z;
    }
    out
}

/// Knot span index of `t` for a clamped B-spline basis, or `None` when the
/// knot vector cannot support `count` poles of the given degree.
fn bspline_span(knots: &[f64], degree: usize, count: usize, t: f64) -> Option<usize> {
    if knots.len() < count + degree + 1 || count <= degree {
        return None;
    }
    if t >= knots[count] {
        return Some(count - 1);
    }
    if t <= knots[degree] {
        return Some(degree);
    }
    let mut lo = degree;
    let mut hi = count;
    while lo < hi {
        let mid = usize::midpoint(lo, hi);
        if t < knots[mid] {
            hi = mid;
        } else if t >= knots[mid + 1] {
            lo = mid + 1;
        } else {
            return Some(mid);
        }
    }
    Some(lo)
}

/// Non-zero basis function values at `t` for the given span (Cox–de Boor).
fn bspline_basis(knots: &[f64], degree: usize, span: usize, t: f64) -> Vec<f64> {
    let mut values = vec![1.0];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    for j in 1..=degree {
        left[j] = t - knots[span + 1 - j];
        right[j] = knots[span + j] - t;
        let mut saved = 0.0;
        let mut next = vec![0.0; j + 1];
        for (r, &value) in values.iter().enumerate().take(j) {
            let denominator = right[r + 1] + left[j - r];
            let factor = if denominator == 0.0 {
                0.0
            } else {
                value / denominator
            };
            next[r] = saved + right[r + 1] * factor;
            saved = left[j - r] * factor;
        }
        next[j] = saved;
        values = next;
    }
    values
}

fn bspline_basis_derivative(knots: &[f64], degree: usize, span: usize, t: f64) -> Vec<f64> {
    if degree == 0 {
        return vec![0.0];
    }
    let lower = bspline_basis(knots, degree - 1, span, t);
    let lower_start = span - (degree - 1);
    (0..=degree)
        .map(|local| {
            let index = span - degree + local;
            let lower_at = |global: usize| {
                global
                    .checked_sub(lower_start)
                    .and_then(|at| lower.get(at))
                    .copied()
                    .unwrap_or(0.0)
            };
            let left_denominator = knots[index + degree] - knots[index];
            let right_denominator = knots[index + degree + 1] - knots[index + 1];
            let left = if left_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index) / left_denominator
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index + 1) / right_denominator
            };
            left - right
        })
        .collect()
}

fn bspline_basis_second_derivative(knots: &[f64], degree: usize, span: usize, t: f64) -> Vec<f64> {
    if degree < 2 {
        return vec![0.0; degree + 1];
    }
    let lower = bspline_basis_derivative(knots, degree - 1, span, t);
    let lower_start = span - (degree - 1);
    (0..=degree)
        .map(|local| {
            let index = span - degree + local;
            let lower_at = |global: usize| {
                global
                    .checked_sub(lower_start)
                    .and_then(|at| lower.get(at))
                    .copied()
                    .unwrap_or(0.0)
            };
            let left_denominator = knots[index + degree] - knots[index];
            let right_denominator = knots[index + degree + 1] - knots[index + 1];
            let left = if left_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index) / left_denominator
            };
            let right = if right_denominator == 0.0 {
                0.0
            } else {
                degree as f64 * lower_at(index + 1) / right_denominator
            };
            left - right
        })
        .collect()
}

/// Evaluate a possibly-rational B-spline curve over 3D poles.
pub fn nurbs_curve_point(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Point3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t);
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut weight_sum = 0.0;
    for (i, value) in basis.iter().enumerate() {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        x += value * weight * pole.x;
        y += value * weight * pole.y;
        z += value * weight * pole.z;
        weight_sum += value * weight;
    }
    (weight_sum != 0.0).then(|| Point3::new(x / weight_sum, y / weight_sum, z / weight_sum))
}

/// Effective knot domain of a structurally evaluable NURBS curve.
pub fn nurbs_curve_parameter_domain(curve: &NurbsCurve) -> Option<[f64; 2]> {
    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    if count <= degree || curve.knots.len() < count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let lower = *curve.knots.get(degree)?;
    let upper = *curve.knots.get(count)?;
    (lower.is_finite() && upper.is_finite() && lower < upper).then_some([lower, upper])
}

/// Find a parameter witness whose NURBS curve point lies within `tolerance` of
/// `point`, searching finite knot spans in proximity to `seed`.
///
/// Interval rejection uses a rational-curve speed bound, so skipped intervals
/// cannot contain an admissible witness. The returned parameter is always
/// forward-evaluated within `tolerance`; `None` also covers malformed input or
/// exhaustion of the bounded certified search.
pub fn nurbs_curve_parameter_near_point(
    curve: &NurbsCurve,
    point: Point3,
    tolerance: f64,
    seed: f64,
) -> Option<f64> {
    const MAX_INTERVALS: usize = 100_000;

    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    let domain = nurbs_curve_parameter_domain(curve)?;
    if degree == 0
        || !tolerance.is_finite()
        || tolerance < 0.0
        || !seed.is_finite()
        || !point.x.is_finite()
        || !point.y.is_finite()
        || !point.z.is_finite()
    {
        return None;
    }
    let weights = validated_nurbs_curve_weights(curve)?;
    let speed_bound = nurbs_curve_speed_bound_about(curve, &weights, point)?;
    let distance = |parameter| {
        let position = nurbs_curve_point(
            curve.degree,
            &curve.knots,
            &curve.control_points,
            Some(&weights),
            parameter,
        )?;
        Some(
            ((position.x - point.x).powi(2)
                + (position.y - point.y).powi(2)
                + (position.z - point.z).powi(2))
            .sqrt(),
        )
    };
    let seed = seed.clamp(domain[0], domain[1]);
    let mut boundaries = curve.knots[degree..=count].to_vec();
    boundaries.sort_by(f64::total_cmp);
    boundaries.dedup();
    let mut boundary_witnesses = boundaries.clone();
    boundary_witnesses
        .sort_by(|first, second| (first - seed).abs().total_cmp(&(second - seed).abs()));
    for parameter in boundary_witnesses {
        if distance(parameter)? <= tolerance {
            return Some(parameter);
        }
    }
    let mut intervals = boundaries
        .windows(2)
        .filter_map(|pair| (pair[0] < pair[1]).then_some([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    intervals.sort_by(|first, second| {
        interval_distance_to_parameter(*second, seed)
            .total_cmp(&interval_distance_to_parameter(*first, seed))
    });
    let mut examined = 0usize;
    while let Some([start, end]) = intervals.pop() {
        examined += 1;
        if examined > MAX_INTERVALS {
            return None;
        }
        let middle = start + (end - start) * 0.5;
        let middle_distance = distance(middle)?;
        if middle_distance <= tolerance {
            return Some(middle);
        }
        if middle_distance - speed_bound * (end - start) * 0.5 > tolerance
            || middle == start
            || middle == end
        {
            continue;
        }
        let halves = [[start, middle], [middle, end]];
        let nearer = usize::from(
            interval_distance_to_parameter(halves[1], seed)
                < interval_distance_to_parameter(halves[0], seed),
        );
        intervals.push(halves[1 - nearer]);
        intervals.push(halves[nearer]);
    }
    None
}

/// Global model-space speed bound for a structurally valid rational NURBS
/// curve over its effective knot domain.
pub fn nurbs_curve_speed_bound(curve: &NurbsCurve) -> Option<f64> {
    let weights = validated_nurbs_curve_weights(curve)?;
    nurbs_curve_speed_bound_about(curve, &weights, Point3::new(0.0, 0.0, 0.0))
}

fn validated_nurbs_curve_weights(curve: &NurbsCurve) -> Option<Vec<f64>> {
    nurbs_curve_parameter_domain(curve)?;
    let count = curve.control_points.len();
    let weights = match &curve.weights {
        Some(weights) if weights.len() == count => weights.clone(),
        Some(_) => return None,
        None => vec![1.0; count],
    };
    if curve
        .control_points
        .iter()
        .zip(&weights)
        .any(|(control, weight)| {
            !control.x.is_finite()
                || !control.y.is_finite()
                || !control.z.is_finite()
                || !weight.is_finite()
                || *weight <= 0.0
        })
        || curve.knots.iter().any(|knot| !knot.is_finite())
        || curve.knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return None;
    }
    Some(weights)
}

fn nurbs_curve_speed_bound_about(
    curve: &NurbsCurve,
    weights: &[f64],
    origin: Point3,
) -> Option<f64> {
    let degree = usize::try_from(curve.degree).ok()?;
    let count = curve.control_points.len();
    let minimum_weight = weights.iter().copied().fold(f64::INFINITY, f64::min);
    let radius = |control: &Point3| {
        ((control.x - origin.x).powi(2)
            + (control.y - origin.y).powi(2)
            + (control.z - origin.z).powi(2))
        .sqrt()
    };
    let maximum_weighted_radius = curve
        .control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| weight * radius(control))
        .fold(0.0_f64, f64::max);
    let mut maximum_numerator_speed = 0.0_f64;
    let mut maximum_weight_speed = 0.0_f64;
    for index in 0..count - 1 {
        let denominator = curve.knots[index + degree + 1] - curve.knots[index + 1];
        if denominator == 0.0 {
            continue;
        }
        let factor = f64::from(curve.degree) / denominator;
        let first = curve.control_points[index];
        let second = curve.control_points[index + 1];
        let numerator_delta = Vector3::new(
            weights[index + 1] * (second.x - origin.x) - weights[index] * (first.x - origin.x),
            weights[index + 1] * (second.y - origin.y) - weights[index] * (first.y - origin.y),
            weights[index + 1] * (second.z - origin.z) - weights[index] * (first.z - origin.z),
        );
        maximum_numerator_speed = maximum_numerator_speed.max(factor * numerator_delta.norm());
        maximum_weight_speed =
            maximum_weight_speed.max(factor * (weights[index + 1] - weights[index]).abs());
    }
    let speed_bound = maximum_numerator_speed / minimum_weight
        + maximum_weighted_radius * maximum_weight_speed / minimum_weight.powi(2);
    speed_bound.is_finite().then_some(speed_bound)
}

fn interval_distance_to_parameter(interval: [f64; 2], parameter: f64) -> f64 {
    if parameter < interval[0] {
        interval[0] - parameter
    } else if parameter > interval[1] {
        parameter - interval[1]
    } else {
        0.0
    }
}

/// Map a NURBS parameter onto its evaluable knot branch.
///
/// Periodic parameters retain their serialized phase outside this operation
/// and are interpreted modulo the positive knot-domain period.
pub fn map_nurbs_curve_parameter(curve: &NurbsCurve, parameter: f64) -> Option<f64> {
    let [lower, upper] = nurbs_curve_parameter_domain(curve)?;
    if !parameter.is_finite() {
        return None;
    }
    if curve.periodic {
        let period = upper - lower;
        Some(lower + (parameter - lower).rem_euclid(period))
    } else {
        (lower..=upper).contains(&parameter).then_some(parameter)
    }
}

/// Evaluate a possibly-rational B-spline curve over 2D `(u, v)` poles.
pub fn nurbs_pcurve_uv(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Point2> {
    nurbs_pcurve_differential(degree, knots, control_points, weights, t)
        .map(|differential| differential.point)
}

struct PcurveDifferential {
    point: Point2,
    tangent: Option<Point2>,
    acceleration: Option<Point2>,
}

fn nurbs_pcurve_differential(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<PcurveDifferential> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t);
    let derivative = bspline_basis_derivative(knots, degree, span, t);
    let second_derivative = bspline_basis_second_derivative(knots, degree, span, t);
    let mut u = 0.0;
    let mut v = 0.0;
    let mut weight_sum = 0.0;
    let mut du = 0.0;
    let mut dv = 0.0;
    let mut weight_derivative = 0.0;
    let mut ddu = 0.0;
    let mut ddv = 0.0;
    let mut weight_second_derivative = 0.0;
    for i in 0..=degree {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        u += basis[i] * weight * pole.u;
        v += basis[i] * weight * pole.v;
        weight_sum += basis[i] * weight;
        du += derivative[i] * weight * pole.u;
        dv += derivative[i] * weight * pole.v;
        weight_derivative += derivative[i] * weight;
        ddu += second_derivative[i] * weight * pole.u;
        ddv += second_derivative[i] * weight * pole.v;
        weight_second_derivative += second_derivative[i] * weight;
    }
    if weight_sum == 0.0 {
        return None;
    }
    let point = Point2::new(u / weight_sum, v / weight_sum);
    let tangent = Point2::new(
        (du - point.u * weight_derivative) / weight_sum,
        (dv - point.v * weight_derivative) / weight_sum,
    );
    let acceleration = Point2::new(
        (ddu - point.u * weight_second_derivative - 2.0 * weight_derivative * tangent.u)
            / weight_sum,
        (ddv - point.v * weight_second_derivative - 2.0 * weight_derivative * tangent.v)
            / weight_sum,
    );
    if !point.u.is_finite() || !point.v.is_finite() {
        return None;
    }
    Some(PcurveDifferential {
        point,
        tangent: (tangent.u.is_finite() && tangent.v.is_finite()).then_some(tangent),
        acceleration: (acceleration.u.is_finite() && acceleration.v.is_finite())
            .then_some(acceleration),
    })
}

/// Return whether a point lies within `tolerance` of a nonperiodic NURBS
/// pcurve, using evaluated witnesses and Lipschitz-bounded interval rejection.
///
/// Positive rational weights make both the homogeneous curve and its
/// derivative convex combinations of their control polygons. Their norms
/// therefore bound Euclidean curve speed after the quotient rule. The search
/// accepts only an evaluated curve point within tolerance; intervals whose
/// midpoint distance minus the maximum possible travel exceeds tolerance are
/// discarded. `None` denotes invalid input or exhaustion of the bounded search.
pub fn nurbs_pcurve_contains_point(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    point: Point2,
    tolerance: f64,
) -> Option<bool> {
    const MAX_INTERVALS: usize = 100_000;

    let degree_usize = usize::try_from(degree).ok()?;
    let count = control_points.len();
    if degree_usize == 0
        || count <= degree_usize
        || knots.len() < count.checked_add(degree_usize)?.checked_add(1)?
        || !tolerance.is_finite()
        || tolerance < 0.0
        || !point.u.is_finite()
        || !point.v.is_finite()
    {
        return None;
    }
    let owned_weights;
    let weights = match weights {
        Some(weights) if weights.len() == count => weights,
        Some(_) => return None,
        None => {
            owned_weights = vec![1.0; count];
            &owned_weights
        }
    };
    if control_points.iter().zip(weights).any(|(control, weight)| {
        !control.u.is_finite() || !control.v.is_finite() || !weight.is_finite() || *weight <= 0.0
    }) || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return None;
    }

    let minimum_weight = weights.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum_weighted_radius = control_points
        .iter()
        .zip(weights)
        .map(|(control, weight)| weight * (control.u - point.u).hypot(control.v - point.v))
        .fold(0.0_f64, f64::max);
    let mut maximum_numerator_speed = 0.0_f64;
    let mut maximum_weight_speed = 0.0_f64;
    for index in 0..count - 1 {
        let denominator = knots[index + degree_usize + 1] - knots[index + 1];
        if denominator == 0.0 {
            continue;
        }
        let factor = f64::from(degree) / denominator;
        let first_u = weights[index] * (control_points[index].u - point.u);
        let first_v = weights[index] * (control_points[index].v - point.v);
        let second_u = weights[index + 1] * (control_points[index + 1].u - point.u);
        let second_v = weights[index + 1] * (control_points[index + 1].v - point.v);
        maximum_numerator_speed =
            maximum_numerator_speed.max(factor * (second_u - first_u).hypot(second_v - first_v));
        maximum_weight_speed =
            maximum_weight_speed.max(factor * (weights[index + 1] - weights[index]).abs());
    }
    let speed_bound = maximum_numerator_speed / minimum_weight
        + maximum_weighted_radius * maximum_weight_speed / minimum_weight.powi(2);
    if !speed_bound.is_finite() {
        return None;
    }

    let domain = [knots[degree_usize], knots[count]];
    if domain[0] > domain[1] {
        return None;
    }
    let mut intervals = knots[degree_usize..=count]
        .windows(2)
        .filter_map(|pair| (pair[0] < pair[1]).then_some([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    if intervals.is_empty() {
        intervals.push(domain);
    }
    let mut examined = 0usize;
    while let Some([start, end]) = intervals.pop() {
        examined += 1;
        if examined > MAX_INTERVALS {
            return None;
        }
        let middle = start + (end - start) * 0.5;
        let curve_point = nurbs_pcurve_uv(degree, knots, control_points, Some(weights), middle)?;
        let distance = (curve_point.u - point.u).hypot(curve_point.v - point.v);
        if distance <= tolerance {
            return Some(true);
        }
        let travel_bound = speed_bound * (end - start) * 0.5;
        if distance - travel_bound > tolerance {
            continue;
        }
        if middle == start || middle == end {
            continue;
        }
        intervals.push([start, middle]);
        intervals.push([middle, end]);
    }
    Some(false)
}

/// Evaluate a tensor-product NURBS surface at `(u, v)`.
pub fn nurbs_surface_point(surface: &NurbsSurface, u_at: f64, v_at: f64) -> Option<Point3> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)? {
        return None;
    }
    let u_at = periodic_parameter(
        &surface.u_knots,
        u_degree,
        u_count,
        surface.u_periodic,
        u_at,
    )?;
    let v_at = periodic_parameter(
        &surface.v_knots,
        v_degree,
        v_count,
        surface.v_periodic,
        v_at,
    )?;
    let u_span = bspline_span(&surface.u_knots, u_degree, u_count, u_at)?;
    let v_span = bspline_span(&surface.v_knots, v_degree, v_count, v_at)?;
    let u_basis = bspline_basis(&surface.u_knots, u_degree, u_span, u_at);
    let v_basis = bspline_basis(&surface.v_knots, v_degree, v_span, v_at);
    let mut x = 0.0;
    let mut y = 0.0;
    let mut z = 0.0;
    let mut weight_sum = 0.0;
    for (i, u_value) in u_basis.iter().enumerate() {
        for (j, v_value) in v_basis.iter().enumerate() {
            let index = (u_span - u_degree + i) * v_count + (v_span - v_degree + j);
            let weight = surface
                .weights
                .as_ref()
                .and_then(|weights| weights.get(index).copied())
                .unwrap_or(1.0);
            let factor = u_value * v_value * weight;
            let pole = surface.control_points.get(index)?;
            x += factor * pole.x;
            y += factor * pole.y;
            z += factor * pole.z;
            weight_sum += factor;
        }
    }
    (weight_sum != 0.0).then(|| Point3::new(x / weight_sum, y / weight_sum, z / weight_sum))
}

/// The parametric direction a surface isoline holds fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolineDirection {
    /// `u` is fixed; the curve runs along `v` in the surface's `v` parameter.
    ConstantU,
    /// `v` is fixed; the curve runs along `u` in the surface's `u` parameter.
    ConstantV,
}

/// The isoline of `surface` at `at` in `direction`, as an exact NURBS curve.
///
/// A tensor-product surface restricted to a constant parameter in one direction
/// is a NURBS curve of the free direction's degree over the free direction's
/// knot vector, whose poles are the fixed direction's pole rows blended by the
/// basis at `at`. The result is exact, not a fit; its parameter is the
/// surface's own parameter in the free direction.
pub fn nurbs_surface_isoline(
    surface: &NurbsSurface,
    direction: IsolineDirection,
    at: f64,
) -> Option<NurbsCurve> {
    let fixed_axis = match direction {
        IsolineDirection::ConstantU => SurfaceParameterAxis::U,
        IsolineDirection::ConstantV => SurfaceParameterAxis::V,
    };
    nurbs_surface_isocurve(surface, fixed_axis, at)
}

/// Extract the exact rational NURBS curve obtained by fixing one parameter of
/// a tensor-product NURBS surface.
pub fn nurbs_surface_isocurve(
    surface: &NurbsSurface,
    fixed_axis: SurfaceParameterAxis,
    fixed_parameter: f64,
) -> Option<NurbsCurve> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)?
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return None;
    }
    let (fixed_degree, fixed_count, fixed_knots, fixed_periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (u_degree, u_count, &surface.u_knots, surface.u_periodic),
        SurfaceParameterAxis::V => (v_degree, v_count, &surface.v_knots, surface.v_periodic),
    };
    let fixed_parameter = periodic_parameter(
        fixed_knots,
        fixed_degree,
        fixed_count,
        fixed_periodic,
        fixed_parameter,
    )?;
    let fixed_span = bspline_span(fixed_knots, fixed_degree, fixed_count, fixed_parameter)?;
    let fixed_basis = bspline_basis(fixed_knots, fixed_degree, fixed_span, fixed_parameter);
    let varying_count = match fixed_axis {
        SurfaceParameterAxis::U => v_count,
        SurfaceParameterAxis::V => u_count,
    };
    let mut control_points = Vec::with_capacity(varying_count);
    let mut derived_weights = Vec::with_capacity(varying_count);
    for varying in 0..varying_count {
        let mut weighted = [0.0; 3];
        let mut weight_sum = 0.0;
        for (local, basis) in fixed_basis.iter().copied().enumerate() {
            let fixed = fixed_span - fixed_degree + local;
            let index = match fixed_axis {
                SurfaceParameterAxis::U => fixed * v_count + varying,
                SurfaceParameterAxis::V => varying * v_count + fixed,
            };
            let weight = surface
                .weights
                .as_ref()
                .and_then(|weights| weights.get(index).copied())
                .unwrap_or(1.0);
            let factor = basis * weight;
            let point = surface.control_points.get(index)?;
            weighted[0] += factor * point.x;
            weighted[1] += factor * point.y;
            weighted[2] += factor * point.z;
            weight_sum += factor;
        }
        if !weight_sum.is_finite() || weight_sum <= 0.0 {
            return None;
        }
        control_points.push(Point3::new(
            weighted[0] / weight_sum,
            weighted[1] / weight_sum,
            weighted[2] / weight_sum,
        ));
        derived_weights.push(weight_sum);
    }
    let (degree, knots, periodic) = match fixed_axis {
        SurfaceParameterAxis::U => (
            surface.v_degree,
            surface.v_knots.clone(),
            surface.v_periodic,
        ),
        SurfaceParameterAxis::V => (
            surface.u_degree,
            surface.u_knots.clone(),
            surface.u_periodic,
        ),
    };
    Some(NurbsCurve {
        degree,
        knots,
        control_points,
        weights: surface.weights.as_ref().map(|_| derived_weights),
        periodic,
    })
}

/// Point and first partial derivatives of a NURBS surface in its stored
/// parameterization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePartials {
    /// Surface point at `(u, v)`.
    pub point: Point3,
    /// First partial derivative with respect to `u`.
    pub du: Vector3,
    /// First partial derivative with respect to `v`.
    pub dv: Vector3,
}

/// Point, first partials, and second partials of a surface in its stored
/// parameterization.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSecondPartials {
    /// Surface point at `(u, v)`.
    pub point: Point3,
    /// First partial derivative with respect to `u`.
    pub du: Vector3,
    /// First partial derivative with respect to `v`.
    pub dv: Vector3,
    /// Second partial derivative with respect to `u`.
    pub duu: Vector3,
    /// Mixed partial derivative.
    pub duv: Vector3,
    /// Second partial derivative with respect to `v`.
    pub dvv: Vector3,
}

/// Evaluate a tensor-product NURBS surface and its exact rational first
/// partials at `(u, v)`.
pub fn nurbs_surface_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<SurfacePartials> {
    nurbs_surface_second_partials(surface, u_at, v_at).map(|partials| SurfacePartials {
        point: partials.point,
        du: partials.du,
        dv: partials.dv,
    })
}

/// Evaluate a tensor-product NURBS surface and its exact rational first and
/// second partials at `(u, v)`.
pub fn nurbs_surface_second_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<SurfaceSecondPartials> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)?
        || surface
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != surface.control_points.len())
    {
        return None;
    }
    let u_at = periodic_parameter(
        &surface.u_knots,
        u_degree,
        u_count,
        surface.u_periodic,
        u_at,
    )?;
    let v_at = periodic_parameter(
        &surface.v_knots,
        v_degree,
        v_count,
        surface.v_periodic,
        v_at,
    )?;
    let u_span = bspline_span(&surface.u_knots, u_degree, u_count, u_at)?;
    let v_span = bspline_span(&surface.v_knots, v_degree, v_count, v_at)?;
    let u_basis = bspline_basis(&surface.u_knots, u_degree, u_span, u_at);
    let v_basis = bspline_basis(&surface.v_knots, v_degree, v_span, v_at);
    let u_derivative = bspline_basis_derivative(&surface.u_knots, u_degree, u_span, u_at);
    let v_derivative = bspline_basis_derivative(&surface.v_knots, v_degree, v_span, v_at);
    let u_second = bspline_basis_second_derivative(&surface.u_knots, u_degree, u_span, u_at);
    let v_second = bspline_basis_second_derivative(&surface.v_knots, v_degree, v_span, v_at);
    let mut weighted = [0.0; 3];
    let mut weighted_u = [0.0; 3];
    let mut weighted_v = [0.0; 3];
    let mut weighted_uu = [0.0; 3];
    let mut weighted_uv = [0.0; 3];
    let mut weighted_vv = [0.0; 3];
    let mut weight = 0.0;
    let mut weight_u = 0.0;
    let mut weight_v = 0.0;
    let mut weight_uu = 0.0;
    let mut weight_uv = 0.0;
    let mut weight_vv = 0.0;
    for i in 0..=u_degree {
        for j in 0..=v_degree {
            let index = (u_span - u_degree + i) * v_count + (v_span - v_degree + j);
            let pole = surface.control_points.get(index)?;
            let pole_weight = surface
                .weights
                .as_ref()
                .map_or(1.0, |weights| weights[index]);
            let basis = u_basis[i] * v_basis[j] * pole_weight;
            let basis_u = u_derivative[i] * v_basis[j] * pole_weight;
            let basis_v = u_basis[i] * v_derivative[j] * pole_weight;
            let basis_uu = u_second[i] * v_basis[j] * pole_weight;
            let basis_uv = u_derivative[i] * v_derivative[j] * pole_weight;
            let basis_vv = u_basis[i] * v_second[j] * pole_weight;
            for (axis, coordinate) in [pole.x, pole.y, pole.z].into_iter().enumerate() {
                weighted[axis] += basis * coordinate;
                weighted_u[axis] += basis_u * coordinate;
                weighted_v[axis] += basis_v * coordinate;
                weighted_uu[axis] += basis_uu * coordinate;
                weighted_uv[axis] += basis_uv * coordinate;
                weighted_vv[axis] += basis_vv * coordinate;
            }
            weight += basis;
            weight_u += basis_u;
            weight_v += basis_v;
            weight_uu += basis_uu;
            weight_uv += basis_uv;
            weight_vv += basis_vv;
        }
    }
    if weight == 0.0 {
        return None;
    }
    let point = Point3::new(
        weighted[0] / weight,
        weighted[1] / weight,
        weighted[2] / weight,
    );
    let derivative = |weighted_derivative: [f64; 3], weight_derivative: f64| {
        Vector3::new(
            (weighted_derivative[0] - point.x * weight_derivative) / weight,
            (weighted_derivative[1] - point.y * weight_derivative) / weight,
            (weighted_derivative[2] - point.z * weight_derivative) / weight,
        )
    };
    let du = derivative(weighted_u, weight_u);
    let dv = derivative(weighted_v, weight_v);
    let second_derivative = |weighted_derivative: [f64; 3],
                             weight_derivative: f64,
                             first_weight: f64,
                             first: Vector3| {
        Vector3::new(
            (weighted_derivative[0] - point.x * weight_derivative - 2.0 * first_weight * first.x)
                / weight,
            (weighted_derivative[1] - point.y * weight_derivative - 2.0 * first_weight * first.y)
                / weight,
            (weighted_derivative[2] - point.z * weight_derivative - 2.0 * first_weight * first.z)
                / weight,
        )
    };
    let mixed_derivative = Vector3::new(
        (weighted_uv[0] - point.x * weight_uv - weight_u * dv.x - weight_v * du.x) / weight,
        (weighted_uv[1] - point.y * weight_uv - weight_u * dv.y - weight_v * du.y) / weight,
        (weighted_uv[2] - point.z * weight_uv - weight_u * dv.z - weight_v * du.z) / weight,
    );
    Some(SurfaceSecondPartials {
        point,
        du,
        dv,
        duu: second_derivative(weighted_uu, weight_uu, weight_u, du),
        duv: mixed_derivative,
        dvv: second_derivative(weighted_vv, weight_vv, weight_v, dv),
    })
}

fn periodic_parameter(
    knots: &[f64],
    degree: usize,
    count: usize,
    periodic: bool,
    parameter: f64,
) -> Option<f64> {
    parameter.is_finite().then_some(())?;
    let start = *knots.get(degree)?;
    let end = *knots.get(count)?;
    if !periodic || (start..=end).contains(&parameter) {
        return Some(parameter);
    }
    let period = end - start;
    (period.is_finite() && period > 0.0).then(|| start + (parameter - start).rem_euclid(period))
}

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization.
pub fn curve_point(geometry: &CurveGeometry, t: f64) -> Option<Point3> {
    curve_point_inner(geometry, t, 0)
}

/// Evaluate the exact first derivative of a directly stored curve.
pub fn curve_tangent(geometry: &CurveGeometry, t: f64) -> Option<Vector3> {
    if !t.is_finite() {
        return None;
    }
    curve_tangent_inner(geometry, t, 0)
        .filter(|tangent| tangent.x.is_finite() && tangent.y.is_finite() && tangent.z.is_finite())
}

/// Evaluate the exact second derivative of a directly stored curve.
pub fn curve_second_derivative(geometry: &CurveGeometry, t: f64) -> Option<Vector3> {
    if !t.is_finite() {
        return None;
    }
    curve_second_derivative_inner(geometry, t, 0).filter(|derivative| {
        derivative.x.is_finite() && derivative.y.is_finite() && derivative.z.is_finite()
    })
}

fn curve_tangent_inner(geometry: &CurveGeometry, t: f64, depth: usize) -> Option<Vector3> {
    if depth > 256 {
        return None;
    }
    match geometry {
        CurveGeometry::Line { direction, .. } => Some(*direction),
        CurveGeometry::Circle {
            axis,
            ref_direction,
            radius,
            ..
        } => Some(vector_sum(&[
            (-radius * t.sin(), *ref_direction),
            (radius * t.cos(), cross(*axis, *ref_direction)),
        ])),
        CurveGeometry::Ellipse {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (-major_radius * t.sin(), *major_direction),
            (minor_radius * t.cos(), cross(*axis, *major_direction)),
        ])),
        CurveGeometry::Parabola {
            axis,
            major_direction,
            focal_distance,
            ..
        } => Some(vector_sum(&[
            (2.0 * focal_distance * t, *major_direction),
            (2.0 * focal_distance, cross(*axis, *major_direction)),
        ])),
        CurveGeometry::Hyperbola {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (major_radius * t.sinh(), *major_direction),
            (minor_radius * t.cosh(), cross(*axis, *major_direction)),
        ])),
        CurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?;
            nurbs_curve_tangent(
                nurbs.degree,
                &nurbs.knots,
                &nurbs.control_points,
                nurbs.weights.as_deref(),
                parameter,
            )
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_tangent(points, parameters.as_deref(), t),
        CurveGeometry::Transformed { basis, transform } => curve_tangent_inner(basis, t, depth + 1)
            .map(|tangent| affine_vector(*transform, tangent)),
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

fn curve_second_derivative_inner(
    geometry: &CurveGeometry,
    t: f64,
    depth: usize,
) -> Option<Vector3> {
    if depth > 256 {
        return None;
    }
    let zero = Vector3::new(0.0, 0.0, 0.0);
    match geometry {
        CurveGeometry::Line { .. } => Some(zero),
        CurveGeometry::Circle {
            axis,
            ref_direction,
            radius,
            ..
        } => Some(vector_sum(&[
            (-radius * t.cos(), *ref_direction),
            (-radius * t.sin(), cross(*axis, *ref_direction)),
        ])),
        CurveGeometry::Ellipse {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (-major_radius * t.cos(), *major_direction),
            (-minor_radius * t.sin(), cross(*axis, *major_direction)),
        ])),
        CurveGeometry::Parabola {
            major_direction,
            focal_distance,
            ..
        } => Some(vector_sum(&[(2.0 * focal_distance, *major_direction)])),
        CurveGeometry::Hyperbola {
            axis,
            major_direction,
            major_radius,
            minor_radius,
            ..
        } => Some(vector_sum(&[
            (major_radius * t.cosh(), *major_direction),
            (minor_radius * t.sinh(), cross(*axis, *major_direction)),
        ])),
        CurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?;
            nurbs_curve_second_derivative(
                nurbs.degree,
                &nurbs.knots,
                &nurbs.control_points,
                nurbs.weights.as_deref(),
                parameter,
            )
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_tangent(points, parameters.as_deref(), t).map(|_| zero),
        CurveGeometry::Transformed { basis, transform } => {
            curve_second_derivative_inner(basis, t, depth + 1)
                .map(|derivative| affine_vector(*transform, derivative))
        }
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

fn nurbs_curve_tangent(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Vector3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t);
    let derivatives = bspline_basis_derivative(knots, degree, span, t);
    let mut weighted = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_derivative = Vector3::new(0.0, 0.0, 0.0);
    let mut weight = 0.0;
    let mut weight_derivative = 0.0;
    for (local, (basis, derivative)) in basis.iter().zip(&derivatives).enumerate() {
        let index = span - degree + local;
        let control = control_points.get(index)?;
        let control_weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        weighted.x += basis * control_weight * control.x;
        weighted.y += basis * control_weight * control.y;
        weighted.z += basis * control_weight * control.z;
        weighted_derivative.x += derivative * control_weight * control.x;
        weighted_derivative.y += derivative * control_weight * control.y;
        weighted_derivative.z += derivative * control_weight * control.z;
        weight += basis * control_weight;
        weight_derivative += derivative * control_weight;
    }
    if weight == 0.0 {
        return None;
    }
    let tangent = Vector3::new(
        (weighted_derivative.x * weight - weighted.x * weight_derivative) / (weight * weight),
        (weighted_derivative.y * weight - weighted.y * weight_derivative) / (weight * weight),
        (weighted_derivative.z * weight - weighted.z * weight_derivative) / (weight * weight),
    );
    (tangent.x.is_finite() && tangent.y.is_finite() && tangent.z.is_finite()).then_some(tangent)
}

fn nurbs_curve_second_derivative(
    degree: u32,
    knots: &[f64],
    control_points: &[Point3],
    weights: Option<&[f64]>,
    t: f64,
) -> Option<Vector3> {
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t);
    let first_basis = bspline_basis_derivative(knots, degree, span, t);
    let second_basis = bspline_basis_second_derivative(knots, degree, span, t);
    let mut weighted = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_first = Vector3::new(0.0, 0.0, 0.0);
    let mut weighted_second = Vector3::new(0.0, 0.0, 0.0);
    let mut weight = 0.0;
    let mut weight_first = 0.0;
    let mut weight_second = 0.0;
    for local in 0..=degree {
        let index = span - degree + local;
        let control = control_points.get(index)?;
        let control_weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let accumulate = |target: &mut Vector3, factor: f64| {
            target.x += factor * control.x;
            target.y += factor * control.y;
            target.z += factor * control.z;
        };
        let basis = basis[local] * control_weight;
        let first = first_basis[local] * control_weight;
        let second = second_basis[local] * control_weight;
        accumulate(&mut weighted, basis);
        accumulate(&mut weighted_first, first);
        accumulate(&mut weighted_second, second);
        weight += basis;
        weight_first += first;
        weight_second += second;
    }
    if weight == 0.0 {
        return None;
    }
    let point = Vector3::new(
        weighted.x / weight,
        weighted.y / weight,
        weighted.z / weight,
    );
    let first = Vector3::new(
        (weighted_first.x - point.x * weight_first) / weight,
        (weighted_first.y - point.y * weight_first) / weight,
        (weighted_first.z - point.z * weight_first) / weight,
    );
    Some(Vector3::new(
        (weighted_second.x - point.x * weight_second - 2.0 * weight_first * first.x) / weight,
        (weighted_second.y - point.y * weight_second - 2.0 * weight_first * first.y) / weight,
        (weighted_second.z - point.z * weight_second - 2.0 * weight_first * first.z) / weight,
    ))
}

/// Evaluate a curve carrier selected by arena id, including supported
/// procedural constructions.
pub fn model_curve_point_by_id(
    index: &crate::index::ModelIndex<'_>,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Option<Point3> {
    let curve = index.curves(&curve_id.0)?;
    let CurveGeometry::Procedural { construction } = &curve.geometry else {
        return curve_point(&curve.geometry, parameter);
    };
    let procedural = index.procedural_curves(&construction.0)?;
    if procedural.curve != *curve_id {
        return None;
    }
    let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
        supports,
        tolerance,
        parameterization: Some(parameterization),
        ..
    } = &procedural.definition
    else {
        return None;
    };
    let parameter_range = parameterization.parameter_range;
    if !parameter.is_finite() || parameter < parameter_range[0] || parameter > parameter_range[1] {
        return None;
    }
    let points = std::array::from_fn(|side| {
        let uv = pcurve_uv(&parameterization.pcurves[side], parameter)?;
        model_surface_point_by_id(index, &supports[side], uv.u, uv.v)
    });
    let [Some(first), Some(second)] = points else {
        return None;
    };
    let separation = ((first.x - second.x).powi(2)
        + (first.y - second.y).powi(2)
        + (first.z - second.z).powi(2))
    .sqrt();
    (separation.is_finite() && separation <= *tolerance).then_some(first)
}

/// Invert a model curve near a caller-selected branch parameter.
///
/// Direct analytic and NURBS carriers preserve their native parameterization.
/// Charted tolerant intersections invert a support chart. The seed selects
/// between repeated model-space points. The returned parameter is
/// forward-validated against the direct carrier or complete two-support
/// construction.
pub fn model_curve_parameter_near_point(
    ir: &CadIr,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
) -> Option<f64> {
    let index = crate::index::ModelIndex::new(ir);
    let curve = index.curves(&curve_id.0)?;
    if !matches!(&curve.geometry, CurveGeometry::Procedural { .. }) {
        return direct_curve_parameter_near_point(
            &curve.geometry,
            point,
            seed,
            ir.tolerances.linear,
        );
    }
    let CurveGeometry::Procedural { construction } = &curve.geometry else {
        unreachable!("direct carriers return before procedural inversion");
    };
    let procedural = index.procedural_curves(&construction.0)?;
    if procedural.curve != *curve_id {
        return None;
    }
    let crate::geometry::ProceduralCurveDefinition::TolerantIntersection {
        supports,
        tolerance,
        parameterization: Some(parameterization),
        ..
    } = &procedural.definition
    else {
        return None;
    };
    let range = parameterization.parameter_range;
    if !seed.is_finite() || seed < range[0] || seed > range[1] {
        return None;
    }
    let mut candidates = Vec::new();
    for (support_id, pcurve) in supports.iter().zip(&parameterization.pcurves) {
        let Some(surface) = index.surfaces(&support_id.0) else {
            continue;
        };
        let PcurveGeometry::Line { origin, direction } = pcurve else {
            continue;
        };
        let parameter = match &surface.geometry {
            SurfaceGeometry::Plane { .. } => {
                let Some(base) = model_surface_point_by_id(&index, support_id, origin.u, origin.v)
                else {
                    continue;
                };
                let Some(next) = model_surface_point_by_id(
                    &index,
                    support_id,
                    origin.u + direction.u,
                    origin.v + direction.v,
                ) else {
                    continue;
                };
                let tangent = Vector3::new(next.x - base.x, next.y - base.y, next.z - base.z);
                let offset = Vector3::new(point.x - base.x, point.y - base.y, point.z - base.z);
                let denominator = tangent.dot(tangent);
                (denominator.is_finite() && denominator > 0.0)
                    .then(|| offset.dot(tangent) / denominator)
            }
            SurfaceGeometry::Cylinder { .. }
            | SurfaceGeometry::Cone { .. }
            | SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. } => {
                analytic_surface_parameters(&surface.geometry, point).and_then(|mut uv| {
                    if direction.v == 0.0 && direction.u != 0.0 {
                        let expected = origin.u + direction.u * seed;
                        uv.u += ((expected - uv.u) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU;
                        Some((uv.u - origin.u) / direction.u)
                    } else if direction.u == 0.0
                        && direction.v != 0.0
                        && matches!(&surface.geometry, SurfaceGeometry::Torus { .. })
                    {
                        let expected = origin.v + direction.v * seed;
                        uv.v += ((expected - uv.v) / std::f64::consts::TAU).round()
                            * std::f64::consts::TAU;
                        Some((uv.v - origin.v) / direction.v)
                    } else if direction.u == 0.0 && direction.v != 0.0 {
                        Some((uv.v - origin.v) / direction.v)
                    } else {
                        None
                    }
                })
            }
            SurfaceGeometry::Nurbs(surface) => {
                let (fixed_axis, fixed_parameter, varying_origin, varying_scale) =
                    if direction.u == 0.0 && direction.v != 0.0 {
                        (SurfaceParameterAxis::U, origin.u, origin.v, direction.v)
                    } else if direction.v == 0.0 && direction.u != 0.0 {
                        (SurfaceParameterAxis::V, origin.v, origin.u, direction.u)
                    } else {
                        continue;
                    };
                let Some(isocurve) = nurbs_surface_isocurve(surface, fixed_axis, fixed_parameter)
                else {
                    continue;
                };
                let isocurve_seed = varying_origin + varying_scale * seed;
                nurbs_curve_parameter_near_point(&isocurve, point, *tolerance, isocurve_seed)
                    .map(|parameter| (parameter - varying_origin) / varying_scale)
            }
            _ => continue,
        };
        let Some(mut parameter) = parameter else {
            continue;
        };
        let endpoint_tolerance = 1.0e-12 * (1.0 + range[0].abs().max(range[1].abs()));
        if parameter < range[0] && range[0] - parameter <= endpoint_tolerance {
            parameter = range[0];
        } else if parameter > range[1] && parameter - range[1] <= endpoint_tolerance {
            parameter = range[1];
        } else if parameter < range[0] || parameter > range[1] {
            continue;
        }
        let Some(evaluated) = model_curve_point_by_id(&index, curve_id, parameter) else {
            continue;
        };
        let distance = ((evaluated.x - point.x).powi(2)
            + (evaluated.y - point.y).powi(2)
            + (evaluated.z - point.z).powi(2))
        .sqrt();
        if distance.is_finite() && distance <= *tolerance {
            candidates.push(parameter);
        }
    }
    candidates
        .into_iter()
        .min_by(|first, second| (first - seed).abs().total_cmp(&(second - seed).abs()))
}

fn direct_curve_parameter_near_point(
    geometry: &CurveGeometry,
    point: Point3,
    seed: f64,
    tolerance: f64,
) -> Option<f64> {
    if !seed.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
        return None;
    }
    let components = |origin: Point3, axis: Vector3, reference: Vector3| -> (f64, f64, f64) {
        let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        let transverse = cross(axis, reference);
        (delta.dot(reference), delta.dot(transverse), delta.dot(axis))
    };
    let parameter = match geometry {
        CurveGeometry::Line { origin, direction } => {
            let delta = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
            let denominator = direction.dot(*direction);
            (denominator.is_finite() && denominator > 0.0)
                .then(|| delta.dot(*direction) / denominator)?
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if *radius == 0.0 {
                return None;
            }
            let (x, y, _) = components(*center, *axis, *ref_direction);
            let canonical = (y / radius).atan2(x / radius);
            canonical + ((seed - canonical) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            if *major_radius == 0.0 || *minor_radius == 0.0 {
                return None;
            }
            let (x, y, _) = components(*center, *axis, *major_direction);
            let canonical = (y / minor_radius).atan2(x / major_radius);
            canonical + ((seed - canonical) / std::f64::consts::TAU).round() * std::f64::consts::TAU
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            if *focal_distance == 0.0 {
                return None;
            }
            let (_, transverse, _) = components(*vertex, *axis, *major_direction);
            transverse / (2.0 * focal_distance)
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            minor_radius,
            ..
        } => {
            if *minor_radius == 0.0 {
                return None;
            }
            let (_, transverse, _) = components(*center, *axis, *major_direction);
            (transverse / minor_radius).asinh()
        }
        CurveGeometry::Nurbs(curve) => {
            nurbs_curve_parameter_near_point(curve, point, tolerance, seed)?
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_parameter_near_point(points, parameters.as_deref(), point, tolerance, seed)?,
        CurveGeometry::Transformed { basis, transform } => {
            let (basis_point, tolerance_scale) = inverse_affine_point(*transform, point)?;
            let basis_tolerance = tolerance * tolerance_scale;
            if !basis_tolerance.is_finite() {
                return None;
            }
            direct_curve_parameter_near_point(basis, basis_point, seed, basis_tolerance)?
        }
        CurveGeometry::Degenerate { point: stored } => {
            let error = (stored.x - point.x)
                .hypot(stored.y - point.y)
                .hypot(stored.z - point.z);
            (error.is_finite() && error <= tolerance).then_some(seed)?
        }
        CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => return None,
    };
    let evaluated = curve_point(geometry, parameter)?;
    let error = ((evaluated.x - point.x).powi(2)
        + (evaluated.y - point.y).powi(2)
        + (evaluated.z - point.z).powi(2))
    .sqrt();
    (parameter.is_finite() && error.is_finite() && error <= tolerance).then_some(parameter)
}

fn inverse_affine_point(transform: Transform, point: Point3) -> Option<(Point3, f64)> {
    let [first, second, third, bottom] = transform.rows;
    let [matrix_00, matrix_01, matrix_02, translate_x] = first;
    let [matrix_10, matrix_11, matrix_12, translate_y] = second;
    let [matrix_20, matrix_21, matrix_22, translate_z] = third;
    if bottom != [0.0, 0.0, 0.0, 1.0] {
        return None;
    }
    let cofactors = [
        [
            matrix_11 * matrix_22 - matrix_12 * matrix_21,
            matrix_02 * matrix_21 - matrix_01 * matrix_22,
            matrix_01 * matrix_12 - matrix_02 * matrix_11,
        ],
        [
            matrix_12 * matrix_20 - matrix_10 * matrix_22,
            matrix_00 * matrix_22 - matrix_02 * matrix_20,
            matrix_02 * matrix_10 - matrix_00 * matrix_12,
        ],
        [
            matrix_10 * matrix_21 - matrix_11 * matrix_20,
            matrix_01 * matrix_20 - matrix_00 * matrix_21,
            matrix_00 * matrix_11 - matrix_01 * matrix_10,
        ],
    ];
    let determinant =
        matrix_00 * cofactors[0][0] + matrix_01 * cofactors[1][0] + matrix_02 * cofactors[2][0];
    if !determinant.is_finite() || determinant == 0.0 {
        return None;
    }
    let inverse = cofactors.map(|row| row.map(|value| value / determinant));
    if inverse.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    let relative = [
        point.x - translate_x,
        point.y - translate_y,
        point.z - translate_z,
    ];
    let coordinates = inverse.map(|row| {
        row.into_iter()
            .zip(relative)
            .map(|(coefficient, coordinate)| coefficient * coordinate)
            .sum::<f64>()
    });
    let tolerance_scale = inverse
        .iter()
        .flatten()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    (coordinates
        .into_iter()
        .chain([tolerance_scale])
        .all(f64::is_finite))
    .then_some((
        Point3::new(coordinates[0], coordinates[1], coordinates[2]),
        tolerance_scale,
    ))
}

fn polyline_parameter_near_point(
    points: &[Point3],
    parameters: Option<&[f64]>,
    point: Point3,
    tolerance: f64,
    seed: f64,
) -> Option<f64> {
    if points.len() < 2 {
        return None;
    }
    let implicit;
    let parameters = if let Some(parameters) = parameters {
        (parameters.len() == points.len()).then_some(parameters)?
    } else {
        implicit = (0..points.len())
            .map(|index| index as f64)
            .collect::<Vec<_>>();
        &implicit
    };
    let mut candidates = Vec::new();
    for (segment, parameter_range) in parameters.windows(2).enumerate() {
        let [parameter_start, parameter_end] = [parameter_range[0], parameter_range[1]];
        let parameter_width = parameter_end - parameter_start;
        if !parameter_start.is_finite() || !parameter_end.is_finite() || parameter_width == 0.0 {
            continue;
        }
        let start = points[segment];
        let end = points[segment + 1];
        let direction = Vector3::new(end.x - start.x, end.y - start.y, end.z - start.z);
        let offset = Vector3::new(point.x - start.x, point.y - start.y, point.z - start.z);
        let length = direction.x.hypot(direction.y).hypot(direction.z);
        if !length.is_finite() {
            continue;
        }
        let fraction = if length == 0.0 {
            if offset.x.hypot(offset.y).hypot(offset.z) > tolerance {
                continue;
            }
            ((seed - parameter_start) / parameter_width).clamp(0.0, 1.0)
        } else {
            let unit = Vector3::new(
                direction.x / length,
                direction.y / length,
                direction.z / length,
            );
            (offset.dot(unit) / length).clamp(0.0, 1.0)
        };
        let candidate = parameter_start + fraction * parameter_width;
        let mapped = Point3::new(
            start.x + fraction * direction.x,
            start.y + fraction * direction.y,
            start.z + fraction * direction.z,
        );
        let error = (mapped.x - point.x)
            .hypot(mapped.y - point.y)
            .hypot(mapped.z - point.z);
        if candidate.is_finite() && error.is_finite() && error <= tolerance {
            candidates.push(candidate);
        }
    }
    candidates
        .into_iter()
        .min_by(|first, second| (first - seed).abs().total_cmp(&(second - seed).abs()))
}

fn curve_point_inner(geometry: &CurveGeometry, t: f64, depth: usize) -> Option<Point3> {
    if depth > 256 {
        return None;
    }
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(offset(*origin, &[(t, *direction)])),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(offset(
            *center,
            &[
                (radius * t.cos(), *ref_direction),
                (radius * t.sin(), cross(*axis, *ref_direction)),
            ],
        )),
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => Some(offset(
            *center,
            &[
                (major_radius * t.cos(), *major_direction),
                (minor_radius * t.sin(), cross(*axis, *major_direction)),
            ],
        )),
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => Some(offset(
            *vertex,
            &[
                (focal_distance * t * t, *major_direction),
                (2.0 * focal_distance * t, cross(*axis, *major_direction)),
            ],
        )),
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => Some(offset(
            *center,
            &[
                (major_radius * t.cosh(), *major_direction),
                (minor_radius * t.sinh(), cross(*axis, *major_direction)),
            ],
        )),
        CurveGeometry::Degenerate { point } => Some(*point),
        CurveGeometry::Nurbs(nurbs) => {
            let parameter = map_nurbs_curve_parameter(nurbs, t)?;
            nurbs_curve_point(
                nurbs.degree,
                &nurbs.knots,
                &nurbs.control_points,
                nurbs.weights.as_deref(),
                parameter,
            )
        }
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_point(points, parameters.as_deref(), t),
        CurveGeometry::Transformed { basis, transform } => {
            curve_point_inner(basis, t, depth + 1).map(|point| affine_point(*transform, point))
        }
        CurveGeometry::Procedural { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization: `u` is
/// the azimuth angle and `v` the axial distance / polar angle on analytic
/// quadrics, and both are knot-domain parameters on NURBS surfaces.
pub fn surface_point(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<Point3> {
    surface_second_partials_inner(geometry, u, v, 0).map(|partials| partials.point)
}

/// Evaluate a directly stored surface and its exact first partial derivatives.
pub fn surface_partials(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<SurfacePartials> {
    surface_second_partials_inner(geometry, u, v, 0).map(|partials| SurfacePartials {
        point: partials.point,
        du: partials.du,
        dv: partials.dv,
    })
}

/// Evaluate a directly stored surface and its exact first and second partial
/// derivatives.
pub fn surface_second_partials(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<SurfaceSecondPartials> {
    surface_second_partials_inner(geometry, u, v, 0)
}

fn surface_second_partials_inner(
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
    depth: usize,
) -> Option<SurfaceSecondPartials> {
    if depth > 256 {
        return None;
    }
    let zero = Vector3::new(0.0, 0.0, 0.0);
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let v_axis = cross(*normal, *u_axis);
            Some(SurfaceSecondPartials {
                point: offset(*origin, &[(u, *u_axis), (v, v_axis)]),
                du: *u_axis,
                dv: v_axis,
                duu: zero,
                duv: zero,
                dvv: zero,
            })
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            let transverse = cross(*axis, *ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    *origin,
                    &[
                        (radius * cosine, *ref_direction),
                        (radius * sine, transverse),
                        (v, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-radius * sine, *ref_direction),
                    (radius * cosine, transverse),
                ]),
                dv: *axis,
                duu: vector_sum(&[
                    (-radius * cosine, *ref_direction),
                    (-radius * sine, transverse),
                ]),
                duv: zero,
                dvv: zero,
            })
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let transverse = cross(*axis, *ref_direction);
            let cosine = u.cos();
            let sine = u.sin();
            let radial_slope = half_angle.tan();
            let local_radius = radius + v * radial_slope;
            Some(SurfaceSecondPartials {
                point: offset(
                    *origin,
                    &[
                        (local_radius * cosine, *ref_direction),
                        (local_radius * ratio * sine, transverse),
                        (v, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-local_radius * sine, *ref_direction),
                    (local_radius * ratio * cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (radial_slope * cosine, *ref_direction),
                    (radial_slope * ratio * sine, transverse),
                    (1.0, *axis),
                ]),
                duu: vector_sum(&[
                    (-local_radius * cosine, *ref_direction),
                    (-local_radius * ratio * sine, transverse),
                ]),
                duv: vector_sum(&[
                    (-radial_slope * sine, *ref_direction),
                    (radial_slope * ratio * cosine, transverse),
                ]),
                dvv: zero,
            })
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let transverse = cross(*axis, *ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            Some(SurfaceSecondPartials {
                point: offset(
                    *center,
                    &[
                        (radius * v_cosine * u_cosine, *ref_direction),
                        (radius * v_cosine * u_sine, transverse),
                        (radius * v_sine, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-radius * v_cosine * u_sine, *ref_direction),
                    (radius * v_cosine * u_cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (-radius * v_sine * u_cosine, *ref_direction),
                    (-radius * v_sine * u_sine, transverse),
                    (radius * v_cosine, *axis),
                ]),
                duu: vector_sum(&[
                    (-radius * v_cosine * u_cosine, *ref_direction),
                    (-radius * v_cosine * u_sine, transverse),
                ]),
                duv: vector_sum(&[
                    (radius * v_sine * u_sine, *ref_direction),
                    (-radius * v_sine * u_cosine, transverse),
                ]),
                dvv: vector_sum(&[
                    (-radius * v_cosine * u_cosine, *ref_direction),
                    (-radius * v_cosine * u_sine, transverse),
                    (-radius * v_sine, *axis),
                ]),
            })
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let transverse = cross(*axis, *ref_direction);
            let u_cosine = u.cos();
            let u_sine = u.sin();
            let v_cosine = v.cos();
            let v_sine = v.sin();
            let ring = major_radius + minor_radius * v_cosine;
            Some(SurfaceSecondPartials {
                point: offset(
                    *center,
                    &[
                        (ring * u_cosine, *ref_direction),
                        (ring * u_sine, transverse),
                        (minor_radius * v_sine, *axis),
                    ],
                ),
                du: vector_sum(&[
                    (-ring * u_sine, *ref_direction),
                    (ring * u_cosine, transverse),
                ]),
                dv: vector_sum(&[
                    (-minor_radius * v_sine * u_cosine, *ref_direction),
                    (-minor_radius * v_sine * u_sine, transverse),
                    (minor_radius * v_cosine, *axis),
                ]),
                duu: vector_sum(&[
                    (-ring * u_cosine, *ref_direction),
                    (-ring * u_sine, transverse),
                ]),
                duv: vector_sum(&[
                    (minor_radius * v_sine * u_sine, *ref_direction),
                    (-minor_radius * v_sine * u_cosine, transverse),
                ]),
                dvv: vector_sum(&[
                    (-minor_radius * v_cosine * u_cosine, *ref_direction),
                    (-minor_radius * v_cosine * u_sine, transverse),
                    (-minor_radius * v_sine, *axis),
                ]),
            })
        }
        SurfaceGeometry::Nurbs(nurbs) => nurbs_surface_second_partials(nurbs, u, v),
        SurfaceGeometry::Transformed { basis, transform } => {
            surface_second_partials_inner(basis, u, v, depth + 1).map(|partials| {
                SurfaceSecondPartials {
                    point: affine_point(*transform, partials.point),
                    du: affine_vector(*transform, partials.du),
                    dv: affine_vector(*transform, partials.dv),
                    duu: affine_vector(*transform, partials.duu),
                    duv: affine_vector(*transform, partials.duv),
                    dvv: affine_vector(*transform, partials.dvv),
                }
            })
        }
        SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

/// Evaluate an exact construction before its solved cache. In particular, a
/// rational-quadratic revolution cache represents the circle exactly but does
/// not preserve the native linear angular parameterization.
fn procedural_surface_point(
    index: &crate::index::ModelIndex<'_>,
    procedural: &crate::geometry::ProceduralSurface,
    u: f64,
    v: f64,
) -> Option<Point3> {
    match &procedural.definition {
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            parameter_interval,
            ..
        } => {
            if parameter_interval.is_some_and(|range| {
                !range[0].is_finite()
                    || !range[1].is_finite()
                    || range[0] > range[1]
                    || u < range[0]
                    || u > range[1]
            }) {
                return None;
            }
            model_curve_point_by_id(index, directrix, u)
                .map(|point| offset(point, &[(v, *direction)]))
        }
        ProceduralSurfaceDefinition::Revolution {
            directrix,
            axis_origin,
            axis_direction,
            angular_interval,
            parameter_interval,
            transposed,
            ..
        } => {
            let (directrix_parameter, angle) = if *transposed { (v, u) } else { (u, v) };
            if parameter_interval.is_some_and(|range| {
                !range[0].is_finite()
                    || !range[1].is_finite()
                    || range[0] > range[1]
                    || directrix_parameter < range[0]
                    || directrix_parameter > range[1]
            }) || !angular_interval.iter().all(|value| value.is_finite())
                || !angle.is_finite()
            {
                return None;
            }
            let axis_length = axis_direction.norm();
            if !axis_length.is_finite() || axis_length == 0.0 {
                return None;
            }
            let axis = Vector3::new(
                axis_direction.x / axis_length,
                axis_direction.y / axis_length,
                axis_direction.z / axis_length,
            );
            let point = model_curve_point_by_id(index, directrix, directrix_parameter)?;
            let delta = Vector3::new(
                point.x - axis_origin.x,
                point.y - axis_origin.y,
                point.z - axis_origin.z,
            );
            let axis_point = offset(*axis_origin, &[(delta.dot(axis), axis)]);
            let radial = Vector3::new(
                point.x - axis_point.x,
                point.y - axis_point.y,
                point.z - axis_point.z,
            );
            let tangent = cross(axis, radial);
            Some(offset(
                axis_point,
                &[(angle.cos(), radial), (angle.sin(), tangent)],
            ))
        }
        _ => None,
    }
}

/// Evaluate a surface carrier with access to construction and child-carrier
/// arenas in `ir`.
pub fn model_surface_point(
    ir: &CadIr,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Option<Point3> {
    let SurfaceGeometry::Procedural { construction } = geometry else {
        return surface_point(geometry, u, v);
    };
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)?;
    let index = crate::index::ModelIndex::new(ir);
    procedural_surface_point(&index, procedural, u, v)
}

/// Evaluate a surface carrier selected by arena id.
pub fn model_surface_point_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Option<Point3> {
    struct SurfaceEvaluation {
        point: Point3,
        oriented_normal: Option<Vector3>,
    }

    fn oriented_normal(partials: SurfacePartials) -> Option<Vector3> {
        let normal = cross(partials.du, partials.dv);
        let magnitude = normal.norm();
        (magnitude.is_finite() && magnitude > 0.0).then(|| {
            Vector3::new(
                normal.x / magnitude,
                normal.y / magnitude,
                normal.z / magnitude,
            )
        })
    }

    fn evaluate(
        index: &crate::index::ModelIndex<'_>,
        surface_id: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        visiting: &mut Vec<crate::ids::SurfaceId>,
    ) -> Option<SurfaceEvaluation> {
        if visiting.contains(surface_id) {
            return None;
        }
        visiting.push(surface_id.clone());
        let surface = index.surfaces(&surface_id.0)?;
        let result = if let SurfaceGeometry::Procedural { construction } = &surface.geometry {
            let procedural = index.procedural_surfaces(&construction.0)?;
            if procedural.surface != *surface_id {
                return None;
            }
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset {
                    support, distance, ..
                } => {
                    let support = evaluate(index, support, u, v, visiting)?;
                    let normal = support.oriented_normal?;
                    Some(SurfaceEvaluation {
                        point: offset(support.point, &[(*distance, normal)]),
                        oriented_normal: Some(normal),
                    })
                }
                _ => procedural_surface_point(index, procedural, u, v).map(|point| {
                    SurfaceEvaluation {
                        point,
                        oriented_normal: surface_partials(&surface.geometry, u, v)
                            .and_then(oriented_normal),
                    }
                }),
            }
        } else {
            let procedural = index
                .ir()
                .model
                .procedural_surfaces
                .iter()
                .find(|procedural| procedural.surface == *surface_id);
            procedural
                .and_then(|procedural| {
                    procedural_surface_point(index, procedural, u, v).map(|point| {
                        SurfaceEvaluation {
                            point,
                            oriented_normal: surface_partials(&surface.geometry, u, v)
                                .and_then(oriented_normal),
                        }
                    })
                })
                .or_else(|| {
                    surface_partials(&surface.geometry, u, v).map(|partials| SurfaceEvaluation {
                        point: partials.point,
                        oriented_normal: oriented_normal(partials),
                    })
                })
        };
        visiting.pop();
        result
    }

    evaluate(index, surface, u, v, &mut Vec::new()).map(|evaluation| evaluation.point)
}

/// Evaluate an arena-selected direct or uniform-offset surface and its exact
/// first partial derivatives.
///
/// Nested offsets share the base surface's oriented unit normal, so their
/// signed distances combine before differentiating the normal field.
pub fn model_surface_partials_by_id(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Option<SurfacePartials> {
    let mut support = surface;
    let mut distance = 0.0;
    let mut visiting = Vec::new();
    loop {
        if visiting.contains(support) {
            return None;
        }
        visiting.push(support.clone());
        let carrier = index.surfaces(&support.0)?;
        if let SurfaceGeometry::Procedural { construction } = &carrier.geometry {
            let procedural = index.procedural_surfaces(&construction.0)?;
            if procedural.surface != *support {
                return None;
            }
            let ProceduralSurfaceDefinition::Offset {
                support: next,
                distance: increment,
                ..
            } = &procedural.definition
            else {
                return None;
            };
            distance += increment;
            support = next;
            continue;
        }

        let base = surface_second_partials(&carrier.geometry, u, v)?;
        let normal_vector = cross(base.du, base.dv);
        let normal_magnitude = normal_vector.norm();
        if !normal_magnitude.is_finite() || normal_magnitude == 0.0 {
            return None;
        }
        let normal = Vector3::new(
            normal_vector.x / normal_magnitude,
            normal_vector.y / normal_magnitude,
            normal_vector.z / normal_magnitude,
        );
        let normal_u_numerator = vector_sum(&[
            (1.0, cross(base.duu, base.dv)),
            (1.0, cross(base.du, base.duv)),
        ]);
        let normal_v_numerator = vector_sum(&[
            (1.0, cross(base.duv, base.dv)),
            (1.0, cross(base.du, base.dvv)),
        ]);
        let unit_normal_derivative = |derivative: Vector3| {
            let normal_component =
                normal.x * derivative.x + normal.y * derivative.y + normal.z * derivative.z;
            Vector3::new(
                (derivative.x - normal_component * normal.x) / normal_magnitude,
                (derivative.y - normal_component * normal.y) / normal_magnitude,
                (derivative.z - normal_component * normal.z) / normal_magnitude,
            )
        };
        let normal_u = unit_normal_derivative(normal_u_numerator);
        let normal_v = unit_normal_derivative(normal_v_numerator);
        return Some(SurfacePartials {
            point: Point3::new(
                base.point.x + distance * normal.x,
                base.point.y + distance * normal.y,
                base.point.z + distance * normal.z,
            ),
            du: Vector3::new(
                base.du.x + distance * normal_u.x,
                base.du.y + distance * normal_u.y,
                base.du.z + distance * normal_u.z,
            ),
            dv: Vector3::new(
                base.dv.x + distance * normal_v.x,
                base.dv.y + distance * normal_v.y,
                base.dv.z + distance * normal_v.z,
            ),
        });
    }
}

fn polyline_point(points: &[Point3], parameters: Option<&[f64]>, t: f64) -> Option<Point3> {
    if points.len() < 2 || !t.is_finite() {
        return None;
    }
    let implicit;
    let parameters = if let Some(parameters) = parameters {
        if parameters.len() != points.len() {
            return None;
        }
        parameters
    } else {
        implicit = (0..points.len())
            .map(|index| index as f64)
            .collect::<Vec<_>>();
        &implicit
    };
    let segment = parameters.windows(2).position(|window| {
        (t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])
    })?;
    let width = parameters[segment + 1] - parameters[segment];
    if width == 0.0 || !width.is_finite() {
        return None;
    }
    let fraction = (t - parameters[segment]) / width;
    let start = points[segment];
    let end = points[segment + 1];
    Some(Point3::new(
        start.x + fraction * (end.x - start.x),
        start.y + fraction * (end.y - start.y),
        start.z + fraction * (end.z - start.z),
    ))
}

fn polyline_tangent(points: &[Point3], parameters: Option<&[f64]>, t: f64) -> Option<Vector3> {
    if points.len() < 2 || !t.is_finite() {
        return None;
    }
    let implicit;
    let parameters = if let Some(parameters) = parameters {
        if parameters.len() != points.len() {
            return None;
        }
        parameters
    } else {
        implicit = (0..points.len())
            .map(|index| index as f64)
            .collect::<Vec<_>>();
        &implicit
    };
    let mut tangent = None;
    for (segment, window) in parameters.windows(2).enumerate() {
        if !((t >= window[0] && t <= window[1]) || (t <= window[0] && t >= window[1])) {
            continue;
        }
        let width = window[1] - window[0];
        if width == 0.0 || !width.is_finite() {
            return None;
        }
        let start = points[segment];
        let end = points[segment + 1];
        let candidate = Vector3::new(
            (end.x - start.x) / width,
            (end.y - start.y) / width,
            (end.z - start.z) / width,
        );
        if tangent.is_some_and(|tangent| tangent != candidate) {
            return None;
        }
        tangent = Some(candidate);
    }
    tangent
}

fn affine_point(transform: Transform, point: Point3) -> Point3 {
    Point3::new(
        transform.rows[0][0] * point.x
            + transform.rows[0][1] * point.y
            + transform.rows[0][2] * point.z
            + transform.rows[0][3],
        transform.rows[1][0] * point.x
            + transform.rows[1][1] * point.y
            + transform.rows[1][2] * point.z
            + transform.rows[1][3],
        transform.rows[2][0] * point.x
            + transform.rows[2][1] * point.y
            + transform.rows[2][2] * point.z
            + transform.rows[2][3],
    )
}

fn affine_vector(transform: Transform, vector: Vector3) -> Vector3 {
    Vector3::new(
        transform.rows[0][0] * vector.x
            + transform.rows[0][1] * vector.y
            + transform.rows[0][2] * vector.z,
        transform.rows[1][0] * vector.x
            + transform.rows[1][1] * vector.y
            + transform.rows[1][2] * vector.z,
        transform.rows[2][0] * vector.x
            + transform.rows[2][1] * vector.y
            + transform.rows[2][2] * vector.z,
    )
}

fn vector_sum(terms: &[(f64, Vector3)]) -> Vector3 {
    terms
        .iter()
        .fold(Vector3::new(0.0, 0.0, 0.0), |mut vector, (factor, term)| {
            vector.x += factor * term.x;
            vector.y += factor * term.y;
            vector.z += factor * term.z;
            vector
        })
}

/// Evaluate a pcurve carrier at parameter `t`, yielding a surface `(u, v)`.
pub fn pcurve_uv(geometry: &PcurveGeometry, t: f64) -> Option<Point2> {
    pcurve_uv_inner(geometry, t, 0)
}

/// Evaluate the exact first derivative of a directly stored pcurve.
pub fn pcurve_tangent(geometry: &PcurveGeometry, t: f64) -> Option<Point2> {
    pcurve_uv_differential_inner(geometry, t, 0)?.tangent
}

fn pcurve_uv_inner(geometry: &PcurveGeometry, t: f64, depth: usize) -> Option<Point2> {
    pcurve_uv_differential_inner(geometry, t, depth).map(|differential| differential.point)
}

fn pcurve_uv_differential_inner(
    geometry: &PcurveGeometry,
    t: f64,
    depth: usize,
) -> Option<PcurveDifferential> {
    if depth > 256 {
        return None;
    }
    if let PcurveGeometry::Offset { distance, basis } = geometry {
        let basis = pcurve_uv_differential_inner(basis, t, depth + 1)?;
        let tangent = basis.tangent?;
        let speed = tangent.u.hypot(tangent.v);
        if !speed.is_finite() || speed == 0.0 {
            return None;
        }
        let unit = Point2::new(tangent.u / speed, tangent.v / speed);
        let point = Point2::new(
            basis.point.u - distance * unit.v,
            basis.point.v + distance * unit.u,
        );
        let tangent = basis.acceleration.map(|acceleration| {
            let tangential_acceleration = unit.u * acceleration.u + unit.v * acceleration.v;
            let unit_derivative = Point2::new(
                (acceleration.u - tangential_acceleration * unit.u) / speed,
                (acceleration.v - tangential_acceleration * unit.v) / speed,
            );
            Point2::new(
                tangent.u - distance * unit_derivative.v,
                tangent.v + distance * unit_derivative.u,
            )
        });
        return Some(PcurveDifferential {
            point,
            tangent: tangent.filter(|tangent| tangent.u.is_finite() && tangent.v.is_finite()),
            acceleration: None,
        });
    }
    let pair = match geometry {
        PcurveGeometry::Line { origin, direction } => (
            Point2::new(origin.u + t * direction.u, origin.v + t * direction.v),
            *direction,
            Point2::new(0.0, 0.0),
        ),
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => {
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    *center,
                    &[(radius * cosine, *x_axis), (radius * sine, *y_axis)],
                ),
                Point2::new(
                    radius * (-sine * x_axis.u + cosine * y_axis.u),
                    radius * (-sine * x_axis.v + cosine * y_axis.v),
                ),
                Point2::new(
                    -radius * (cosine * x_axis.u + sine * y_axis.u),
                    -radius * (cosine * x_axis.v + sine * y_axis.v),
                ),
            )
        }
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = t.cos();
            let sine = t.sin();
            (
                offset2(
                    *center,
                    &[
                        (major_radius * cosine, *x_axis),
                        (minor_radius * sine, *y_axis),
                    ],
                ),
                Point2::new(
                    -major_radius * sine * x_axis.u + minor_radius * cosine * y_axis.u,
                    -major_radius * sine * x_axis.v + minor_radius * cosine * y_axis.v,
                ),
                Point2::new(
                    -major_radius * cosine * x_axis.u - minor_radius * sine * y_axis.u,
                    -major_radius * cosine * x_axis.v - minor_radius * sine * y_axis.v,
                ),
            )
        }
        PcurveGeometry::Harmonic {
            center,
            cosine,
            sine,
        } => {
            let cosine_parameter = t.cos();
            let sine_parameter = t.sin();
            (
                offset2(
                    *center,
                    &[(cosine_parameter, *cosine), (sine_parameter, *sine)],
                ),
                Point2::new(
                    -sine_parameter * cosine.u + cosine_parameter * sine.u,
                    -sine_parameter * cosine.v + cosine_parameter * sine.v,
                ),
                Point2::new(
                    -cosine_parameter * cosine.u - sine_parameter * sine.u,
                    -cosine_parameter * cosine.v - sine_parameter * sine.v,
                ),
            )
        }
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } if *focal_distance != 0.0 => (
            offset2(
                *vertex,
                &[(t * t / (4.0 * focal_distance), *x_axis), (t, *y_axis)],
            ),
            Point2::new(
                t / (2.0 * focal_distance) * x_axis.u + y_axis.u,
                t / (2.0 * focal_distance) * x_axis.v + y_axis.v,
            ),
            Point2::new(
                x_axis.u / (2.0 * focal_distance),
                x_axis.v / (2.0 * focal_distance),
            ),
        ),
        PcurveGeometry::Parabola { .. } => return None,
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => {
            let cosine = t.cosh();
            let sine = t.sinh();
            (
                offset2(
                    *center,
                    &[
                        (major_radius * cosine, *x_axis),
                        (minor_radius * sine, *y_axis),
                    ],
                ),
                Point2::new(
                    major_radius * sine * x_axis.u + minor_radius * cosine * y_axis.u,
                    major_radius * sine * x_axis.v + minor_radius * cosine * y_axis.v,
                ),
                Point2::new(
                    major_radius * cosine * x_axis.u + minor_radius * sine * y_axis.u,
                    major_radius * cosine * x_axis.v + minor_radius * sine * y_axis.v,
                ),
            )
        }
        PcurveGeometry::Hyperbolic {
            center,
            cosine,
            sine,
        } => {
            let cosine_parameter = t.cosh();
            let sine_parameter = t.sinh();
            (
                offset2(
                    *center,
                    &[(cosine_parameter, *cosine), (sine_parameter, *sine)],
                ),
                Point2::new(
                    sine_parameter * cosine.u + cosine_parameter * sine.u,
                    sine_parameter * cosine.v + cosine_parameter * sine.v,
                ),
                Point2::new(
                    cosine_parameter * cosine.u + sine_parameter * sine.u,
                    cosine_parameter * cosine.v + sine_parameter * sine.v,
                ),
            )
        }
        PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        } => {
            let cosine = t.cos();
            let sine = t.sin();
            let x = radial_center.u + radial_cos.u * cosine + radial_sin.u * sine;
            let y = radial_center.v + radial_cos.v * cosine + radial_sin.v * sine;
            let dx = -radial_cos.u * sine + radial_sin.u * cosine;
            let dy = -radial_cos.v * sine + radial_sin.v * cosine;
            let ddx = -radial_cos.u * cosine - radial_sin.u * sine;
            let ddy = -radial_cos.v * cosine - radial_sin.v * sine;
            let radius_squared = x * x + y * y;
            if radius_squared == 0.0 {
                return None;
            }
            (
                Point2::new(
                    y.atan2(x),
                    axial_origin + axial_cos * cosine + axial_sin * sine,
                ),
                Point2::new(
                    (x * dy - y * dx) / radius_squared,
                    -axial_cos * sine + axial_sin * cosine,
                ),
                Point2::new(
                    ((x * ddy - y * ddx) * radius_squared
                        - (x * dy - y * dx) * 2.0 * (x * dx + y * dy))
                        / (radius_squared * radius_squared),
                    -axial_cos * cosine - axial_sin * sine,
                ),
            )
        }
        PcurveGeometry::PolarNurbs {
            degree,
            knots,
            radial_control_points,
            axial_control_points,
            weights,
            ..
        } => {
            if radial_control_points.len() != axial_control_points.len() {
                return None;
            }
            let radial = nurbs_pcurve_differential(
                *degree,
                knots,
                radial_control_points,
                weights.as_deref(),
                t,
            )?;
            let axial_points = axial_control_points
                .iter()
                .map(|value| Point2::new(*value, 0.0))
                .collect::<Vec<_>>();
            let axial =
                nurbs_pcurve_differential(*degree, knots, &axial_points, weights.as_deref(), t)?;
            let radius_squared = radial.point.u * radial.point.u + radial.point.v * radial.point.v;
            if radius_squared == 0.0 {
                return None;
            }
            let point = Point2::new(radial.point.v.atan2(radial.point.u), axial.point.u);
            let tangent = radial
                .tangent
                .zip(axial.tangent)
                .map(|(radial_tangent, axial_tangent)| {
                    Point2::new(
                        (radial.point.u * radial_tangent.v - radial.point.v * radial_tangent.u)
                            / radius_squared,
                        axial_tangent.u,
                    )
                })
                .filter(|tangent| tangent.u.is_finite() && tangent.v.is_finite());
            let acceleration = radial
                .tangent
                .zip(radial.acceleration)
                .zip(axial.acceleration)
                .map(
                    |((radial_tangent, radial_acceleration), axial_acceleration)| {
                        let numerator =
                            radial.point.u * radial_tangent.v - radial.point.v * radial_tangent.u;
                        let numerator_derivative = radial.point.u * radial_acceleration.v
                            - radial.point.v * radial_acceleration.u;
                        let denominator_derivative = 2.0
                            * (radial.point.u * radial_tangent.u
                                + radial.point.v * radial_tangent.v);
                        Point2::new(
                            (numerator_derivative * radius_squared
                                - numerator * denominator_derivative)
                                / (radius_squared * radius_squared),
                            axial_acceleration.u,
                        )
                    },
                )
                .filter(|acceleration| acceleration.u.is_finite() && acceleration.v.is_finite());
            return Some(PcurveDifferential {
                point,
                tangent,
                acceleration,
            });
        }
        PcurveGeometry::SphericalGreatCircle {
            azimuth_origin,
            azimuth_rate,
            plane_phase,
            plane_slope,
        } => {
            let azimuth = azimuth_origin + azimuth_rate * t;
            let phase = azimuth - plane_phase;
            let cosine = phase.cos();
            let sine = phase.sin();
            let latitude = (plane_slope * cosine).atan();
            let denominator = 1.0 + plane_slope * plane_slope * cosine * cosine;
            let numerator = -plane_slope * azimuth_rate * sine;
            let denominator_derivative =
                -2.0 * plane_slope * plane_slope * azimuth_rate * cosine * sine;
            let numerator_derivative = -plane_slope * azimuth_rate * azimuth_rate * cosine;
            let point = Point2::new(azimuth, latitude);
            let tangent = Point2::new(*azimuth_rate, numerator / denominator);
            let acceleration = Point2::new(
                0.0,
                (numerator_derivative * denominator - numerator * denominator_derivative)
                    / (denominator * denominator),
            );
            return (point.u.is_finite() && point.v.is_finite()).then_some(PcurveDifferential {
                point,
                tangent: (tangent.u.is_finite() && tangent.v.is_finite()).then_some(tangent),
                acceleration: (acceleration.u.is_finite() && acceleration.v.is_finite())
                    .then_some(acceleration),
            });
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } => {
            return nurbs_pcurve_differential(
                *degree,
                knots,
                control_points,
                weights.as_deref(),
                t,
            );
        }
        PcurveGeometry::Trimmed { basis, .. } => {
            return pcurve_uv_differential_inner(basis, t, depth + 1);
        }
        PcurveGeometry::Offset { .. } => return None,
    };
    if !pair.0.u.is_finite() || !pair.0.v.is_finite() {
        return None;
    }
    Some(PcurveDifferential {
        point: pair.0,
        tangent: (pair.1.u.is_finite() && pair.1.v.is_finite()).then_some(pair.1),
        acceleration: (pair.2.u.is_finite() && pair.2.v.is_finite()).then_some(pair.2),
    })
}

fn offset2(base: Point2, terms: &[(f64, Point2)]) -> Point2 {
    terms.iter().fold(base, |mut point, (factor, direction)| {
        point.u += factor * direction.u;
        point.v += factor * direction.v;
        point
    })
}

#[cfg(test)]
mod tests {
    use super::{
        curve_point, curve_second_derivative, curve_tangent, model_surface_partials_by_id,
        model_surface_point_by_id, nurbs_curve_parameter_near_point, nurbs_curve_point,
        nurbs_curve_speed_bound, nurbs_surface_isocurve, nurbs_surface_isoline,
        nurbs_surface_partials, nurbs_surface_point, nurbs_surface_second_partials, pcurve_tangent,
        pcurve_uv, surface_partials, surface_second_partials, IsolineDirection,
    };
    use crate::geometry::{
        Curve, CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, ProceduralSurface,
        ProceduralSurfaceDefinition, Surface, SurfaceGeometry, SurfaceParameterAxis,
    };
    use crate::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
    use crate::math::{Point2, Point3, Vector3};
    use crate::transform::Transform;
    use crate::CadIr;

    #[test]
    fn direct_analytic_curve_inverses_preserve_native_parameters() {
        let geometries = [
            CurveGeometry::Line {
                origin: Point3::new(1.0, 2.0, 3.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            CurveGeometry::Circle {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                ref_direction: Vector3::new(1.0, 0.0, 0.0),
                radius: 4.0,
            },
            CurveGeometry::Ellipse {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                major_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 4.0,
                minor_radius: 2.0,
            },
            CurveGeometry::Parabola {
                vertex: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                major_direction: Vector3::new(1.0, 0.0, 0.0),
                focal_distance: 2.0,
            },
            CurveGeometry::Hyperbola {
                center: Point3::new(1.0, 2.0, 3.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
                major_direction: Vector3::new(1.0, 0.0, 0.0),
                major_radius: 4.0,
                minor_radius: 2.0,
            },
        ];
        for (index, geometry) in geometries.into_iter().enumerate() {
            let parameter = if matches!(
                &geometry,
                CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }
            ) {
                0.7 + std::f64::consts::TAU
            } else {
                0.7
            };
            let point = curve_point(&geometry, parameter).expect("analytic curve evaluates");
            let id = CurveId(format!("test:inverse:{index}"));
            let mut ir = CadIr::empty(crate::units::Units::default());
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            let inverse = super::model_curve_parameter_near_point(&ir, &id, point, parameter)
                .expect("direct analytic inverse");
            assert!((inverse - parameter).abs() < 1.0e-12);
        }
    }

    #[test]
    fn polyline_inverse_searches_every_segment_in_native_parameter_space() {
        let cases = [
            (
                CurveGeometry::Polyline {
                    points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                        Point3::new(1.0, 1.0, 0.0),
                    ],
                    parameters: None,
                    chordal_deflection: 0.0,
                },
                Point3::new(0.5, 0.0, 0.0),
                0.5,
                0.5,
            ),
            (
                CurveGeometry::Polyline {
                    points: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                        Point3::new(1.0, 1.0, 0.0),
                    ],
                    parameters: Some(vec![4.0, 2.0, 0.0]),
                    chordal_deflection: 0.0,
                },
                Point3::new(1.0, 0.5, 0.0),
                1.0,
                1.0,
            ),
            (
                CurveGeometry::Polyline {
                    points: vec![
                        Point3::new(2.0, 3.0, 4.0),
                        Point3::new(2.0, 3.0, 4.0),
                        Point3::new(5.0, 3.0, 4.0),
                    ],
                    parameters: Some(vec![0.0, 1.0, 2.0]),
                    chordal_deflection: 0.0,
                },
                Point3::new(2.0, 3.0, 4.0),
                0.7,
                0.7,
            ),
        ];
        for (index, (geometry, point, seed, expected)) in cases.into_iter().enumerate() {
            let id = CurveId(format!("test:polyline-inverse:{index}"));
            let mut ir = CadIr::empty(crate::units::Units::default());
            ir.model.curves.push(Curve {
                id: id.clone(),
                geometry,
                source_object: None,
            });
            let inverse = super::model_curve_parameter_near_point(&ir, &id, point, seed)
                .expect("polyline inverse");
            assert!((inverse - expected).abs() < 1.0e-12);
        }
    }

    #[test]
    fn transformed_curve_inverse_uses_the_basis_parameterization() {
        let basis = CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        };
        let transform = Transform {
            rows: [
                [-2.0, 0.0, 0.0, 1.0e6],
                [0.0, 0.5, 0.0, -2.0e6],
                [0.0, 0.0, 3.0, 3.0e6],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let geometry = CurveGeometry::Transformed {
            basis: Box::new(basis.clone()),
            transform,
        };
        let parameter = 0.7 + std::f64::consts::TAU;
        let point = curve_point(&geometry, parameter).expect("transformed curve evaluates");
        let id = CurveId("test:transformed-inverse".into());
        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        let inverse = super::model_curve_parameter_near_point(&ir, &id, point, parameter)
            .expect("transformed inverse");
        assert!((inverse - parameter).abs() < 1.0e-10);

        ir.model.curves[0].geometry = CurveGeometry::Transformed {
            basis: Box::new(basis),
            transform: Transform {
                rows: [
                    [0.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
        };
        assert!(
            super::model_curve_parameter_near_point(&ir, &id, Point3::new(0.0, 0.0, 0.0), 0.0,)
                .is_none()
        );
    }

    #[test]
    fn degenerate_curve_inverse_preserves_the_selected_parameter() {
        let point = Point3::new(2.0, 3.0, 4.0);
        let id = CurveId("test:degenerate-inverse".into());
        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry: CurveGeometry::Degenerate { point },
            source_object: None,
        });
        let seed = 123.5;
        assert_eq!(
            super::model_curve_parameter_near_point(&ir, &id, point, seed),
            Some(seed)
        );
        assert!(super::model_curve_parameter_near_point(
            &ir,
            &id,
            Point3::new(2.0, 3.0, 5.0),
            seed,
        )
        .is_none());
    }

    #[test]
    fn a_surface_isoline_reproduces_the_surface_along_its_free_parameter() {
        // Rational, quadratic in u and linear in v, so the blend across the
        // fixed direction has to carry weights to stay exact.
        let surface = NurbsSurface {
            u_degree: 2,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![-2.0, -2.0, 3.0, 3.0],
            u_count: 3,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 4.0),
                Point3::new(1.0, 2.0, 0.5),
                Point3::new(1.0, 2.0, 4.5),
                Point3::new(3.0, -1.0, 1.0),
                Point3::new(3.0, -1.0, 5.0),
            ],
            weights: Some(vec![1.0, 2.0, 0.5, 1.5, 3.0, 0.25]),
            u_periodic: false,
            v_periodic: false,
        };

        for (direction, at, samples) in [
            (IsolineDirection::ConstantU, 0.4, [-2.0, 0.75, 3.0]),
            (IsolineDirection::ConstantV, 1.25, [0.0, 0.6, 1.0]),
        ] {
            let curve = nurbs_surface_isoline(&surface, direction, at).expect("isoline");
            for sample in samples {
                let (u, v) = match direction {
                    IsolineDirection::ConstantU => (at, sample),
                    IsolineDirection::ConstantV => (sample, at),
                };
                let expected = nurbs_surface_point(&surface, u, v).expect("surface point");
                let actual = nurbs_curve_point(
                    curve.degree,
                    &curve.knots,
                    &curve.control_points,
                    curve.weights.as_deref(),
                    sample,
                )
                .expect("curve point");
                for (left, right) in [
                    (actual.x, expected.x),
                    (actual.y, expected.y),
                    (actual.z, expected.z),
                ] {
                    assert!((left - right).abs() <= 1.0e-12, "{left} vs {right}");
                }
            }
        }
    }

    #[test]
    fn bilinear_surface_partials_follow_stored_parameterization() {
        let surface = NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 3.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 3.0, 0.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let partials = nurbs_surface_partials(&surface, 0.25, 0.75).expect("partials");
        assert_eq!(partials.point, Point3::new(0.5, 2.25, 0.0));
        assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
        assert_eq!(partials.dv, Vector3::new(0.0, 3.0, 0.0));
    }

    #[test]
    fn quadratic_surface_second_partials_follow_stored_parameterization() {
        let surface = NurbsSurface {
            u_degree: 2,
            v_degree: 2,
            u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            u_count: 3,
            v_count: 3,
            control_points: (0..3)
                .flat_map(|i| {
                    (0..3).map(move |j| {
                        Point3::new(
                            f64::from(i) / 2.0,
                            f64::from(j) / 2.0,
                            f64::from(u8::from(i == 2)) + f64::from(u8::from(j == 2)),
                        )
                    })
                })
                .collect(),
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let partials =
            nurbs_surface_second_partials(&surface, 0.25, 0.75).expect("second partials");
        assert_eq!(partials.point, Point3::new(0.25, 0.75, 0.625));
        assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.5));
        assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 1.5));
        assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 2.0));
        assert_eq!(partials.duv, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(partials.dvv, Vector3::new(0.0, 0.0, 2.0));
    }

    #[test]
    fn recursive_offsets_use_exact_support_normals_at_large_parameters() {
        let support_id = SurfaceId("support".into());
        let first_id = SurfaceId("first-offset".into());
        let second_id = SurfaceId("second-offset".into());
        let first_construction = ProceduralSurfaceId("first-construction".into());
        let second_construction = ProceduralSurfaceId("second-construction".into());
        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.model.surfaces = vec![
            Surface {
                id: support_id.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            },
            Surface {
                id: first_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: first_construction.clone(),
                },
                source_object: None,
            },
            Surface {
                id: second_id.clone(),
                geometry: SurfaceGeometry::Procedural {
                    construction: second_construction.clone(),
                },
                source_object: None,
            },
        ];
        ir.model.procedural_surfaces = vec![
            ProceduralSurface {
                id: first_construction,
                surface: first_id.clone(),
                definition: ProceduralSurfaceDefinition::Offset {
                    support: support_id,
                    distance: 2.0,
                    u_sense: None,
                    v_sense: None,
                    extension_flags: Vec::new(),
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            },
            ProceduralSurface {
                id: second_construction,
                surface: second_id.clone(),
                definition: ProceduralSurfaceDefinition::Offset {
                    support: first_id,
                    distance: -5.0,
                    u_sense: None,
                    v_sense: None,
                    extension_flags: Vec::new(),
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        ];

        let index = crate::index::ModelIndex::new(&ir);
        assert_eq!(
            model_surface_point_by_id(&index, &second_id, 1.0e16, -1.0e16),
            Some(Point3::new(1.0e16, -1.0e16, -3.0))
        );
        let partials = model_surface_partials_by_id(&index, &second_id, 1.0e16, -1.0e16)
            .expect("transformed plane evaluates");
        assert_eq!(partials.point, Point3::new(1.0e16, -1.0e16, -3.0));
        assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.0));
        assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn analytic_and_transformed_surface_partials_follow_parameterization() {
        let cylinder = SurfaceGeometry::Cylinder {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
        };
        let cone = SurfaceGeometry::Cone {
            origin: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 2.0,
            ratio: 1.0,
            half_angle: std::f64::consts::FRAC_PI_4,
        };
        let sphere = SurfaceGeometry::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 3.0,
        };
        let torus = SurfaceGeometry::Torus {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 5.0,
            minor_radius: 2.0,
        };
        let transformed = SurfaceGeometry::Transformed {
            basis: Box::new(SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            }),
            transform: Transform {
                rows: [
                    [2.0, 0.0, 0.0, 7.0],
                    [0.0, 3.0, 0.0, 11.0],
                    [0.0, 0.0, 4.0, 13.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
        };

        let cylinder_second = surface_second_partials(&cylinder, 0.0, 4.0)
            .expect("cylinder second partials evaluate");
        let cylinder = surface_partials(&cylinder, 0.0, 4.0).expect("cylinder partials evaluate");
        assert_eq!(cylinder.point, Point3::new(2.0, 0.0, 4.0));
        assert_eq!(cylinder.du, Vector3::new(0.0, 2.0, 0.0));
        assert_eq!(cylinder.dv, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(cylinder_second.duu, Vector3::new(-2.0, 0.0, 0.0));
        assert_eq!(cylinder_second.duv, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(cylinder_second.dvv, Vector3::new(0.0, 0.0, 0.0));
        let cone = surface_partials(&cone, 0.0, 3.0).expect("cone partials evaluate");
        assert!((cone.point.x - 5.0).abs() < 1e-12);
        assert!((cone.du.y - 5.0).abs() < 1e-12);
        assert!((cone.dv.x - 1.0).abs() < 1e-12);
        assert_eq!(cone.dv.z, 1.0);
        let sphere = surface_partials(&sphere, 0.0, 0.0).expect("sphere partials evaluate");
        assert_eq!(sphere.point, Point3::new(3.0, 0.0, 0.0));
        assert_eq!(sphere.du, Vector3::new(0.0, 3.0, 0.0));
        assert_eq!(sphere.dv, Vector3::new(0.0, 0.0, 3.0));
        let torus = surface_partials(&torus, 0.0, 0.0).expect("torus partials evaluate");
        assert_eq!(torus.point, Point3::new(7.0, 0.0, 0.0));
        assert_eq!(torus.du, Vector3::new(0.0, 7.0, 0.0));
        assert_eq!(torus.dv, Vector3::new(0.0, 0.0, 2.0));
        let transformed =
            surface_partials(&transformed, 2.0, 3.0).expect("transformed partials evaluate");
        assert_eq!(transformed.point, Point3::new(11.0, 20.0, 13.0));
        assert_eq!(transformed.du, Vector3::new(2.0, 0.0, 0.0));
        assert_eq!(transformed.dv, Vector3::new(0.0, 3.0, 0.0));
    }

    #[test]
    fn analytic_and_rational_curve_derivatives_are_exact() {
        let parameter = 1.0e16;
        let circle = CurveGeometry::Circle {
            center: Point3::new(0.0, 0.0, 0.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 3.0,
        };
        let tangent = curve_tangent(&circle, parameter).expect("analytic tangent");
        assert_eq!(
            tangent,
            Vector3::new(-3.0 * parameter.sin(), 3.0 * parameter.cos(), 0.0)
        );
        assert_eq!(
            curve_second_derivative(&circle, parameter),
            Some(Vector3::new(
                -3.0 * parameter.cos(),
                -3.0 * parameter.sin(),
                0.0,
            ))
        );
        assert_eq!(curve_tangent(&circle, f64::NAN), None);

        let arc = CurveGeometry::Nurbs(NurbsCurve {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            periodic: false,
        });
        for parameter in [0.0, 0.5, 1.0] {
            let point = curve_point(&arc, parameter).expect("rational arc point");
            let tangent = curve_tangent(&arc, parameter).expect("rational arc tangent");
            let second =
                curve_second_derivative(&arc, parameter).expect("rational arc acceleration");
            let radial_dot = point.x * tangent.x + point.y * tangent.y;
            assert!(radial_dot.abs() < 1e-12);
            assert!((point.x * second.x + point.y * second.y + tangent.dot(tangent)).abs() < 1e-11);
            assert!(tangent.norm() > 0.0);
        }

        let corner = CurveGeometry::Polyline {
            points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            parameters: Some(vec![0.0, 1.0, 2.0]),
            chordal_deflection: 0.0,
        };
        assert_eq!(
            curve_tangent(&corner, 0.5),
            Some(Vector3::new(1.0, 0.0, 0.0))
        );
        assert_eq!(curve_tangent(&corner, 1.0), None);
    }

    #[test]
    fn rational_surface_partials_apply_the_weight_quotient_rule() {
        let surface = NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 3.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 3.0, 0.0),
            ],
            weights: Some(vec![1.0, 1.0, 2.0, 2.0]),
            u_periodic: false,
            v_periodic: false,
        };
        let partials = nurbs_surface_partials(&surface, 0.5, 0.25).expect("partials");
        assert!((partials.point.x - 4.0 / 3.0).abs() < 1e-12);
        assert!((partials.point.y - 0.75).abs() < 1e-12);
        assert!((partials.du.x - 16.0 / 9.0).abs() < 1e-12);
        assert!(partials.du.y.abs() < 1e-12);
        assert!((partials.dv.y - 3.0).abs() < 1e-12);
        let second = nurbs_surface_second_partials(&surface, 0.5, 0.25).expect("second partials");
        assert!((second.duu.x + 64.0 / 27.0).abs() < 1e-12);
        assert!(second.duu.y.abs() < 1e-12);
        assert_eq!(second.duv, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(second.dvv, Vector3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn rational_surface_isocurves_preserve_the_tensor_product_parameterization() {
        let surface = NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 3.0, 0.0),
                Point3::new(2.0, 0.0, 1.0),
                Point3::new(2.0, 3.0, 1.0),
            ],
            weights: Some(vec![1.0, 2.0, 3.0, 4.0]),
            u_periodic: false,
            v_periodic: false,
        };
        for (axis, fixed) in [
            (SurfaceParameterAxis::U, 0.25),
            (SurfaceParameterAxis::V, 0.75),
        ] {
            let isocurve = nurbs_surface_isocurve(&surface, axis, fixed).expect("exact isocurve");
            let geometry = CurveGeometry::Nurbs(isocurve);
            for varying in [0.0, 0.2, 0.7, 1.0] {
                let expected = match axis {
                    SurfaceParameterAxis::U => {
                        nurbs_surface_point(&surface, fixed, varying).expect("surface point")
                    }
                    SurfaceParameterAxis::V => {
                        nurbs_surface_point(&surface, varying, fixed).expect("surface point")
                    }
                };
                let actual = curve_point(&geometry, varying).expect("isocurve point");
                assert!((actual.x - expected.x).abs() < 1e-12);
                assert!((actual.y - expected.y).abs() < 1e-12);
                assert!((actual.z - expected.z).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn nurbs_curve_inverse_uses_the_seed_to_select_an_ambiguous_witness() {
        let curve = NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            weights: None,
            periodic: false,
        };
        let point = Point3::new(0.5, 0.0, 0.0);
        assert_eq!(
            nurbs_curve_parameter_near_point(&curve, point, 1.0e-12, 0.1),
            Some(0.25)
        );
        assert_eq!(
            nurbs_curve_parameter_near_point(&curve, point, 1.0e-12, 0.9),
            Some(0.75)
        );
        assert_eq!(
            nurbs_curve_parameter_near_point(&curve, Point3::new(0.5, 1.0, 0.0), 1.0e-12, 0.5,),
            None
        );
        assert!(nurbs_curve_speed_bound(&curve).is_some_and(|bound| bound >= 2.0));
    }

    #[test]
    fn analytic_pcurves_preserve_angular_parameterization() {
        let circle = PcurveGeometry::Circle {
            center: Point2::new(2.0, 3.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, -1.0),
            radius: 4.0,
        };
        let ellipse = PcurveGeometry::Ellipse {
            center: Point2::new(2.0, 3.0),
            x_axis: Point2::new(0.0, 1.0),
            y_axis: Point2::new(-1.0, 0.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        };
        let polar = PcurveGeometry::PolarHarmonic {
            radial_center: Point2::new(0.0, 0.0),
            radial_cos: Point2::new(2.0, 0.0),
            radial_sin: Point2::new(0.0, 2.0),
            axial_origin: 3.0,
            axial_cos: 4.0,
            axial_sin: 0.0,
        };
        let polar_nurbs = PcurveGeometry::PolarNurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            radial_control_points: vec![
                Point2::new(2.0, 0.0),
                Point2::new(2.0, 2.0),
                Point2::new(0.0, 2.0),
            ],
            axial_control_points: vec![3.0, 4.0, 5.0],
            weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            periodic: false,
        };

        let circle_tangent =
            pcurve_tangent(&circle, std::f64::consts::FRAC_PI_2).expect("circle tangent");
        let circle = pcurve_uv(&circle, std::f64::consts::FRAC_PI_2).expect("circle evaluates");
        let ellipse = pcurve_uv(&ellipse, std::f64::consts::FRAC_PI_2).expect("ellipse evaluates");
        let polar = pcurve_uv(&polar, std::f64::consts::FRAC_PI_2).expect("polar curve evaluates");
        let polar_nurbs = pcurve_uv(&polar_nurbs, 0.5).expect("polar NURBS evaluates");
        assert!((circle.u - 2.0).abs() < 1e-12 && (circle.v + 1.0).abs() < 1e-12);
        assert!((circle_tangent.u + 4.0).abs() < 1e-12 && circle_tangent.v.abs() < 1e-12);
        assert!(ellipse.u.abs() < 1e-12 && (ellipse.v - 3.0).abs() < 1e-12);
        assert!((polar.u - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((polar.v - 3.0).abs() < 1e-12);
        assert!((polar_nurbs.u - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((polar_nurbs.v - 4.0).abs() < 1e-12);
    }

    #[test]
    fn spherical_great_circle_pcurve_preserves_affine_source_parameterization() {
        let geometry = PcurveGeometry::SphericalGreatCircle {
            azimuth_origin: 0.25,
            azimuth_rate: 0.5,
            plane_phase: 1.0,
            plane_slope: -0.75,
        };
        let point = pcurve_uv(&geometry, 1.5).expect("great-circle pcurve evaluates");
        assert_eq!(point.u, 1.0);
        assert_eq!(point.v, (-0.75_f64).atan());
    }

    #[test]
    fn general_harmonic_pcurves_evaluate_their_vector_coefficients() {
        let harmonic = PcurveGeometry::Harmonic {
            center: Point2::new(2.0, 3.0),
            cosine: Point2::new(4.0, -1.0),
            sine: Point2::new(2.0, 5.0),
        };
        let hyperbolic = PcurveGeometry::Hyperbolic {
            center: Point2::new(-3.0, 7.0),
            cosine: Point2::new(2.5, -4.0),
            sine: Point2::new(1.5, 0.75),
        };
        let angle = std::f64::consts::FRAC_PI_3;
        assert_eq!(
            pcurve_uv(&harmonic, angle),
            Some(Point2::new(
                2.0 + 4.0 * angle.cos() + 2.0 * angle.sin(),
                3.0 - angle.cos() + 5.0 * angle.sin(),
            ))
        );
        let parameter = 0.75_f64;
        assert_eq!(
            pcurve_uv(&hyperbolic, parameter),
            Some(Point2::new(
                -3.0 + 2.5 * parameter.cosh() + 1.5 * parameter.sinh(),
                7.0 - 4.0 * parameter.cosh() + 0.75 * parameter.sinh(),
            ))
        );
    }

    #[test]
    fn signed_offset_pcurves_use_the_exact_left_normal() {
        let line = PcurveGeometry::Offset {
            distance: 2.0,
            basis: Box::new(PcurveGeometry::Line {
                origin: Point2::new(1.0, 2.0),
                direction: Point2::new(3.0, 4.0),
            }),
        };
        let circle = PcurveGeometry::Offset {
            distance: 1.0,
            basis: Box::new(PcurveGeometry::Circle {
                center: Point2::new(0.0, 0.0),
                x_axis: Point2::new(1.0, 0.0),
                y_axis: Point2::new(0.0, 1.0),
                radius: 4.0,
            }),
        };
        let point = pcurve_uv(&line, 0.5).expect("regular line offset evaluates");
        assert!((point.u - 0.9).abs() < 1e-12);
        assert!((point.v - 5.2).abs() < 1e-12);
        assert_eq!(pcurve_uv(&circle, 0.0), Some(Point2::new(3.0, 0.0)));
        assert_eq!(pcurve_tangent(&line, 0.5), Some(Point2::new(3.0, 4.0)));
        assert_eq!(pcurve_tangent(&circle, 0.0), Some(Point2::new(0.0, 3.0)));

        let rational_arc = PcurveGeometry::Offset {
            distance: 0.25,
            basis: Box::new(PcurveGeometry::Nurbs {
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                control_points: vec![
                    Point2::new(1.0, 0.0),
                    Point2::new(1.0, 1.0),
                    Point2::new(0.0, 1.0),
                ],
                weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
                periodic: false,
            }),
        };
        for parameter in [0.0, 0.5, 1.0] {
            let point = pcurve_uv(&rational_arc, parameter)
                .expect("regular rational NURBS offset evaluates");
            let tangent =
                pcurve_tangent(&rational_arc, parameter).expect("rational offset tangent");
            assert!((point.u.hypot(point.v) - 0.75).abs() < 1e-12);
            assert!((point.u * tangent.u + point.v * tangent.v).abs() < 1e-12);
        }

        let nested = PcurveGeometry::Offset {
            distance: 1.0,
            basis: Box::new(line),
        };
        let nested_point = pcurve_uv(&nested, 0.5).expect("nested offset point");
        assert!((nested_point.u - 0.1).abs() < 1e-12);
        assert!((nested_point.v - 5.8).abs() < 1e-12);
        assert_eq!(pcurve_tangent(&nested, 0.5), None);
    }
}
