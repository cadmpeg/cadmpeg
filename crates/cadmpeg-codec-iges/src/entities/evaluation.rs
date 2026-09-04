// SPDX-License-Identifier: Apache-2.0
//! Exact evaluation helpers for decoded neutral carriers.

use cadmpeg_core::decode::alloc_filled;
use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry};
use cadmpeg_ir::math::{Point2, Point3};

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
    let degree_slots = degree.checked_add(1)?;
    let mut values = alloc_filled(degree_slots, 0.0, "iges basis values").ok()?;
    let mut left = alloc_filled(degree_slots, 0.0, "iges basis left knots").ok()?;
    let mut right = alloc_filled(degree_slots, 0.0, "iges basis right knots").ok()?;
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
    let mut result = alloc_filled(count, 0.0, "iges basis result").ok()?;
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
        PcurveGeometry::Harmonic {
            center,
            cosine,
            sine,
        } => Some(Point2::new(
            center.u + cosine.u * parameter.cos() + sine.u * parameter.sin(),
            center.v + cosine.v * parameter.cos() + sine.v * parameter.sin(),
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
        PcurveGeometry::Hyperbolic {
            center,
            cosine,
            sine,
        } => Some(Point2::new(
            center.u + cosine.u * parameter.cosh() + sine.u * parameter.sinh(),
            center.v + cosine.v * parameter.cosh() + sine.v * parameter.sinh(),
        )),
        PcurveGeometry::Nurbs { nurbs } => {
            let values = basis(
                nurbs.knots(),
                usize::try_from(nurbs.degree()).ok()?,
                nurbs.control_points().len(),
                parameter,
            )?;
            let mut u = 0.0;
            let mut v = 0.0;
            let mut denominator = 0.0;
            for (index, value) in values.into_iter().enumerate() {
                let weight = nurbs.weights().map_or(1.0, |weights| weights[index]);
                let coefficient = value * weight;
                u += coefficient * nurbs.control_points()[index].u;
                v += coefficient * nurbs.control_points()[index].v;
                denominator += coefficient;
            }
            (denominator != 0.0).then(|| Point2::new(u / denominator, v / denominator))
        }
        PcurveGeometry::Trimmed {
            parameter_range,
            basis,
            ..
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
        PcurveGeometry::Transformed { basis, transform } => {
            pcurve(basis, parameter).map(|point| transform.apply_point(point))
        }
        PcurveGeometry::PolarHarmonic { .. }
        | PcurveGeometry::PolarNurbs { .. }
        | PcurveGeometry::SphericalGreatCircle { .. } => {
            cadmpeg_ir::eval::pcurve_uv(geometry, parameter)
        }
    }
}

pub(super) fn curve(geometry: &CurveGeometry, parameter: f64) -> Option<Point3> {
    match geometry {
        CurveGeometry::Line { origin, direction } => Some(origin.translated(*direction, parameter)),
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let side = axis.cross(*ref_direction);
            let point = center.translated(*ref_direction, radius * parameter.cos());
            Some(point.translated(side, radius * parameter.sin()))
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let minor_direction = axis.cross(*major_direction);
            let point = center.translated(*major_direction, major_radius * parameter.cos());
            Some(point.translated(minor_direction, minor_radius * parameter.sin()))
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            let minor_direction = axis.cross(*major_direction);
            let point = vertex.translated(*major_direction, focal_distance * parameter * parameter);
            Some(point.translated(minor_direction, 2.0 * focal_distance * parameter))
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let minor_direction = axis.cross(*major_direction);
            let point = center.translated(*major_direction, major_radius * parameter.cosh());
            Some(point.translated(minor_direction, minor_radius * parameter.sinh()))
        }
        CurveGeometry::Degenerate { point } => Some(*point),
        CurveGeometry::Nurbs(nurbs) => {
            let values = basis(
                nurbs.knots(),
                usize::try_from(nurbs.degree()).ok()?,
                nurbs.control_points().len(),
                parameter,
            )?;
            let mut point = Point3::new(0.0, 0.0, 0.0);
            let mut denominator = 0.0;
            for (index, value) in values.into_iter().enumerate() {
                let weight = nurbs.weights().map_or(1.0, |weights| weights[index]);
                let coefficient = value * weight;
                point.x += coefficient * nurbs.control_points()[index].x;
                point.y += coefficient * nurbs.control_points()[index].y;
                point.z += coefficient * nurbs.control_points()[index].z;
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
        CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => None,
    }
}

pub(super) fn distance(left: Point3, right: Point3) -> f64 {
    ((left.x - right.x).powi(2) + (left.y - right.y).powi(2) + (left.z - right.z).powi(2)).sqrt()
}
