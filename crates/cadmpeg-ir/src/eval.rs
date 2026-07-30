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
use crate::transform::Transform;
use crate::CadIr;

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
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
    let mut u = 0.0;
    let mut v = 0.0;
    let mut weight_sum = 0.0;
    let mut du = 0.0;
    let mut dv = 0.0;
    let mut weight_derivative = 0.0;
    for (i, (value, derivative)) in basis.iter().zip(&derivative).enumerate() {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        u += value * weight * pole.u;
        v += value * weight * pole.v;
        weight_sum += value * weight;
        du += derivative * weight * pole.u;
        dv += derivative * weight * pole.v;
        weight_derivative += derivative * weight;
    }
    if weight_sum == 0.0 {
        return None;
    }
    let point = Point2::new(u / weight_sum, v / weight_sum);
    let tangent = Point2::new(
        (du - point.u * weight_derivative) / weight_sum,
        (dv - point.v * weight_derivative) / weight_sum,
    );
    if !point.u.is_finite() || !point.v.is_finite() {
        return None;
    }
    Some(PcurveDifferential {
        point,
        tangent: (tangent.u.is_finite() && tangent.v.is_finite()).then_some(tangent),
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

/// Evaluate a tensor-product NURBS surface and its exact rational first
/// partials at `(u, v)`.
pub fn nurbs_surface_partials(
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Option<SurfacePartials> {
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
    let mut weighted = [0.0; 3];
    let mut weighted_u = [0.0; 3];
    let mut weighted_v = [0.0; 3];
    let mut weight = 0.0;
    let mut weight_u = 0.0;
    let mut weight_v = 0.0;
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
            for (axis, coordinate) in [pole.x, pole.y, pole.z].into_iter().enumerate() {
                weighted[axis] += basis * coordinate;
                weighted_u[axis] += basis_u * coordinate;
                weighted_v[axis] += basis_v * coordinate;
            }
            weight += basis;
            weight_u += basis_u;
            weight_v += basis_v;
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
    Some(SurfacePartials {
        point,
        du: derivative(weighted_u, weight_u),
        dv: derivative(weighted_v, weight_v),
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

/// Evaluate a curve carrier selected by arena id, including supported
/// procedural constructions.
pub fn model_curve_point_by_id(
    ir: &CadIr,
    curve_id: &crate::ids::CurveId,
    parameter: f64,
) -> Option<Point3> {
    let curve = ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve_id)?;
    let CurveGeometry::Procedural { construction } = &curve.geometry else {
        return curve_point(&curve.geometry, parameter);
    };
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|candidate| candidate.id == *construction && candidate.curve == *curve_id)?;
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
        model_surface_point_by_id(ir, &supports[side], uv.u, uv.v)
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
/// Charted tolerant intersections use an exact affine line on planar supports
/// or an exact NURBS isocurve from a chart that fixes one surface parameter.
/// The seed selects between repeated model-space points. The returned parameter
/// is forward-validated against the complete two-support construction.
pub fn model_curve_parameter_near_point(
    ir: &CadIr,
    curve_id: &crate::ids::CurveId,
    point: Point3,
    seed: f64,
) -> Option<f64> {
    let curve = ir
        .model
        .curves
        .iter()
        .find(|candidate| candidate.id == *curve_id)?;
    let CurveGeometry::Procedural { construction } = &curve.geometry else {
        return None;
    };
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|candidate| candidate.id == *construction && candidate.curve == *curve_id)?;
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
        let Some(surface) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *support_id)
        else {
            continue;
        };
        let PcurveGeometry::Line { origin, direction } = pcurve else {
            continue;
        };
        let parameter = match &surface.geometry {
            SurfaceGeometry::Plane { .. } => {
                let Some(base) = model_surface_point_by_id(ir, support_id, origin.u, origin.v)
                else {
                    continue;
                };
                let Some(next) = model_surface_point_by_id(
                    ir,
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
            SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. }
                if direction.u == 0.0 && direction.v != 0.0 =>
            {
                analytic_surface_parameters(&surface.geometry, point)
                    .map(|uv| (uv.v - origin.v) / direction.v)
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
        let Some(parameter) = parameter else {
            continue;
        };
        if parameter < range[0] || parameter > range[1] {
            continue;
        }
        let Some(evaluated) = model_curve_point_by_id(ir, curve_id, parameter) else {
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
    surface_point_inner(geometry, u, v, 0)
}

fn surface_point_inner(geometry: &SurfaceGeometry, u: f64, v: f64, depth: usize) -> Option<Point3> {
    if depth > 256 {
        return None;
    }
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => Some(offset(
            *origin,
            &[(u, *u_axis), (v, cross(*normal, *u_axis))],
        )),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => Some(offset(
            *origin,
            &[
                (radius * u.cos(), *ref_direction),
                (radius * u.sin(), cross(*axis, *ref_direction)),
                (v, *axis),
            ],
        )),
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let local_radius = radius + v * half_angle.tan();
            Some(offset(
                *origin,
                &[
                    (local_radius * u.cos(), *ref_direction),
                    (local_radius * ratio * u.sin(), cross(*axis, *ref_direction)),
                    (v, *axis),
                ],
            ))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => Some(offset(
            *center,
            &[
                (radius * v.cos() * u.cos(), *ref_direction),
                (radius * v.cos() * u.sin(), cross(*axis, *ref_direction)),
                (radius * v.sin(), *axis),
            ],
        )),
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let ring = major_radius + minor_radius * v.cos();
            Some(offset(
                *center,
                &[
                    (ring * u.cos(), *ref_direction),
                    (ring * u.sin(), cross(*axis, *ref_direction)),
                    (minor_radius * v.sin(), *axis),
                ],
            ))
        }
        SurfaceGeometry::Nurbs(nurbs) => nurbs_surface_point(nurbs, u, v),
        SurfaceGeometry::Polygonal { .. } => None,
        SurfaceGeometry::Transformed { basis, transform } => {
            surface_point_inner(basis, u, v, depth + 1).map(|point| affine_point(*transform, point))
        }
        SurfaceGeometry::Procedural { .. } | SurfaceGeometry::Unknown { .. } => None,
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
    match &procedural.definition {
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            direction,
            ..
        } => {
            let curve = ir
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *directrix)?;
            curve_point(&curve.geometry, u).map(|point| offset(point, &[(v, *direction)]))
        }
        _ => None,
    }
}

/// Evaluate a surface carrier selected by arena id.
pub fn model_surface_point_by_id(
    ir: &CadIr,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
) -> Option<Point3> {
    fn point(
        ir: &CadIr,
        surface_id: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        visiting: &mut Vec<crate::ids::SurfaceId>,
    ) -> Option<Point3> {
        if visiting.contains(surface_id) {
            return None;
        }
        visiting.push(surface_id.clone());
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| candidate.id == *surface_id)?;
        let result = if let SurfaceGeometry::Procedural { construction } = &surface.geometry {
            let procedural = ir.model.procedural_surfaces.iter().find(|candidate| {
                candidate.id == *construction && candidate.surface == *surface_id
            })?;
            match &procedural.definition {
                ProceduralSurfaceDefinition::Offset {
                    support, distance, ..
                } => {
                    let support_point = point(ir, support, u, v, visiting)?;
                    let step = 1.0e-6;
                    let u0 = point(ir, support, u - step, v, visiting)?;
                    let u1 = point(ir, support, u + step, v, visiting)?;
                    let v0 = point(ir, support, u, v - step, visiting)?;
                    let v1 = point(ir, support, u, v + step, visiting)?;
                    let du = Vector3::new(u1.x - u0.x, u1.y - u0.y, u1.z - u0.z);
                    let dv = Vector3::new(v1.x - v0.x, v1.y - v0.y, v1.z - v0.z);
                    let normal = cross(du, dv);
                    let norm = normal.norm();
                    (norm.is_finite() && norm > 0.0)
                        .then(|| offset(support_point, &[(distance / norm, normal)]))
                }
                _ => model_surface_point(ir, &surface.geometry, u, v),
            }
        } else {
            surface_point(&surface.geometry, u, v)
        };
        visiting.pop();
        result
    }

    point(ir, surface, u, v, &mut Vec::new())
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

/// Evaluate a pcurve carrier at parameter `t`, yielding a surface `(u, v)`.
pub fn pcurve_uv(geometry: &PcurveGeometry, t: f64) -> Option<Point2> {
    pcurve_uv_inner(geometry, t, 0)
}

fn pcurve_uv_inner(geometry: &PcurveGeometry, t: f64, depth: usize) -> Option<Point2> {
    if depth > 256 {
        return None;
    }
    match geometry {
        PcurveGeometry::Offset { distance, basis } => {
            let differential = pcurve_uv_differential_inner(basis, t, depth + 1)?;
            let point = differential.point;
            let tangent = differential.tangent?;
            let magnitude = tangent.u.hypot(tangent.v);
            if !magnitude.is_finite() || magnitude == 0.0 {
                return None;
            }
            let point = Point2::new(
                point.u - distance * tangent.v / magnitude,
                point.v + distance * tangent.u / magnitude,
            );
            (point.u.is_finite() && point.v.is_finite()).then_some(point)
        }
        _ => {
            pcurve_uv_differential_inner(geometry, t, depth).map(|differential| differential.point)
        }
    }
}

fn pcurve_uv_differential_inner(
    geometry: &PcurveGeometry,
    t: f64,
    depth: usize,
) -> Option<PcurveDifferential> {
    if depth > 256 {
        return None;
    }
    let pair = match geometry {
        PcurveGeometry::Line { origin, direction } => (
            Point2::new(origin.u + t * direction.u, origin.v + t * direction.v),
            *direction,
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
            return Some(PcurveDifferential { point, tangent });
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
        curve_point, nurbs_curve_parameter_near_point, nurbs_curve_speed_bound,
        nurbs_surface_isocurve, nurbs_surface_partials, nurbs_surface_point, pcurve_uv,
    };
    use crate::geometry::{
        CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, SurfaceParameterAxis,
    };
    use crate::math::{Point2, Point3, Vector3};

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

        let circle = pcurve_uv(&circle, std::f64::consts::FRAC_PI_2).expect("circle evaluates");
        let ellipse = pcurve_uv(&ellipse, std::f64::consts::FRAC_PI_2).expect("ellipse evaluates");
        let polar = pcurve_uv(&polar, std::f64::consts::FRAC_PI_2).expect("polar curve evaluates");
        let polar_nurbs = pcurve_uv(&polar_nurbs, 0.5).expect("polar NURBS evaluates");
        assert!((circle.u - 2.0).abs() < 1e-12 && (circle.v + 1.0).abs() < 1e-12);
        assert!(ellipse.u.abs() < 1e-12 && (ellipse.v - 3.0).abs() < 1e-12);
        assert!((polar.u - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((polar.v - 3.0).abs() < 1e-12);
        assert!((polar_nurbs.u - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((polar_nurbs.v - 4.0).abs() < 1e-12);
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
            assert!((point.u.hypot(point.v) - 0.75).abs() < 1e-12);
        }

        let nested = PcurveGeometry::Offset {
            distance: 1.0,
            basis: Box::new(line),
        };
        assert_eq!(pcurve_uv(&nested, 0.5), None);
    }
}
