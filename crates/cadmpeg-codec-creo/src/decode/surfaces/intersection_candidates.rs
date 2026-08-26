// SPDX-License-Identifier: Apache-2.0
//! Parallel, coaxial, and meridian intersection candidate families.

use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use super::super::analytic::{circular_cone, cross, dot, CarrierEquation};
use super::super::sketch::normalized;

const EPS_AXIS_ORTHO: f64 = 1.0e-10;
const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;
const EPS_DISTANCE_NONZERO: f64 = 1.0e-12;
const EPS_TANGENCY_RESIDUAL: f64 = 1.0e-9;
const EPS_HEIGHT_RESIDUAL: f64 = 1.0e-12;
const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_SLOPE_NONZERO: f64 = 1.0e-12;
const EPS_METRIC_AGREEMENT: f64 = 1.0e-10;
const EPS_DETERMINANT: f64 = 1.0e-12;
const EPS_DISCRIMINANT: f64 = 1.0e-9;
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
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    if dot(normal, axis).abs() > EPS_AXIS_ORTHO || cylinder.radius <= 0.0 {
        return Vec::new();
    }
    let signed_distance = dot(
        normal,
        std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
    );
    let scale = cylinder.radius.max(1.0);
    let offset_squared = cylinder
        .radius
        .mul_add(cylinder.radius, -(signed_distance * signed_distance));
    if offset_squared <= 1e-18 * scale * scale {
        return Vec::new();
    }
    let closest: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - signed_distance * normal[index]);
    let Some(transverse) = normalized(cross(axis, normal)) else {
        return Vec::new();
    };
    let offset = offset_squared.sqrt();
    [-1.0, 1.0]
        .into_iter()
        .map(|sense| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| closest[index] + sense * offset * transverse[index]);
            (
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(axis[0], axis[1], axis[2]),
                },
                "plane_cylinder_secant_generator",
            )
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
    let (Some(first_axis), Some(second_axis)) = (normalized(first.axis), normalized(second.axis))
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
    let distance = dot(transverse, transverse).sqrt();
    let scale = first.radius.max(second.radius).max(distance).max(1.0);
    if distance <= EPS_DISTANCE_NONZERO * scale
        || distance >= first.radius + second.radius - EPS_GEOMETRY_AGREEMENT * scale
        || distance <= (first.radius - second.radius).abs() + EPS_GEOMETRY_AGREEMENT * scale
    {
        return Vec::new();
    }
    let center_direction = transverse.map(|value| value / distance);
    let along = (first.radius * first.radius - second.radius * second.radius + distance * distance)
        / (2.0 * distance);
    let height_squared = first.radius.mul_add(first.radius, -(along * along));
    if height_squared <= EPS_HEIGHT_RESIDUAL * scale * scale {
        return Vec::new();
    }
    let Some(perpendicular) = normalized(cross(first_axis, center_direction)) else {
        return Vec::new();
    };
    let base: [f64; 3] =
        std::array::from_fn(|index| first.origin[index] + along * center_direction[index]);
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|offset| {
            let origin: [f64; 3] =
                std::array::from_fn(|index| base[index] + offset * perpendicular[index]);
            (
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                },
                "parallel_cylinder_secant_generator",
            )
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
    let Some(axis) = normalized(cylinder.axis) else {
        return Vec::new();
    };
    let relative: [f64; 3] =
        std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
    let axial = dot(relative, axis);
    let transverse: [f64; 3] = std::array::from_fn(|index| relative[index] - axial * axis[index]);
    let scale = sphere.radius.max(cylinder.radius).max(1.0);
    if sphere.radius <= 0.0
        || cylinder.radius <= 0.0
        || dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale
    {
        return Vec::new();
    }
    let offset_squared = sphere
        .radius
        .mul_add(sphere.radius, -(cylinder.radius * cylinder.radius));
    if offset_squared <= EPS_TANGENCY_RESIDUAL * scale * scale {
        return Vec::new();
    }
    let Some(reference) = normalized(cylinder.ref_direction) else {
        return Vec::new();
    };
    let offset = offset_squared.sqrt();
    [-offset, offset]
        .into_iter()
        .map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + offset * axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_sphere_secant_circle",
            )
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
        normalized(cone.axis),
        normalized(cylinder.axis),
        normalized(cone.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, cylinder_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
        return Vec::new();
    }
    let relative: [f64; 3] =
        std::array::from_fn(|index| cylinder.origin[index] - cone.origin[index]);
    let axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
    let scale = cone.radius.max(cylinder.radius).max(1.0);
    let slope = cone.half_angle.tan();
    if dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale
        || cylinder.radius <= EPS_RADIUS_NONZERO * scale
        || cone.radius < 0.0
        || slope.abs() <= EPS_SLOPE_NONZERO
        || !slope.is_finite()
    {
        return Vec::new();
    }
    [cylinder.radius, -cylinder.radius]
        .into_iter()
        .map(|signed_radius| {
            let parameter = (signed_radius - cone.radius) / slope;
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * cone_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cone_cylinder_secant_circle",
            )
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
    if first.ratio <= 0.0
        || second.ratio <= 0.0
        || !first.ratio.is_finite()
        || !second.ratio.is_finite()
    {
        return Vec::new();
    }
    let (Some(first_axis), Some(second_axis), Some(reference), Some(second_reference)) = (
        normalized(first.axis),
        normalized(second.axis),
        normalized(first.ref_direction),
        normalized(second.ref_direction),
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
        let y = dot(direction, second_y) / second.ratio;
        x.mul_add(x, y * y)
    };
    let metric_xx = second_metric(reference);
    let metric_yy = second_metric(first_y);
    let metric_xy = dot(reference, second_reference).mul_add(
        dot(first_y, second_reference),
        dot(reference, second_y) * dot(first_y, second_y) / (second.ratio * second.ratio),
    );
    let metric_scale_squared = metric_xx;
    let metric_coefficient_scale = metric_xx.abs().max(metric_yy.abs()).max(1.0);
    if metric_scale_squared <= 0.0
        || !metric_scale_squared.is_finite()
        || !metric_yy.is_finite()
        || !metric_xy.is_finite()
        || metric_xy.abs() > EPS_METRIC_AGREEMENT * metric_coefficient_scale
        || (metric_yy - metric_scale_squared / (first.ratio * first.ratio)).abs()
            > EPS_METRIC_AGREEMENT * metric_coefficient_scale
    {
        return Vec::new();
    }
    let metric_scale = metric_scale_squared.sqrt();
    let relative: [f64; 3] =
        std::array::from_fn(|index| second.origin[index] - first.origin[index]);
    let second_origin_axial = dot(relative, first_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - second_origin_axial * first_axis[index]);
    let scale = first.radius.max(second.radius).max(1.0);
    let first_slope = first.half_angle.tan();
    let second_slope = axis_alignment * second.half_angle.tan();
    let second_intercept = second.radius - second_slope * second_origin_axial;
    if dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale
        || first.radius < 0.0
        || second.radius < 0.0
        || first_slope.abs() <= EPS_SLOPE_NONZERO
        || second_slope.abs() <= EPS_SLOPE_NONZERO
        || !first_slope.is_finite()
        || !second_slope.is_finite()
    {
        return Vec::new();
    }

    let mut parameters = Vec::<f64>::new();
    let scaled_first_slope = metric_scale * first_slope;
    let scaled_first_radius = metric_scale * first.radius;
    let slope_scale = scaled_first_slope.abs().max(second_slope.abs()).max(1.0);
    let intercept_scale = first
        .radius
        .max(scaled_first_radius.abs())
        .max(second_intercept.abs())
        .max(second.radius)
        .max(1.0);
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
        let radius = (first.radius + parameter * first_slope).abs();
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
        .map(|parameter| {
            let radius = (first.radius + parameter * first_slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| first.origin[index] + parameter * first_axis[index]);
            let (geometry, tag) = if circular_cone(first) {
                (
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius,
                    },
                    "coaxial_cones_circle",
                )
            } else {
                (
                    CurveGeometry::Ellipse {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                        major_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        major_radius: radius,
                        minor_radius: radius * first.ratio,
                    },
                    "coaxial_cones_ellipse",
                )
            };
            (geometry, tag)
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
    let Some(normal) = normalized(plane.normal) else {
        return Vec::new();
    };
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    let Some(x_axis) = normalized(cone.ref_direction) else {
        return Vec::new();
    };
    let slope = cone.half_angle.tan();
    if slope <= EPS_SLOPE_NONZERO
        || !slope.is_finite()
        || cone.radius < 0.0
        || cone.ratio <= 0.0
        || !cone.ratio.is_finite()
        || dot(axis, x_axis).abs() > EPS_AXIS_ORTHO
    {
        return Vec::new();
    }
    let apex: [f64; 3] =
        std::array::from_fn(|index| cone.origin[index] - cone.radius / slope * axis[index]);
    let plane_distance = dot(
        normal,
        std::array::from_fn(|index| apex[index] - plane.origin[index]),
    );
    let scale = cone.radius.max(1.0);
    if plane_distance.abs() > EPS_GEOMETRY_AGREEMENT * scale {
        return Vec::new();
    }
    let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
        normal[0], normal[1], normal[2],
    ));
    let plane_u = [reference.x, reference.y, reference.z];
    let plane_v = cross(normal, plane_u);
    let y_axis = cross(axis, x_axis);
    let cone_coordinates = |direction: [f64; 3]| {
        [
            dot(direction, x_axis),
            dot(direction, y_axis) / cone.ratio,
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
                normalized(std::array::from_fn(|index| {
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
        .map(|direction| {
            (
                CurveGeometry::Line {
                    origin: Point3::new(apex[0], apex[1], apex[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                tag,
            )
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
    let Some(axis) = normalized(cone.axis) else {
        return Vec::new();
    };
    let relative: [f64; 3] = std::array::from_fn(|index| sphere.center[index] - cone.origin[index]);
    let sphere_axial = dot(relative, axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - sphere_axial * axis[index]);
    let scale = cone.radius.max(sphere.radius).max(1.0);
    if dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale {
        return Vec::new();
    }
    let slope = cone.half_angle.tan();
    if slope.abs() <= EPS_SLOPE_NONZERO || !slope.is_finite() || cone.radius < 0.0 {
        return Vec::new();
    }
    let quadratic = 1.0 + slope * slope;
    let linear = 2.0 * (cone.radius * slope - sphere_axial);
    let constant =
        cone.radius * cone.radius + sphere_axial * sphere_axial - sphere.radius * sphere.radius;
    let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
    let discriminant_scale = linear
        .abs()
        .max((4.0 * quadratic * constant).abs().sqrt())
        .max(1.0);
    if discriminant <= EPS_DISCRIMINANT * discriminant_scale * discriminant_scale {
        return Vec::new();
    }
    let Some(reference) = normalized(cone.ref_direction) else {
        return Vec::new();
    };
    let root_delta = discriminant.sqrt();
    [-root_delta, root_delta]
        .into_iter()
        .filter_map(|delta| {
            let parameter = (-linear + delta) / (2.0 * quadratic);
            let radius = (cone.radius + parameter * slope).abs();
            if radius <= EPS_RADIUS_NONZERO * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * axis[index]);
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_sphere_secant_circle",
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
        normalized(cone.axis),
        normalized(torus.axis),
        normalized(cone.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cone_axis, torus_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| torus.center[index] - cone.origin[index]);
    let torus_axial = dot(relative, cone_axis);
    let transverse: [f64; 3] =
        std::array::from_fn(|index| relative[index] - torus_axial * cone_axis[index]);
    let scale = cone
        .radius
        .max(torus.major_radius)
        .max(torus.minor_radius)
        .max(1.0);
    let slope = cone.half_angle.tan();
    if dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale
        || cone.radius < 0.0
        || torus.major_radius <= EPS_RADIUS_NONZERO * scale
        || torus.minor_radius <= EPS_RADIUS_NONZERO * scale
        || slope.abs() <= EPS_SLOPE_NONZERO
        || !slope.is_finite()
    {
        return Vec::new();
    }

    let quadratic = 1.0 + slope * slope;
    let mut parameters = Vec::<f64>::new();
    for radial_sense in [-1.0, 1.0] {
        let radial_offset = radial_sense * cone.radius - torus.major_radius;
        let radial_slope = radial_sense * slope;
        let linear = 2.0 * (radial_offset * radial_slope - torus_axial);
        let constant = radial_offset * radial_offset + torus_axial * torus_axial
            - torus.minor_radius * torus.minor_radius;
        let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
        let discriminant_scale = linear
            .abs()
            .max((4.0 * quadratic * constant).abs().sqrt())
            .max(1.0);
        let tolerance = EPS_DISCRIMINANT * discriminant_scale * discriminant_scale;
        let deltas = if discriminant < -tolerance {
            continue;
        } else if discriminant.abs() <= tolerance {
            vec![0.0]
        } else {
            let root = discriminant.sqrt();
            vec![-root, root]
        };
        for delta in deltas {
            let parameter = (-linear + delta) / (2.0 * quadratic);
            let radius = radial_sense * (cone.radius + parameter * slope);
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
        .map(|parameter| {
            let radius = (cone.radius + parameter * slope).abs();
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + parameter * cone_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_torus_circle",
            )
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
        normalized(cylinder.axis),
        normalized(torus.axis),
        normalized(cylinder.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
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
        .max(cylinder.radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale {
        return Vec::new();
    }
    let radial_delta = cylinder.radius - torus.major_radius;
    let height_squared = torus
        .minor_radius
        .mul_add(torus.minor_radius, -(radial_delta * radial_delta));
    if height_squared <= EPS_TANGENCY_RESIDUAL * scale * scale
        || cylinder.radius <= EPS_RADIUS_NONZERO * scale
    {
        return Vec::new();
    }
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|offset| {
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + offset * torus_axis[index]);
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(torus_axis[0], torus_axis[1], torus_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_torus_secant_circle",
            )
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
        normalized(plane.normal),
        normalized(torus.axis),
        normalized(torus.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(normal, axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
        return Vec::new();
    }
    let relative: [f64; 3] = std::array::from_fn(|index| plane.origin[index] - torus.center[index]);
    let axial = dot(relative, axis);
    let scale = torus.major_radius.max(torus.minor_radius).max(1.0);
    let radial_offset_squared = torus
        .minor_radius
        .mul_add(torus.minor_radius, -(axial * axial));
    if radial_offset_squared <= EPS_TANGENCY_RESIDUAL * scale * scale {
        return Vec::new();
    }
    let center: [f64; 3] = std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
    let radial_offset = radial_offset_squared.sqrt();
    [
        torus.major_radius - radial_offset,
        torus.major_radius + radial_offset,
    ]
    .into_iter()
    .filter(|radius| *radius > EPS_RADIUS_NONZERO * scale)
    .map(|radius| {
        (
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "plane_torus_secant_circle",
        )
    })
    .collect()
}

pub(in super::super) fn meridian_circle_intersections(
    first_center: [f64; 2],
    first_radius: f64,
    second_center: [f64; 2],
    second_radius: f64,
    scale: f64,
) -> Vec<[f64; 2]> {
    let delta = [
        second_center[0] - first_center[0],
        second_center[1] - first_center[1],
    ];
    let distance = delta[0].hypot(delta[1]);
    if distance <= EPS_DISTANCE_NONZERO * scale
        || distance >= first_radius + second_radius - EPS_GEOMETRY_AGREEMENT * scale
        || distance <= (first_radius - second_radius).abs() + EPS_GEOMETRY_AGREEMENT * scale
    {
        return Vec::new();
    }
    let along = (distance * distance + first_radius * first_radius - second_radius * second_radius)
        / (2.0 * distance);
    let height_squared = first_radius.mul_add(first_radius, -(along * along));
    if height_squared <= EPS_HEIGHT_RESIDUAL * scale * scale {
        return Vec::new();
    }
    let unit = [delta[0] / distance, delta[1] / distance];
    let height = height_squared.sqrt();
    [-height, height]
        .into_iter()
        .map(|sense| {
            [
                first_center[0] + along * unit[0] - sense * unit[1],
                first_center[1] + along * unit[1] + sense * unit[0],
            ]
        })
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
    let (Some(normal), Some(axis)) = (normalized(plane.normal), normalized(torus.axis)) else {
        return Vec::new();
    };
    let scale = torus.major_radius.max(torus.minor_radius).max(1.0);
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
    let Some(radial) = normalized(cross(normal, axis)) else {
        return Vec::new();
    };
    [-1.0, 1.0]
        .into_iter()
        .map(|sense| {
            let center: [f64; 3] = std::array::from_fn(|index| {
                torus.center[index] + sense * torus.major_radius * radial[index]
            });
            (
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(axis[0], axis[1], axis[2]),
                    radius: torus.minor_radius,
                },
                "axis_containing_plane_torus_meridian_circle",
            )
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
    let (Some(axis), Some(reference)) = (normalized(torus.axis), normalized(torus.ref_direction))
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
        .max(sphere.radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale {
        return Vec::new();
    }
    meridian_circle_intersections(
        [0.0, 0.0],
        sphere.radius,
        [torus.major_radius, axial],
        torus.minor_radius,
        scale,
    )
    .into_iter()
    .filter_map(|[radius, center_axial]| {
        let radius = radius.abs();
        if radius <= EPS_RADIUS_NONZERO * scale {
            return None;
        }
        let center: [f64; 3] =
            std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
        Some((
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(axis[0], axis[1], axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "coaxial_sphere_torus_secant_circle",
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
        normalized(first.axis),
        normalized(second.axis),
        normalized(first.ref_direction),
    ) else {
        return Vec::new();
    };
    if (dot(first_axis, second_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
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
        .max(second.minor_radius)
        .max(1.0);
    if dot(transverse, transverse).sqrt() > EPS_GEOMETRY_AGREEMENT * scale {
        return Vec::new();
    }
    meridian_circle_intersections(
        [first.major_radius, 0.0],
        first.minor_radius,
        [second.major_radius, axial],
        second.minor_radius,
        scale,
    )
    .into_iter()
    .filter_map(|[radius, center_axial]| {
        let radius = radius.abs();
        if radius <= EPS_RADIUS_NONZERO * scale {
            return None;
        }
        let center: [f64; 3] =
            std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
        Some((
            CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                radius,
            },
            "coaxial_tori_secant_circle",
        ))
    })
    .collect()
}
