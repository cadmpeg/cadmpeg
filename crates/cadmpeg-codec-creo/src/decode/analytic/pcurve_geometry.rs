// SPDX-License-Identifier: Apache-2.0
//! Revolution, meridian, and ruled pcurve geometry.

use cadmpeg_ir::geometry::{CurveGeometry, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point2;

use crate::vecmath::{cross, dot};

const EPS_AGREE: f64 = 1.0e-9;
const EPS_ORTHO: f64 = 1.0e-10;
const EPS_NEAR_ZERO: f64 = 1.0e-12;

pub fn stored_unit_vector(vector: [f64; 3]) -> Option<[f64; 3]> {
    let length = dot(vector, vector).sqrt();
    (length.is_finite() && (length - 1.0).abs() <= EPS_ORTHO).then_some(vector)
}

enum RevolutionRadii {
    Cylinder(f64),
    Cone {
        radius: f64,
        ratio: f64,
        half_angle: f64,
    },
    Sphere(f64),
    Torus {
        major_radius: f64,
        minor_radius: f64,
    },
}

pub fn surface_of_revolution_parallel_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let (center, conic_axis, conic_x, conic_radii) = match geometry {
        CurveGeometry::Circle(circle_curve)
            if {
                let (_, _, _, radius) = circle_curve.parts();
                radius.is_finite() && *radius > 0.0
            } =>
        {
            let (center, axis, ref_direction, radius) = circle_curve.parts();
            (*center, *axis, *ref_direction, [*radius, *radius])
        }
        CurveGeometry::Ellipse(ellipse_curve)
            if {
                let (_, _, _, major_radius, minor_radius) = ellipse_curve.parts();
                major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius > 0.0
                    && *minor_radius > 0.0
            } =>
        {
            let (center, axis, major_direction, major_radius, minor_radius) = ellipse_curve.parts();
            (
                *center,
                *axis,
                *major_direction,
                [*major_radius, *minor_radius],
            )
        }
        _ => return None,
    };
    let (origin, axis, ref_direction, radial) = match surface {
        SurfaceGeometry::Cylinder(cylinder) if *cylinder.parts().3 > 0.0 => {
            let (origin, axis, ref_direction, radius) = cylinder.parts();
            (
                *origin,
                *axis,
                *ref_direction,
                RevolutionRadii::Cylinder(*radius),
            )
        }
        SurfaceGeometry::Cone(cone) => {
            let (origin, axis, ref_direction, radius, ratio, half_angle) = cone.parts();
            half_angle.tan().is_finite().then_some(())?;
            (
                *origin,
                *axis,
                *ref_direction,
                RevolutionRadii::Cone {
                    radius: *radius,
                    ratio: *ratio,
                    half_angle: *half_angle,
                },
            )
        }
        SurfaceGeometry::Sphere(sphere) if *sphere.parts().3 > 0.0 => {
            let (center, axis, ref_direction, radius) = sphere.parts();
            (
                *center,
                *axis,
                *ref_direction,
                RevolutionRadii::Sphere(*radius),
            )
        }
        SurfaceGeometry::Torus(torus) if *torus.parts().3 > 0.0 && *torus.parts().4 > 0.0 => {
            let (center, axis, ref_direction, major_radius, minor_radius) = torus.parts();
            (
                *center,
                *axis,
                *ref_direction,
                RevolutionRadii::Torus {
                    major_radius: *major_radius,
                    minor_radius: *minor_radius,
                },
            )
        }
        _ => return None,
    };
    let surface_axis = stored_unit_vector([axis.x, axis.y, axis.z])?;
    let surface_x = stored_unit_vector([ref_direction.x, ref_direction.y, ref_direction.z])?;
    (dot(surface_axis, surface_x).abs() <= EPS_ORTHO).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let conic_axis = stored_unit_vector([conic_axis.x, conic_axis.y, conic_axis.z])?;
    let conic_x = stored_unit_vector([conic_x.x, conic_x.y, conic_x.z])?;
    (dot(conic_axis, conic_x).abs() <= EPS_ORTHO
        && (dot(conic_axis, surface_axis).abs() - 1.0).abs() <= EPS_ORTHO)
        .then_some(())?;
    let conic_y = cross(conic_axis, conic_x);
    let center_relative = [
        center.x - origin.x,
        center.y - origin.y,
        center.z - origin.z,
    ];
    let axial = dot(center_relative, surface_axis);
    let center_radial = std::array::from_fn::<_, 3, _>(|index| {
        center_relative[index] - axial * surface_axis[index]
    });
    let (v, surface_radii) = match radial {
        RevolutionRadii::Cylinder(radius) => (axial, [radius, radius]),
        RevolutionRadii::Cone {
            radius,
            ratio,
            half_angle,
        } => {
            let local_radius = radius + axial * half_angle.tan();
            (axial, [local_radius, local_radius * ratio])
        }
        RevolutionRadii::Sphere(radius) => {
            ((conic_radii[0] - conic_radii[1]).abs()
                <= EPS_AGREE * conic_radii.into_iter().fold(1.0, f64::max))
            .then_some(())?;
            let scale = radius.abs().max(conic_radii[0]).max(1.0);
            ((axial.mul_add(axial, conic_radii[0] * conic_radii[0]) - radius * radius).abs()
                <= EPS_AGREE * scale * scale)
                .then_some(())?;
            let polar = axial.atan2(conic_radii[0]);
            let ring = radius * polar.cos();
            (polar, [ring, ring])
        }
        RevolutionRadii::Torus {
            major_radius,
            minor_radius,
        } => {
            ((conic_radii[0] - conic_radii[1]).abs()
                <= EPS_AGREE * conic_radii.into_iter().fold(1.0, f64::max))
            .then_some(())?;
            let candidates = [conic_radii[0], -conic_radii[0]]
                .into_iter()
                .filter_map(|ring| {
                    let sine = axial / minor_radius;
                    let cosine = (ring - major_radius) / minor_radius;
                    ((sine.mul_add(sine, cosine * cosine) - 1.0).abs() <= EPS_AGREE)
                        .then_some((sine.atan2(cosine), ring))
                })
                .collect::<Vec<_>>();
            let [candidate] = candidates.as_slice() else {
                return None;
            };
            (candidate.0, [candidate.1, candidate.1])
        }
    };
    let scale = surface_radii
        .into_iter()
        .chain(conic_radii)
        .map(f64::abs)
        .fold(1.0, f64::max);
    (dot(center_radial, center_radial).sqrt() <= EPS_AGREE * scale
        && surface_radii
            .iter()
            .all(|radius| radius.abs() > EPS_NEAR_ZERO * scale)
        && surface_radii
            .into_iter()
            .map(f64::abs)
            .zip(conic_radii)
            .all(|(surface_radius, conic_radius)| {
                (surface_radius - conic_radius).abs() <= EPS_AGREE * scale
            }))
    .then_some(())?;
    let radius_sign = surface_radii[0].signum();
    let phase =
        (radius_sign * dot(conic_x, surface_y)).atan2(radius_sign * dot(conic_x, surface_x));
    let surface_tangent = std::array::from_fn::<_, 3, _>(|index| {
        -phase.sin() * surface_x[index] + phase.cos() * surface_y[index]
    });
    let orientation = radius_sign * dot(conic_y, surface_tangent);
    ((orientation.abs() - 1.0).abs() <= EPS_ORTHO).then_some(())?;
    Some(PcurveGeometry::Line(
        cadmpeg_ir::geometry::LinePcurve::try_new(
            Point2::new(phase, v),
            Point2::new(orientation.signum(), 0.0),
        )
        .ok()?,
    ))
}

pub fn meridian_circle_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let (surface_center, surface_axis, surface_x, major_radius, meridian_radius) = match surface {
        SurfaceGeometry::Sphere(sphere_surface)
            if {
                let (_, _, _, radius) = sphere_surface.parts();
                radius.is_finite() && *radius > 0.0
            } =>
        {
            let (center, axis, ref_direction, radius) = sphere_surface.parts();
            (*center, *axis, *ref_direction, None, *radius)
        }
        SurfaceGeometry::Torus(torus_surface)
            if {
                let (_, _, _, major_radius, minor_radius) = torus_surface.parts();
                major_radius.is_finite()
                    && minor_radius.is_finite()
                    && *major_radius > 0.0
                    && *minor_radius > 0.0
            } =>
        {
            let (center, axis, ref_direction, major_radius, minor_radius) = torus_surface.parts();
            (
                *center,
                *axis,
                *ref_direction,
                Some(*major_radius),
                *minor_radius,
            )
        }
        _ => return None,
    };
    let CurveGeometry::Circle(circle_curve) = geometry else {
        return None;
    };
    let (circle_center, circle_axis, circle_x, circle_radius) = circle_curve.parts();
    (circle_radius.is_finite() && *circle_radius > 0.0).then_some(())?;
    let surface_axis = stored_unit_vector([surface_axis.x, surface_axis.y, surface_axis.z])?;
    let surface_x = stored_unit_vector([surface_x.x, surface_x.y, surface_x.z])?;
    (dot(surface_axis, surface_x).abs() <= EPS_ORTHO).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let circle_axis = stored_unit_vector([circle_axis.x, circle_axis.y, circle_axis.z])?;
    let circle_x = stored_unit_vector([circle_x.x, circle_x.y, circle_x.z])?;
    (dot(circle_axis, circle_x).abs() <= EPS_ORTHO).then_some(())?;
    let circle_y = cross(circle_axis, circle_x);
    let center_relative = [
        circle_center.x - surface_center.x,
        circle_center.y - surface_center.y,
        circle_center.z - surface_center.z,
    ];
    let scale = major_radius
        .unwrap_or(0.0)
        .abs()
        .max(meridian_radius.abs())
        .max(circle_radius.abs())
        .max(1.0);
    ((circle_radius - meridian_radius).abs() <= EPS_AGREE * scale).then_some(())?;
    let radial = if let Some(major_radius) = major_radius {
        let axial = dot(center_relative, surface_axis);
        let radial = std::array::from_fn::<_, 3, _>(|index| {
            center_relative[index] - axial * surface_axis[index]
        });
        let radial_length = dot(radial, radial).sqrt();
        (axial.abs() <= EPS_AGREE * scale
            && (radial_length - major_radius).abs() <= EPS_AGREE * scale)
            .then_some(())?;
        radial.map(|coordinate| coordinate / radial_length)
    } else {
        (dot(center_relative, center_relative).sqrt() <= EPS_AGREE * scale).then_some(())?;
        let radial = cross(circle_axis, surface_axis);
        stored_unit_vector(radial)?
    };
    let meridian_normal = cross(surface_axis, radial);
    ((dot(circle_axis, meridian_normal).abs() - 1.0).abs() <= EPS_ORTHO).then_some(())?;
    let u = dot(radial, surface_y).atan2(dot(radial, surface_x));
    let phase = dot(circle_x, surface_axis).atan2(dot(circle_x, radial));
    let surface_tangent = std::array::from_fn::<_, 3, _>(|index| {
        -phase.sin() * radial[index] + phase.cos() * surface_axis[index]
    });
    let orientation = dot(circle_y, surface_tangent);
    ((orientation.abs() - 1.0).abs() <= EPS_ORTHO).then_some(())?;
    Some(PcurveGeometry::Line(
        cadmpeg_ir::geometry::LinePcurve::try_new(
            Point2::new(u, phase),
            Point2::new(0.0, orientation.signum()),
        )
        .ok()?,
    ))
}

pub fn ruled_generator_line_pcurve(
    surface: &SurfaceGeometry,
    geometry: &CurveGeometry,
) -> Option<PcurveGeometry> {
    let CurveGeometry::Line(line_curve) = geometry else {
        return None;
    };
    let (line_origin, line_direction) = line_curve.parts();
    let (surface_origin, surface_axis, surface_x, reference_radius, radius_ratio, radius_slope) =
        match surface {
            SurfaceGeometry::Cylinder(cylinder_surface)
                if {
                    let (_, _, _, radius) = cylinder_surface.parts();
                    radius.is_finite() && *radius > 0.0
                } =>
            {
                let (origin, axis, ref_direction, radius) = cylinder_surface.parts();
                (*origin, *axis, *ref_direction, *radius, 1.0, 0.0)
            }
            SurfaceGeometry::Cone(cone_surface)
                if {
                    let (_, _, _, radius, ratio, half_angle) = cone_surface.parts();
                    radius.is_finite()
                        && ratio.is_finite()
                        && *ratio > 0.0
                        && half_angle.is_finite()
                } =>
            {
                let (origin, axis, ref_direction, radius, ratio, half_angle) = cone_surface.parts();
                let slope = half_angle.tan();
                slope.is_finite().then_some((
                    *origin,
                    *axis,
                    *ref_direction,
                    *radius,
                    *ratio,
                    slope,
                ))?
            }
            _ => return None,
        };
    let surface_axis = stored_unit_vector([surface_axis.x, surface_axis.y, surface_axis.z])?;
    let surface_x = stored_unit_vector([surface_x.x, surface_x.y, surface_x.z])?;
    (dot(surface_axis, surface_x).abs() <= EPS_ORTHO).then_some(())?;
    let surface_y = cross(surface_axis, surface_x);
    let relative = [
        line_origin.x - surface_origin.x,
        line_origin.y - surface_origin.y,
        line_origin.z - surface_origin.z,
    ];
    let v = dot(relative, surface_axis);
    let radial = std::array::from_fn::<_, 3, _>(|index| relative[index] - v * surface_axis[index]);
    let local_radius = reference_radius + v * radius_slope;
    let scale = local_radius
        .abs()
        .max((local_radius * radius_ratio).abs())
        .max(1.0);
    (local_radius.abs() > EPS_NEAR_ZERO * scale).then_some(())?;
    let chart_x = dot(radial, surface_x) / local_radius;
    let chart_y = dot(radial, surface_y) / (local_radius * radius_ratio);
    (chart_x.is_finite()
        && chart_y.is_finite()
        && (chart_x.mul_add(chart_x, chart_y * chart_y) - 1.0).abs() <= EPS_AGREE)
        .then_some(())?;
    let u = chart_y.atan2(chart_x);
    let chart_radial = std::array::from_fn::<_, 3, _>(|index| {
        chart_x * surface_x[index] + radius_ratio * chart_y * surface_y[index]
    });
    let surface_derivative = std::array::from_fn::<_, 3, _>(|index| {
        surface_axis[index] + radius_slope * chart_radial[index]
    });
    let line_direction = [line_direction.x, line_direction.y, line_direction.z];
    let direction_length = dot(line_direction, line_direction).sqrt();
    let derivative_norm = dot(surface_derivative, surface_derivative);
    (direction_length.is_finite()
        && direction_length > 0.0
        && derivative_norm.is_finite()
        && derivative_norm > 0.0)
        .then_some(())?;
    let parameter_scale = dot(line_direction, surface_derivative) / derivative_norm;
    let residual = std::array::from_fn::<_, 3, _>(|index| {
        line_direction[index] - parameter_scale * surface_derivative[index]
    });
    (parameter_scale.is_finite()
        && parameter_scale.abs() > 0.0
        && dot(residual, residual).sqrt() <= EPS_ORTHO * direction_length)
        .then_some(())?;
    Some(PcurveGeometry::Line(
        cadmpeg_ir::geometry::LinePcurve::try_new(
            Point2::new(u, v),
            Point2::new(0.0, parameter_scale),
        )
        .ok()?,
    ))
}
