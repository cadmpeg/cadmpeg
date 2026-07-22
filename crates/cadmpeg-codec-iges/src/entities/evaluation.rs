// SPDX-License-Identifier: Apache-2.0
//! Exact evaluation helpers for decoded neutral carriers.

use cadmpeg_ir::geometry::{CurveGeometry, NurbsSurface, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point2, Point3, Vector3};

fn add(origin: Point3, direction: Vector3, scale: f64) -> Point3 {
    Point3::new(
        origin.x + direction.x * scale,
        origin.y + direction.y * scale,
        origin.z + direction.z * scale,
    )
}

fn cross(left: Vector3, right: Vector3) -> Vector3 {
    Vector3::new(
        left.y * right.z - left.z * right.y,
        left.z * right.x - left.x * right.z,
        left.x * right.y - left.y * right.x,
    )
}

fn basis(knots: &[f64], degree: usize, count: usize, parameter: f64) -> Option<Vec<f64>> {
    if count == 0 || knots.len() != count.checked_add(degree)?.checked_add(1)? {
        return None;
    }
    let last = count - 1;
    let span = if parameter == knots[count] {
        last
    } else {
        (degree..count).find(|index| knots[*index] <= parameter && parameter < knots[*index + 1])?
    };
    let mut values = vec![0.0; degree + 1];
    let mut left = vec![0.0; degree + 1];
    let mut right = vec![0.0; degree + 1];
    values[0] = 1.0;
    for order in 1..=degree {
        left[order] = parameter - knots[span + 1 - order];
        right[order] = knots[span + order] - parameter;
        let mut saved = 0.0;
        for index in 0..order {
            let denominator = right[index + 1] + left[order - index];
            let term = if denominator == 0.0 {
                0.0
            } else {
                values[index] / denominator
            };
            values[index] = saved + right[index + 1] * term;
            saved = left[order - index] * term;
        }
        values[order] = saved;
    }
    let mut result = vec![0.0; count];
    for (offset, value) in values.into_iter().enumerate() {
        result[span - degree + offset] = value;
    }
    Some(result)
}

pub(super) fn pcurve(geometry: &PcurveGeometry, parameter: f64) -> Option<Point2> {
    match geometry {
        PcurveGeometry::Line { origin, direction } => Some(Point2::new(
            origin.u + parameter * direction.u,
            origin.v + parameter * direction.v,
        )),
        PcurveGeometry::Circle {
            center,
            x_axis,
            y_axis,
            radius,
        } => Some(Point2::new(
            center.u + radius * (x_axis.u * parameter.cos() + y_axis.u * parameter.sin()),
            center.v + radius * (x_axis.v * parameter.cos() + y_axis.v * parameter.sin()),
        )),
        PcurveGeometry::Ellipse {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => Some(Point2::new(
            center.u
                + major_radius * x_axis.u * parameter.cos()
                + minor_radius * y_axis.u * parameter.sin(),
            center.v
                + major_radius * x_axis.v * parameter.cos()
                + minor_radius * y_axis.v * parameter.sin(),
        )),
        PcurveGeometry::Parabola {
            vertex,
            x_axis,
            y_axis,
            focal_distance,
        } => Some(Point2::new(
            vertex.u
                + focal_distance * x_axis.u * parameter * parameter
                + 2.0 * focal_distance * y_axis.u * parameter,
            vertex.v
                + focal_distance * x_axis.v * parameter * parameter
                + 2.0 * focal_distance * y_axis.v * parameter,
        )),
        PcurveGeometry::Hyperbola {
            center,
            x_axis,
            y_axis,
            major_radius,
            minor_radius,
        } => Some(Point2::new(
            center.u
                + major_radius * x_axis.u * parameter.cosh()
                + minor_radius * y_axis.u * parameter.sinh(),
            center.v
                + major_radius * x_axis.v * parameter.cosh()
                + minor_radius * y_axis.v * parameter.sinh(),
        )),
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            ..
        } => {
            let values = basis(
                knots,
                usize::try_from(*degree).ok()?,
                control_points.len(),
                parameter,
            )?;
            let mut u = 0.0;
            let mut v = 0.0;
            let mut denominator = 0.0;
            for (index, value) in values.into_iter().enumerate() {
                let weight = weights.as_ref().map_or(1.0, |weights| weights[index]);
                let coefficient = value * weight;
                u += coefficient * control_points[index].u;
                v += coefficient * control_points[index].v;
                denominator += coefficient;
            }
            (denominator != 0.0).then(|| Point2::new(u / denominator, v / denominator))
        }
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
        } => {
            let parameter = parameter.clamp(
                parameter_range[0].min(parameter_range[1]),
                parameter_range[0].max(parameter_range[1]),
            );
            pcurve(basis, parameter)
        }
        PcurveGeometry::Offset { distance, basis } => {
            let delta = f64::EPSILON.sqrt() * parameter.abs().max(1.0);
            let point = pcurve(basis, parameter)?;
            let before = pcurve(basis, parameter - delta)?;
            let after = pcurve(basis, parameter + delta)?;
            let du = after.u - before.u;
            let dv = after.v - before.v;
            let magnitude = du.hypot(dv);
            (magnitude > 0.0).then(|| {
                Point2::new(
                    point.u - distance * dv / magnitude,
                    point.v + distance * du / magnitude,
                )
            })
        }
        PcurveGeometry::PolarHarmonic { .. } | PcurveGeometry::PolarNurbs { .. } => {
            cadmpeg_ir::eval::pcurve_uv(geometry, parameter)
        }
    }
}

pub(super) fn curve(geometry: &CurveGeometry, parameter: f64) -> Option<Point3> {
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(add(*origin, *direction, parameter)),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let side = cross(*axis, *ref_direction);
            let point = add(*center, *ref_direction, radius * parameter.cos());
            Some(add(point, side, radius * parameter.sin()))
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let minor_direction = cross(*axis, *major_direction);
            let point = add(*center, *major_direction, major_radius * parameter.cos());
            Some(add(point, minor_direction, minor_radius * parameter.sin()))
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            let minor_direction = cross(*axis, *major_direction);
            let point = add(
                *vertex,
                *major_direction,
                focal_distance * parameter * parameter,
            );
            Some(add(
                point,
                minor_direction,
                2.0 * focal_distance * parameter,
            ))
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let minor_direction = cross(*axis, *major_direction);
            let point = add(*center, *major_direction, major_radius * parameter.cosh());
            Some(add(point, minor_direction, minor_radius * parameter.sinh()))
        }
        CurveGeometry::Degenerate { point } => Some(*point),
        CurveGeometry::Nurbs(nurbs) => {
            let values = basis(
                &nurbs.knots,
                usize::try_from(nurbs.degree).ok()?,
                nurbs.control_points.len(),
                parameter,
            )?;
            let mut point = Point3::new(0.0, 0.0, 0.0);
            let mut denominator = 0.0;
            for (index, value) in values.into_iter().enumerate() {
                let weight = nurbs.weights.as_ref().map_or(1.0, |weights| weights[index]);
                let coefficient = value * weight;
                point.x += coefficient * nurbs.control_points[index].x;
                point.y += coefficient * nurbs.control_points[index].y;
                point.z += coefficient * nurbs.control_points[index].z;
                denominator += coefficient;
            }
            (denominator != 0.0).then(|| {
                Point3::new(
                    point.x / denominator,
                    point.y / denominator,
                    point.z / denominator,
                )
            })
        }
        CurveGeometry::Polyline { .. } | CurveGeometry::Transformed { .. } => {
            cadmpeg_ir::eval::curve_point(geometry, parameter)
        }
        CurveGeometry::Composite { .. } | CurveGeometry::Unknown { .. } => None,
    }
}

fn nurbs_surface(surface: &NurbsSurface, uv: Point2) -> Option<Point3> {
    let u_count = usize::try_from(surface.u_count).ok()?;
    let v_count = usize::try_from(surface.v_count).ok()?;
    let u_basis = basis(
        &surface.u_knots,
        usize::try_from(surface.u_degree).ok()?,
        u_count,
        uv.u,
    )?;
    let v_basis = basis(
        &surface.v_knots,
        usize::try_from(surface.v_degree).ok()?,
        v_count,
        uv.v,
    )?;
    let mut point = Point3::new(0.0, 0.0, 0.0);
    let mut denominator = 0.0;
    for (u, u_value) in u_basis.into_iter().enumerate() {
        for (v, v_value) in v_basis.iter().copied().enumerate() {
            let index = u.checked_mul(v_count)?.checked_add(v)?;
            let weight = surface
                .weights
                .as_ref()
                .map_or(1.0, |weights| weights[index]);
            let coefficient = u_value * v_value * weight;
            point.x += coefficient * surface.control_points[index].x;
            point.y += coefficient * surface.control_points[index].y;
            point.z += coefficient * surface.control_points[index].z;
            denominator += coefficient;
        }
    }
    (denominator != 0.0).then(|| {
        Point3::new(
            point.x / denominator,
            point.y / denominator,
            point.z / denominator,
        )
    })
}

pub(super) fn surface(geometry: &SurfaceGeometry, uv: Point2) -> Option<Point3> {
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let point = add(*origin, *u_axis, uv.u);
            Some(add(point, cross(*normal, *u_axis), uv.v))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            let side = cross(*axis, *ref_direction);
            let point = add(*origin, *ref_direction, radius * uv.u.cos());
            let point = add(point, side, radius * uv.u.sin());
            Some(add(point, *axis, uv.v))
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            let side = cross(*axis, *ref_direction);
            let section_radius = radius + uv.v * half_angle.tan();
            let point = add(*origin, *axis, uv.v);
            let point = add(point, *ref_direction, section_radius * uv.u.cos());
            Some(add(point, side, section_radius * ratio * uv.u.sin()))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let side = cross(*axis, *ref_direction);
            let point = add(*center, *ref_direction, radius * uv.v.cos() * uv.u.cos());
            let point = add(point, side, radius * uv.v.cos() * uv.u.sin());
            Some(add(point, *axis, radius * uv.v.sin()))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let side = cross(*axis, *ref_direction);
            let radial = major_radius + minor_radius * uv.v.cos();
            let point = add(*center, *ref_direction, radial * uv.u.cos());
            let point = add(point, side, radial * uv.u.sin());
            Some(add(point, *axis, minor_radius * uv.v.sin()))
        }
        SurfaceGeometry::Nurbs(surface) => nurbs_surface(surface, uv),
        SurfaceGeometry::Polygonal { .. } => None,
        SurfaceGeometry::Transformed { .. } => {
            cadmpeg_ir::eval::surface_point(geometry, uv.u, uv.v)
        }
        SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(super) fn distance(left: Point3, right: Point3) -> f64 {
    ((left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)).sqrt()
}
