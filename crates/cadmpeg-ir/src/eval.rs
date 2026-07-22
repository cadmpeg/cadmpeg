// SPDX-License-Identifier: Apache-2.0
//! Point evaluation of geometry carriers.
//!
//! Evaluators map carrier parameters to model-space (or parameter-space)
//! points using the carriers' own parameterizations: conic parameters are
//! angles from the reference/major direction, line parameters are signed
//! distances along the unit direction, and B-splines evaluate by Cox–de Boor
//! over their stored knot vectors. Carriers without a typed parameterization
//! ([`CurveGeometry::Unknown`], [`CurveGeometry::Composite`],
//! [`SurfaceGeometry::Unknown`], parabolas, and hyperbolas) evaluate to `None`.

use crate::geometry::{CurveGeometry, NurbsSurface, PcurveGeometry, SurfaceGeometry};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform;

fn cross(a: Vector3, b: Vector3) -> Vector3 {
    Vector3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
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

/// Evaluate a tensor-product NURBS surface at `(u, v)`.
pub fn nurbs_surface_point(surface: &NurbsSurface, u_at: f64, v_at: f64) -> Option<Point3> {
    let u_degree = usize::try_from(surface.u_degree).ok()?;
    let v_degree = usize::try_from(surface.v_degree).ok()?;
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    if surface.control_points.len() != u_count.checked_mul(v_count)? {
        return None;
    }
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
        CurveGeometry::Nurbs(nurbs) => nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            nurbs.weights.as_deref(),
            t,
        ),
        CurveGeometry::Polyline {
            points, parameters, ..
        } => polyline_point(points, parameters.as_deref(), t),
        CurveGeometry::Transformed { basis, transform } => {
            curve_point_inner(basis, t, depth + 1).map(|point| affine_point(*transform, point))
        }
        CurveGeometry::Composite { .. } | CurveGeometry::Unknown { .. } => None,
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
        SurfaceGeometry::Unknown { .. } => None,
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
    use super::pcurve_uv;
    use crate::geometry::PcurveGeometry;
    use crate::math::Point2;

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
