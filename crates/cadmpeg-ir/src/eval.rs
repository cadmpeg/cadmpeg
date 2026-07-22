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

use crate::geometry::{
    CurveGeometry, NurbsCurve, NurbsSurface, PcurveGeometry, ProceduralSurfaceDefinition,
    SurfaceGeometry,
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
    let degree = usize::try_from(degree).ok()?;
    let span = bspline_span(knots, degree, control_points.len(), t)?;
    let basis = bspline_basis(knots, degree, span, t);
    let mut u = 0.0;
    let mut v = 0.0;
    let mut weight_sum = 0.0;
    for (i, value) in basis.iter().enumerate() {
        let index = span - degree + i;
        let weight = weights
            .and_then(|weights| weights.get(index).copied())
            .unwrap_or(1.0);
        let pole = control_points.get(index)?;
        u += value * weight * pole.u;
        v += value * weight * pole.v;
        weight_sum += value * weight;
    }
    (weight_sum != 0.0).then(|| Point2::new(u / weight_sum, v / weight_sum))
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
        PcurveGeometry::Line { origin, direction } => Some(Point2::new(
            origin.u + t * direction.u,
            origin.v + t * direction.v,
        )),
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => Some(offset2(
            *center,
            &[(radius * t.cos(), *x_axis), (radius * t.sin(), *y_axis)],
        )),
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => Some(offset2(
            *center,
            &[
                (major_radius * t.cos(), *x_axis),
                (minor_radius * t.sin(), *y_axis),
            ],
        )),
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } if *focal_distance != 0.0 => Some(offset2(
            *vertex,
            &[(t * t / (4.0 * focal_distance), *x_axis), (t, *y_axis)],
        )),
        PcurveGeometry::Parabola { .. } => None,
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => Some(offset2(
            *center,
            &[
                (major_radius * t.cosh(), *x_axis),
                (minor_radius * t.sinh(), *y_axis),
            ],
        )),
        PcurveGeometry::PolarHarmonic {
            radial_center,
            radial_cos,
            radial_sin,
            axial_origin,
            axial_cos,
            axial_sin,
        } => {
            let cos = t.cos();
            let sin = t.sin();
            let x = radial_center.u + radial_cos.u * cos + radial_sin.u * sin;
            let y = radial_center.v + radial_cos.v * cos + radial_sin.v * sin;
            ((x != 0.0) || (y != 0.0))
                .then(|| Point2::new(y.atan2(x), axial_origin + axial_cos * cos + axial_sin * sin))
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
            let radial =
                nurbs_pcurve_uv(*degree, knots, radial_control_points, weights.as_deref(), t)?;
            let axial_points = axial_control_points
                .iter()
                .map(|value| Point2::new(*value, 0.0))
                .collect::<Vec<_>>();
            let axial = nurbs_pcurve_uv(*degree, knots, &axial_points, weights.as_deref(), t)?;
            ((radial.u != 0.0) || (radial.v != 0.0))
                .then(|| Point2::new(radial.v.atan2(radial.u), axial.u))
        }
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } => nurbs_pcurve_uv(*degree, knots, control_points, weights.as_deref(), t),
        PcurveGeometry::Trimmed { basis, .. } => pcurve_uv_inner(basis, t, depth + 1),
        // Exact offset evaluation also requires the basis tangent. The IR
        // retains the exact construction even when this point-only evaluator
        // cannot establish a stable tangent.
        PcurveGeometry::Offset { .. } => None,
    }
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
    use super::{nurbs_surface_partials, pcurve_uv};
    use crate::geometry::{NurbsSurface, PcurveGeometry};
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

        let circle = pcurve_uv(&circle, std::f64::consts::FRAC_PI_2).unwrap();
        let ellipse = pcurve_uv(&ellipse, std::f64::consts::FRAC_PI_2).unwrap();
        let polar = pcurve_uv(&polar, std::f64::consts::FRAC_PI_2).unwrap();
        let polar_nurbs = pcurve_uv(&polar_nurbs, 0.5).unwrap();
        assert!((circle.u - 2.0).abs() < 1e-12 && (circle.v + 1.0).abs() < 1e-12);
        assert!(ellipse.u.abs() < 1e-12 && (ellipse.v - 3.0).abs() < 1e-12);
        assert!((polar.u - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((polar.v - 3.0).abs() < 1e-12);
        assert!((polar_nurbs.u - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
        assert!((polar_nurbs.v - 4.0).abs() < 1e-12);
    }
}
