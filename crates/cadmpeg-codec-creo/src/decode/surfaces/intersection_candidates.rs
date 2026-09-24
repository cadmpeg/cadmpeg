// SPDX-License-Identifier: Apache-2.0
//! Parallel, coaxial, and meridian intersection candidate families.

use crate::vecmath::normalize;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{planar::line_circle_intersections, Point2, Point3, Vector3};

use crate::decode::analytic::equations::{circular_cone, CarrierEquation};
use crate::vecmath::{cross, dot};

const EPS_APEX_PLANE_ROUNDOFF: f64 = 8.0 * f64::EPSILON;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-10;
const EPS_CENTER_ALIGNMENT: f64 = 1.0e-9;
const EPS_TANGENT_ROOT: f64 = 1.0e-9;
const EPS_NONZERO_SLOPE: f64 = 1.0e-12;
const EPS_POSITIVE_RADIUS: f64 = 1.0e-12;
const EPS_AXIS_ORTHO: f64 = 1.0e-10;
const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;
const EPS_DISTANCE_NONZERO: f64 = 1.0e-12;
const EPS_PLANE_CYLINDER_SECANT_SQUARED: f64 = 1e-18;
const EPS_HEIGHT_RESIDUAL: f64 = 1.0e-12;
const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_SLOPE_NONZERO: f64 = 1.0e-12;
const EPS_METRIC_AGREEMENT: f64 = 1.0e-10;
const EPS_DETERMINANT: f64 = 1.0e-12;
const EPS_PARAMETER_DEDUP: f64 = 1.0e-9;

pub(in super::super) fn parallel_plane_cylinder_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
    | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(normal) = normalize(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalize(cylinder.axis) else {
        return Vec::new();
    };
    if dot(normal, axis).abs() > EPS_AXIS_ORTHO || cylinder.radius <= 0.0 {
        return Vec::new();
    }
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
    );
    let scale = cylinder.radius.max(signed_distance.abs());
    let radius = cylinder.radius / scale;
    let distance = signed_distance / scale;
    let offset_squared = (radius - distance.abs()) * (radius + distance.abs());
    if offset_squared <= EPS_PLANE_CYLINDER_SECANT_SQUARED {
        return Vec::new();
    }
    let closest: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - signed_distance * normal[index]);
    let Some(transverse) = normalize(cross(axis, normal)) else {
        return Vec::new();
    };
    let offset = offset_squared.sqrt() * scale;
    [-1.0, 1.0]
        .into_iter()
        .filter_map(|sense| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| closest[index] + sense * offset * transverse[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::from(origin),
                        Vector3::from(axis),
                    )
                    .ok()?,
                )),
                "plane_cylinder_secant_generator",
            ))
        })
        .collect()
}

pub(in super::super) fn parallel_cylinder_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(first_axis), Some(second_axis)) = (normalize(first.axis), normalize(second.axis))
    else {
        return Vec::new();
    };
    if (dot(first_axis, second_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO
        || first.radius <= 0.0
        || second.radius <= 0.0
    {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.origin[index] - first.origin[index]);
    let axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
    let distance = transverse[0].hypot(transverse[1]).hypot(transverse[2]);
    let scale = first.radius.max(second.radius).max(distance);
    let first_radius = first.radius / scale;
    let second_radius = second.radius / scale;
    let separation = distance / scale;
    if separation <= EPS_DISTANCE_NONZERO
        || separation >= first_radius + second_radius - EPS_GEOMETRY_AGREEMENT
        || separation <= (first_radius - second_radius).abs() + EPS_GEOMETRY_AGREEMENT
    {
        return Vec::new();
    }
    let center_direction = transverse.map(|value| value / distance);
    let along = 0.5
        * (separation
            + (first_radius - second_radius) * (first_radius + second_radius) / separation);
    let height_squared = (first_radius - along) * (first_radius + along);
    if height_squared <= EPS_HEIGHT_RESIDUAL {
        return Vec::new();
    }
    let Some(perpendicular) = normalize(cross(first_axis, center_direction)) else {
        return Vec::new();
    };
    let along = along * scale;
    let base: [f64; 3] =
        std::array::from_fn(|index| first.origin[index] + along * center_direction[index]);
    let height = height_squared.sqrt() * scale;
    [-height, height]
        .into_iter()
        .filter_map(|offset| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| base[index] + offset * perpendicular[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::from(origin),
                        Vector3::from(first_axis),
                    )
                    .ok()?,
                )),
                "parallel_cylinder_secant_generator",
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_cylinder_sphere_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
    | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(axis) = normalize(cylinder.axis) else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
    let axial = dot(relative, axis);
    let transverse: [f64; 3] = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let scale = sphere.radius.max(cylinder.radius);
    if sphere.radius <= 0.0
        || cylinder.radius <= 0.0
        || transverse[0].hypot(transverse[1]).hypot(transverse[2]) > EPS_CENTER_ALIGNMENT * scale
    {
        return Vec::new();
    }
    let sphere_radius = sphere.radius / scale;
    let cylinder_radius = cylinder.radius / scale;
    let offset_squared = (sphere_radius - cylinder_radius) * (sphere_radius + cylinder_radius);
    let offset_tolerance = EPS_TANGENT_ROOT;
    if offset_squared < -offset_tolerance {
        return Vec::new();
    }
    let Some(reference) = normalize(cylinder.ref_direction) else {
        return Vec::new();
    };
    let (offsets, tag) = if offset_squared.abs() <= offset_tolerance {
        (vec![0.0], "coaxial_cylinder_sphere_tangent_circle")
    } else {
        let offset = offset_squared.sqrt() * scale;
        (
            vec![-offset, offset],
            "coaxial_cylinder_sphere_secant_circle",
        )
    };
    offsets
        .into_iter()
        .filter_map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + offset * axis[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        cylinder.radius,
                    )
                    .ok()?,
                )),
                tag,
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_cone_cylinder_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Cylinder(cylinder))
    | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let (Some(cone_axis), Some(cylinder_axis), Some(reference)) = (
        normalize(cone.axis()),
        normalize(cylinder.axis),
        normalize(cone.ref_direction()),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, cylinder_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - cone.origin()[index]);
    let axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
    let scale = cone.radius().max(cylinder.radius);
    let slope = cone.half_angle().tan();
    if Vector3::from(transverse).norm() > EPS_GEOMETRY_AGREEMENT * scale
        || cylinder.radius <= EPS_RADIUS_NONZERO * scale
        || cone.radius() < 0.0
        || slope.abs() <= EPS_SLOPE_NONZERO
    {
        return Vec::new();
    }
    [cylinder.radius, -cylinder.radius]
        .into_iter()
        .filter_map(|signed_radius| {
            let parameter = (signed_radius - cone.radius()) / slope;
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin()[index] + parameter * cone_axis[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(cone_axis),
                        Vector3::from(reference),
                        cylinder.radius,
                    )
                    .ok()?,
                )),
                "coaxial_cone_cylinder_secant_circle",
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_cones_section_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Cone(first), CarrierEquation::Cone(second)) = (first, second) else {
        return Vec::new();
    };
    let (Some(first_axis), Some(second_axis), Some(reference), Some(second_reference)) = (
        normalize(first.axis()),
        normalize(second.axis()),
        normalize(first.ref_direction()),
        normalize(second.ref_direction()),
    ) else {
        return Vec::new();
    };
    let axis_alignment = dot(first_axis, second_axis);
    if (axis_alignment.abs() - 1.0).abs() > EPS_AXIS_ORTHO
        || dot(first_axis, reference).abs() > EPS_AXIS_ORTHO
        || dot(second_axis, second_reference).abs() > EPS_AXIS_ORTHO
    {
        return Vec::new();
    }
    let first_y = cross(first_axis, reference);
    let second_y = cross(second_axis, second_reference);
    let second_metric = |direction: [f64; 3]| {
        let x = dot(direction, second_reference);
        let y = dot(direction, second_y) / second.ratio();
        x.mul_add(x, y * y)
    };
    let metric_xx = second_metric(reference);
    let metric_yy = second_metric(first_y);
    let metric_xy = dot(reference, second_reference).mul_add(
        dot(first_y, second_reference),
        dot(reference, second_y) * dot(first_y, second_y) / (second.ratio() * second.ratio()),
    );
    let metric_scale_squared = metric_xx;
    let metric_coefficient_scale = metric_xx.abs().max(metric_yy.abs()).max(1.0);
    if metric_scale_squared <= 0.0
        || !metric_scale_squared.is_finite()
        || !metric_yy.is_finite()
        || !metric_xy.is_finite()
        || metric_xy.abs() > EPS_METRIC_AGREEMENT * metric_coefficient_scale
        || (metric_yy - metric_scale_squared / (first.ratio() * first.ratio())).abs()
            > EPS_METRIC_AGREEMENT * metric_coefficient_scale
    {
        return Vec::new();
    }
    let metric_scale = metric_scale_squared.sqrt();
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.origin()[index] - first.origin()[index]);
    let second_origin_axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - second_origin_axial * first_axis[index]);
    let scale = first
        .radius()
        .max(second.radius())
        .max(second_origin_axial.abs());
    let first_slope = first.half_angle().tan();
    let second_slope = axis_alignment * second.half_angle().tan();
    let second_intercept = second.radius() - second_slope * second_origin_axial;
    if Vector3::from(transverse).norm() > EPS_GEOMETRY_AGREEMENT * scale
        || first.radius() < 0.0
        || second.radius() < 0.0
        || first_slope.abs() <= EPS_SLOPE_NONZERO
        || second_slope.abs() <= EPS_SLOPE_NONZERO
    {
        return Vec::new();
    }

    let mut parameters = Vec::<f64>::new();
    let scaled_first_slope = metric_scale * first_slope;
    let scaled_first_radius = metric_scale * first.radius();
    let slope_scale = scaled_first_slope.abs().max(second_slope.abs()).max(1.0);
    let intercept_scale = first
        .radius()
        .max(scaled_first_radius.abs())
        .max(second_intercept.abs())
        .max(second.radius());
    for radial_sense in [-1.0, 1.0] {
        let denominator = scaled_first_slope - radial_sense * second_slope;
        let numerator = radial_sense * second_intercept - scaled_first_radius;
        if denominator.abs() <= EPS_DETERMINANT * slope_scale {
            if numerator.abs() <= EPS_GEOMETRY_AGREEMENT * intercept_scale {
                return Vec::new();
            }
            continue;
        }
        let parameter = numerator / denominator;
        let radius = (first.radius() + parameter * first_slope).abs();
        if radius <= EPS_RADIUS_NONZERO * scale {
            continue;
        }
        if !parameters
            .iter()
            .any(|known| (parameter - known).abs() <= EPS_PARAMETER_DEDUP * scale)
        {
            parameters.push(parameter);
        }
    }
    parameters
        .into_iter()
        .filter_map(|parameter| {
            let radius = (first.radius() + parameter * first_slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| first.origin()[index] + parameter * first_axis[index]);
            let (geometry, tag) = if circular_cone(first) {
                (
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                            Point3::from(center),
                            Vector3::from(first_axis),
                            Vector3::from(reference),
                            radius,
                        )
                        .ok()?,
                    )),
                    "coaxial_cones_circle",
                )
            } else {
                (
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                            Point3::from(center),
                            Vector3::from(first_axis),
                            Vector3::from(reference),
                            radius,
                            radius * first.ratio(),
                        )
                        .ok()?,
                    )),
                    "coaxial_cones_ellipse",
                )
            };
            Some((geometry, tag))
        })
        .collect()
}

pub(in super::super) fn apex_plane_cone_generator_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
    | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let Some(normal) = normalize(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalize(cone.axis()) else {
        return Vec::new();
    };
    let Some(x_axis) = normalize(cone.ref_direction()) else {
        return Vec::new();
    };
    let slope = cone.half_angle().tan();
    if slope <= EPS_SLOPE_NONZERO || cone.radius() < 0.0 || dot(axis, x_axis).abs() > EPS_AXIS_ORTHO
    {
        return Vec::new();
    }
    let apex: [f64; 3] =
        std::array::from_fn(|index| cone.origin()[index] - cone.radius() / slope * axis[index]);
    let plane_offset = std::array::from_fn(|index| apex[index] - plane.origin[index]);
    let plane_distance = dot(normal, plane_offset);
    let scale = cone.radius().max((cone.radius() / slope).abs());
    // An apex-based cone has no reference radius. Retain only the arithmetic
    // error bound of the plane dot product when its geometric scale is zero.
    let roundoff = normal
        .into_iter()
        .zip(plane_offset)
        .map(|(a, b)| EPS_APEX_PLANE_ROUNDOFF * (a * b).abs())
        .sum::<f64>();
    if !plane_distance.is_finite()
        || plane_distance.abs() > (EPS_GEOMETRY_AGREEMENT * scale).max(roundoff)
    {
        return Vec::new();
    }
    let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::from(normal));
    let plane_u = [reference.x, reference.y, reference.z];
    let plane_v = cross(normal, plane_u);
    let y_axis = cross(axis, x_axis);
    let cone_coordinates = |direction: [f64; 3]| {
        [
            dot(direction, x_axis),
            dot(direction, y_axis) / cone.ratio(),
            dot(direction, axis),
        ]
    };
    let quadratic = |first: [f64; 3], second: [f64; 3]| {
        first[0].mul_add(
            second[0],
            first[1] * second[1] - slope * slope * first[2] * second[2],
        )
    };
    let u_coordinates = cone_coordinates(plane_u);
    let v_coordinates = cone_coordinates(plane_v);
    let quadratic_uu = quadratic(u_coordinates, u_coordinates);
    let quadratic_uv = quadratic(u_coordinates, v_coordinates);
    let quadratic_vv = quadratic(v_coordinates, v_coordinates);
    let coefficient_scale = quadratic_uu
        .abs()
        .max(quadratic_uv.abs())
        .max(quadratic_vv.abs())
        .max(1.0);
    let determinant = quadratic_uu.mul_add(quadratic_vv, -quadratic_uv * quadratic_uv);
    let determinant_tolerance = EPS_DETERMINANT * coefficient_scale * coefficient_scale;
    if determinant > determinant_tolerance {
        return Vec::new();
    }
    let angle = 0.5 * (2.0 * quadratic_uv).atan2(quadratic_uu - quadratic_vv);
    let (sine, cosine) = angle.sin_cos();
    let first_direction: [f64; 3] =
        std::array::from_fn(|index| cosine * plane_u[index] + sine * plane_v[index]);
    let second_direction: [f64; 3] =
        std::array::from_fn(|index| -sine * plane_u[index] + cosine * plane_v[index]);
    let first_value = quadratic_uu * cosine * cosine
        + 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * sine * sine;
    let second_value = quadratic_uu * sine * sine - 2.0 * quadratic_uv * cosine * sine
        + quadratic_vv * cosine * cosine;
    let directions = if determinant.abs() <= determinant_tolerance {
        if first_value.abs() <= second_value.abs() {
            vec![first_direction]
        } else {
            vec![second_direction]
        }
    } else {
        let (negative_value, negative_direction, positive_value, positive_direction) =
            if first_value < 0.0 {
                (first_value, first_direction, second_value, second_direction)
            } else {
                (second_value, second_direction, first_value, first_direction)
            };
        let negative_weight = positive_value.sqrt();
        let positive_weight = (-negative_value).sqrt();
        [-1.0, 1.0]
            .into_iter()
            .filter_map(|sense| {
                normalize(std::array::from_fn(|index| {
                    negative_weight * negative_direction[index]
                        + sense * positive_weight * positive_direction[index]
                }))
            })
            .collect()
    };
    let tag = if directions.len() == 1 {
        "plane_cone_tangent_line"
    } else {
        "plane_cone_secant_generator"
    };
    directions
        .into_iter()
        .filter_map(|direction| {
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::from(apex),
                        Vector3::from(direction),
                    )
                    .ok()?,
                )),
                tag,
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_cone_sphere_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
    | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let Some(axis) = normalize(cone.axis()) else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| sphere.center[index] - cone.origin()[index]);
    let sphere_axial = dot(relative, axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - sphere_axial * axis[index]);
    let scale = cone.radius().max(sphere.radius).max(sphere_axial.abs());
    if transverse[0].hypot(transverse[1]).hypot(transverse[2]) > EPS_CENTER_ALIGNMENT * scale {
        return Vec::new();
    }
    let slope = cone.half_angle().tan();
    if slope.abs() <= EPS_NONZERO_SLOPE || cone.radius() < 0.0 {
        return Vec::new();
    }
    let radial_origin = cone.radius() / scale;
    let Some(parameters) = line_circle_intersections(
        Point2::new(radial_origin, 0.0),
        Point2::new(radial_origin + slope, 1.0),
        Point2::new(0.0, sphere_axial / scale),
        sphere.radius / scale,
    )
    .map(|hits| hits.map(|(parameter, _)| parameter.get())) else {
        return Vec::new();
    };
    let Some(reference) = normalize(cone.ref_direction()) else {
        return Vec::new();
    };
    let tag = if parameters[0] == parameters[1] {
        "coaxial_cone_sphere_tangent_circle"
    } else {
        "coaxial_cone_sphere_secant_circle"
    };
    parameters
        .into_iter()
        .enumerate()
        .filter(|(index, parameter)| *index == 0 || *parameter != parameters[0])
        .filter_map(|(_, parameter)| {
            let parameter = parameter * scale;
            let radius = (cone.radius() + parameter * slope).abs();
            if radius <= EPS_POSITIVE_RADIUS * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin()[index] + parameter * axis[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        radius,
                    )
                    .ok()?,
                )),
                tag,
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_cone_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cone(cone), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Cone(cone))) = (first, second)
    else {
        return Vec::new();
    };
    if !circular_cone(cone) {
        return Vec::new();
    }
    let (Some(cone_axis), Some(torus_axis), Some(reference)) = (
        normalize(cone.axis()),
        normalize(torus.axis),
        normalize(cone.ref_direction()),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, torus_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - cone.origin()[index]);
    let torus_axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - torus_axial * cone_axis[index]);
    let scale = cone
        .radius()
        .max(torus.major_radius)
        .max(torus.minor_radius)
        .max(torus_axial.abs());
    let slope = cone.half_angle().tan();
    if transverse[0].hypot(transverse[1]).hypot(transverse[2]) > EPS_GEOMETRY_AGREEMENT * scale
        || cone.radius() < 0.0
        || torus.major_radius <= EPS_RADIUS_NONZERO * scale
        || torus.minor_radius <= EPS_RADIUS_NONZERO * scale
        || slope.abs() <= EPS_SLOPE_NONZERO
    {
        return Vec::new();
    }

    let mut parameters = Vec::<f64>::new();
    for radial_sense in [-1.0, 1.0] {
        let radial_origin = radial_sense * cone.radius() / scale;
        let Some(roots) = line_circle_intersections(
            Point2::new(radial_origin, 0.0),
            Point2::new(radial_origin + radial_sense * slope, 1.0),
            Point2::new(torus.major_radius / scale, torus_axial / scale),
            torus.minor_radius / scale,
        )
        .map(|hits| hits.map(|(parameter, _)| parameter.get())) else {
            continue;
        };
        for root in roots {
            let parameter = root * scale;
            let radius = radial_sense * (cone.radius() + parameter * slope);
            if radius <= EPS_RADIUS_NONZERO * scale {
                continue;
            }
            if !parameters
                .iter()
                .any(|known| (parameter - known).abs() <= EPS_PARAMETER_DEDUP * scale)
            {
                parameters.push(parameter);
            }
        }
    }
    parameters
        .into_iter()
        .filter_map(|parameter| {
            let radius = (cone.radius() + parameter * slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin()[index] + parameter * cone_axis[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(cone_axis),
                        Vector3::from(reference),
                        radius,
                    )
                    .ok()?,
                )),
                "coaxial_cone_torus_circle",
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_cylinder_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(cylinder_axis), Some(torus_axis), Some(reference)) = (
        normalize(cylinder.axis),
        normalize(torus.axis),
        normalize(cylinder.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > EPS_AXIS_ALIGNMENT {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - cylinder.origin[index]);
    let axial = dot(relative, cylinder_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * cylinder_axis[index]);
    let scale = torus
        .major_radius
        .max(torus.minor_radius)
        .max(cylinder.radius);
    if transverse[0].hypot(transverse[1]).hypot(transverse[2]) > EPS_CENTER_ALIGNMENT * scale {
        return Vec::new();
    }
    let radial_delta = cylinder.radius - torus.major_radius;
    let radial_scale = torus.minor_radius.max(radial_delta.abs());
    let minor = torus.minor_radius / radial_scale;
    let delta = radial_delta / radial_scale;
    let height_squared = (minor - delta.abs()) * (minor + delta.abs());
    let height_tolerance = EPS_TANGENT_ROOT;
    if height_squared < -height_tolerance || cylinder.radius <= EPS_POSITIVE_RADIUS * scale {
        return Vec::new();
    }
    let (offsets, tag) = if height_squared.abs() <= height_tolerance {
        (vec![0.0], "coaxial_cylinder_torus_tangent_circle")
    } else {
        let height = height_squared.sqrt() * radial_scale;
        (
            vec![-height, height],
            "coaxial_cylinder_torus_secant_circle",
        )
    };
    offsets
        .into_iter()
        .filter_map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + offset * torus_axis[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(torus_axis),
                        Vector3::from(reference),
                        cylinder.radius,
                    )
                    .ok()?,
                )),
                tag,
            ))
        })
        .collect()
}

pub(in super::super) fn axis_normal_plane_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(normal), Some(axis), Some(reference)) = (
        normalize(plane.normal),
        normalize(torus.axis),
        normalize(torus.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(normal, axis).abs() - 1.0).abs() > EPS_AXIS_ALIGNMENT {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| plane.origin[index] - torus.center[index]);
    let axial = dot(relative, axis);
    let scale = torus.major_radius.max(torus.minor_radius);
    let radial_scale = torus.minor_radius.max(axial.abs());
    let minor = torus.minor_radius / radial_scale;
    let height = axial / radial_scale;
    let radial_offset_squared = (minor - height.abs()) * (minor + height.abs());
    let radial_offset_tolerance = EPS_TANGENT_ROOT;
    if radial_offset_squared < -radial_offset_tolerance {
        return Vec::new();
    }
    let center: [f64; 3] = std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
    let (radii, tag) = if radial_offset_squared.abs() <= radial_offset_tolerance {
        (vec![torus.major_radius], "plane_torus_tangent_circle")
    } else {
        let radial_offset = radial_offset_squared.sqrt() * radial_scale;
        (
            vec![
                torus.major_radius - radial_offset,
                torus.major_radius + radial_offset,
            ],
            "plane_torus_secant_circle",
        )
    };
    radii
        .into_iter()
        .filter(|radius| *radius > EPS_POSITIVE_RADIUS * scale)
        .filter_map(|radius| {
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        radius,
                    )
                    .ok()?,
                )),
                tag,
            ))
        })
        .collect()
}

fn meridian_circle_intersections(
    first_center: [f64; 2],
    first_radius: f64,
    second_center: [f64; 2],
    second_radius: f64,
) -> Vec<[f64; 2]> {
    use cadmpeg_ir::math::{planar::circle_intersections, Point2};
    circle_intersections(
        Point2::new(first_center[0], first_center[1]),
        first_radius,
        Point2::new(second_center[0], second_center[1]),
        second_radius,
    )
    .unwrap_or_default()
    .into_iter()
    .map(|point| [point.u, point.v])
    .collect()
}

pub(in super::super) fn axis_containing_plane_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(normal), Some(axis)) = (normalize(plane.normal), normalize(torus.axis)) else {
        return Vec::new();
    };
    let scale = torus.major_radius.max(torus.minor_radius);
    let center_offset: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - plane.origin[index]);
    if dot(normal, axis).abs() > EPS_AXIS_ORTHO
        || dot(normal, center_offset).abs() > EPS_GEOMETRY_AGREEMENT * scale
        || !torus.major_radius.is_finite()
        || !torus.minor_radius.is_finite()
        || torus.major_radius <= EPS_RADIUS_NONZERO * scale
        || torus.minor_radius <= EPS_RADIUS_NONZERO * scale
    {
        return Vec::new();
    }
    let Some(radial) = normalize(cross(normal, axis)) else {
        return Vec::new();
    };
    [-1.0, 1.0]
        .into_iter()
        .filter_map(|sense| {
            let center: [f64; 3] = std::array::from_fn(|index| {
                torus.center[index] + sense * torus.major_radius * radial[index]
            });
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(normal),
                        Vector3::from(axis),
                        torus.minor_radius,
                    )
                    .ok()?,
                )),
                "axis_containing_plane_torus_meridian_circle",
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_sphere_torus_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let ((CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
    | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere))) = (first, second)
    else {
        return Vec::new();
    };
    let (Some(axis), Some(reference)) = (normalize(torus.axis), normalize(torus.ref_direction))
    else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
    let axial = dot(relative, axis);
    let transverse: [f64; 3] = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let scale = torus
        .major_radius
        .max(torus.minor_radius)
        .max(sphere.radius);
    if Vector3::from(transverse).norm() > EPS_CENTER_ALIGNMENT * scale {
        return Vec::new();
    }
    let intersections = meridian_circle_intersections(
        [0.0, 0.0],
        sphere.radius,
        [torus.major_radius, axial],
        torus.minor_radius,
    );
    let tag = match intersections.len() {
        1 => "coaxial_sphere_torus_tangent_circle",
        2 => "coaxial_sphere_torus_secant_circle",
        _ => return Vec::new(),
    };
    intersections
        .into_iter()
        .filter_map(|[radius, center_axial]| {
            let radius = radius.abs();
            if radius <= EPS_POSITIVE_RADIUS * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        radius,
                    )
                    .ok()?,
                )),
                tag,
            ))
        })
        .collect()
}

pub(in super::super) fn coaxial_tori_circle_candidates(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Vec<(CurveGeometry, &'static str)> {
    let (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) = (first, second) else {
        return Vec::new();
    };
    let (Some(first_axis), Some(second_axis), Some(reference)) = (
        normalize(first.axis),
        normalize(second.axis),
        normalize(first.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(first_axis, second_axis).abs() - 1.0).abs() > EPS_AXIS_ALIGNMENT {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.center[index] - first.center[index]);
    let axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
    let scale = first
        .major_radius
        .max(first.minor_radius)
        .max(second.major_radius)
        .max(second.minor_radius);
    if Vector3::from(transverse).norm() > EPS_CENTER_ALIGNMENT * scale {
        return Vec::new();
    }
    let intersections = meridian_circle_intersections(
        [first.major_radius, 0.0],
        first.minor_radius,
        [second.major_radius, axial],
        second.minor_radius,
    );
    let tag = match intersections.len() {
        1 => "coaxial_tori_tangent_circle",
        2 => "coaxial_tori_secant_circle",
        _ => return Vec::new(),
    };
    intersections
        .into_iter()
        .filter_map(|[radius, center_axial]| {
            let radius = radius.abs();
            if radius <= EPS_POSITIVE_RADIUS * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(first_axis),
                        Vector3::from(reference),
                        radius,
                    )
                    .ok()?,
                )),
                tag,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests;
