// SPDX-License-Identifier: Apache-2.0
//! Carrier pairwise intersection curves.

use cadmpeg_ir::geometry::CurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use super::super::analytic::{circular_cone, cross, dot, plane_cone_conic, CarrierEquation};
use super::super::sketch::normalized;

use super::intersection_candidates::apex_plane_cone_generator_candidates;

const EPS_AXIS_ORTHO: f64 = 1e-10;
const EPS_CARRIER_AGREEMENT: f64 = 1e-9;
const EPS_CONE_SLOPE_NONZERO: f64 = 1e-12;
const EPS_RADIUS_NONZERO: f64 = 1e-12;
const EPS_DISTANCE_NONZERO: f64 = 1e-12;
const EPS_TRANSVERSE_RESIDUAL: f64 = 1e-9;
const EPS_RADIUS_AGREEMENT: f64 = 1e-9;
const EPS_DISCRIMINANT_RESIDUAL: f64 = 1e-9;

pub(in super::super) fn carrier_intersection_curve(
    first: CarrierEquation,
    second: CarrierEquation,
) -> Option<(CurveGeometry, &'static str)> {
    match (first, second) {
        (CarrierEquation::Plane(first), CarrierEquation::Plane(second)) => {
            let direction = cross(first.normal, second.normal);
            let denominator = dot(direction, direction);
            if denominator <= 1e-18 {
                return None;
            }
            let first_distance = dot(first.normal, first.origin);
            let second_distance = dot(second.normal, second.origin);
            let weighted = [0, 1, 2].map(|axis| {
                first_distance * second.normal[axis] - second_distance * first.normal[axis]
            });
            let point_numerator = cross(weighted, direction);
            let origin = point_numerator.map(|value| value / denominator);
            let direction = normalized(direction)?;
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(direction[0], direction[1], direction[2]),
                },
                "plane_intersection_line",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
        | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cylinder.axis)?;
            let cosine = dot(normal, axis);
            if cosine.abs() <= EPS_AXIS_ORTHO {
                let signed_distance = dot(
                    normal,
                    std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
                );
                let scale = cylinder.radius.max(1.0);
                if (signed_distance.abs() - cylinder.radius).abs() > EPS_CARRIER_AGREEMENT * scale {
                    return None;
                }
                let origin: [f64; 3] = std::array::from_fn(|index| {
                    cylinder.origin[index] - signed_distance * normal[index]
                });
                return Some((
                    CurveGeometry::Line {
                        origin: Point3::new(origin[0], origin[1], origin[2]),
                        direction: Vector3::new(axis[0], axis[1], axis[2]),
                    },
                    "plane_cylinder_tangent_line",
                ));
            }
            let axis_parameter = dot(
                normal,
                std::array::from_fn(|index| plane.origin[index] - cylinder.origin[index]),
            ) / cosine;
            let center: [f64; 3] =
                std::array::from_fn(|index| cylinder.origin[index] + axis_parameter * axis[index]);
            if (cosine.abs() - 1.0).abs() <= EPS_AXIS_ORTHO {
                let reference = normalized(cylinder.ref_direction)?;
                return Some((
                    CurveGeometry::Circle {
                        center: Point3::new(center[0], center[1], center[2]),
                        axis: Vector3::new(normal[0], normal[1], normal[2]),
                        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                        radius: cylinder.radius,
                    },
                    "plane_cylinder_circle",
                ));
            }
            let projected_axis = normalized(std::array::from_fn(|index| {
                axis[index] - cosine * normal[index]
            }))?;
            Some((
                CurveGeometry::Ellipse {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    major_direction: Vector3::new(
                        projected_axis[0],
                        projected_axis[1],
                        projected_axis[2],
                    ),
                    major_radius: cylinder.radius / cosine.abs(),
                    minor_radius: cylinder.radius,
                },
                "plane_cylinder_ellipse",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let signed_distance = dot(
                normal,
                std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
            );
            let radius_squared = sphere
                .radius
                .mul_add(sphere.radius, -(signed_distance * signed_distance));
            let scale = sphere.radius.max(1.0);
            if radius_squared <= 1e-18 * scale * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - signed_distance * normal[index]);
            let reference = normalized(std::array::from_fn(|index| {
                sphere.ref_direction[index] - dot(sphere.ref_direction, normal) * normal[index]
            }))
            .unwrap_or_else(|| {
                let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                    normal[0], normal[1], normal[2],
                ));
                [reference.x, reference.y, reference.z]
            });
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: radius_squared.sqrt(),
                },
                "plane_sphere_circle",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
        | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(cone.axis)?;
            let alignment = dot(normal, axis);
            let slope = cone.half_angle.tan();
            if circular_cone(cone) && slope.abs() > EPS_CONE_SLOPE_NONZERO {
                let apex: [f64; 3] = std::array::from_fn(|index| {
                    cone.origin[index] - (cone.radius / slope) * axis[index]
                });
                let plane_distance = dot(
                    normal,
                    std::array::from_fn(|index| apex[index] - plane.origin[index]),
                );
                let scale = cone.radius.max(1.0);
                if plane_distance.abs() <= EPS_CARRIER_AGREEMENT * scale
                    && (alignment.abs() - cone.half_angle.sin()).abs() <= EPS_AXIS_ORTHO
                {
                    let direction = normalized(std::array::from_fn(|index| {
                        axis[index] - alignment * normal[index]
                    }))?;
                    return Some((
                        CurveGeometry::Line {
                            origin: Point3::new(apex[0], apex[1], apex[2]),
                            direction: Vector3::new(direction[0], direction[1], direction[2]),
                        },
                        "plane_cone_tangent_line",
                    ));
                }
            }
            let apex_generators = apex_plane_cone_generator_candidates(
                CarrierEquation::Plane(plane),
                CarrierEquation::Cone(cone),
            );
            if apex_generators.len() == 1 {
                return apex_generators.into_iter().next();
            }
            if (alignment.abs() - 1.0).abs() <= EPS_AXIS_ORTHO {
                let axial = dot(
                    axis,
                    std::array::from_fn(|index| plane.origin[index] - cone.origin[index]),
                );
                let radius = (cone.radius + axial * cone.half_angle.tan()).abs();
                if radius <= EPS_RADIUS_NONZERO {
                    return None;
                }
                let center: [f64; 3] =
                    std::array::from_fn(|index| cone.origin[index] + axial * axis[index]);
                let reference = normalized(cone.ref_direction)?;
                let (geometry, tag) = if circular_cone(cone) {
                    (
                        CurveGeometry::Circle {
                            center: Point3::new(center[0], center[1], center[2]),
                            axis: Vector3::new(normal[0], normal[1], normal[2]),
                            ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                            radius,
                        },
                        "plane_cone_circle",
                    )
                } else {
                    (
                        CurveGeometry::Ellipse {
                            center: Point3::new(center[0], center[1], center[2]),
                            axis: Vector3::new(normal[0], normal[1], normal[2]),
                            major_direction: Vector3::new(reference[0], reference[1], reference[2]),
                            major_radius: radius,
                            minor_radius: radius * cone.ratio,
                        },
                        "plane_cone_parallel_ellipse",
                    )
                };
                return Some((geometry, tag));
            }
            plane_cone_conic(plane, cone)
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane)) => {
            let normal = normalized(plane.normal)?;
            let axis = normalized(torus.axis)?;
            if (dot(normal, axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
                return None;
            }
            let axial = dot(
                axis,
                std::array::from_fn(|index| plane.origin[index] - torus.center[index]),
            );
            let scale = torus.minor_radius.max(torus.major_radius).max(1.0);
            if (axial.abs() - torus.minor_radius).abs() > EPS_CARRIER_AGREEMENT * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(normal[0], normal[1], normal[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: torus.major_radius,
                },
                "plane_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            let alignment = dot(first_axis, second_axis);
            if (alignment.abs() - 1.0).abs() > EPS_AXIS_ORTHO {
                return None;
            }
            let relative = std::array::from_fn(|index| second.origin[index] - first.origin[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let distance = dot(transverse, transverse).sqrt();
            if distance <= EPS_DISTANCE_NONZERO {
                return None;
            }
            let external = first.radius + second.radius;
            let internal = (first.radius - second.radius).abs();
            let scale = external.max(distance).max(1.0);
            let first_fraction = if (distance - external).abs() <= EPS_CARRIER_AGREEMENT * scale {
                first.radius / distance
            } else if (distance - internal).abs() <= EPS_CARRIER_AGREEMENT * scale {
                let signed = if first.radius >= second.radius {
                    first.radius
                } else {
                    -first.radius
                };
                signed / distance
            } else {
                return None;
            };
            let origin: [f64; 3] = std::array::from_fn(|index| {
                first.origin[index] + first_fraction * transverse[index]
            });
            Some((
                CurveGeometry::Line {
                    origin: Point3::new(origin[0], origin[1], origin[2]),
                    direction: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                },
                "parallel_cylinder_tangent_line",
            ))
        }
        (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
            let center_delta: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let distance = dot(center_delta, center_delta).sqrt();
            if distance <= EPS_DISTANCE_NONZERO
                || distance >= first.radius + second.radius
                || distance <= (first.radius - second.radius).abs()
            {
                return None;
            }
            let axis = center_delta.map(|value| value / distance);
            let axial = (distance * distance + first.radius * first.radius
                - second.radius * second.radius)
                / (2.0 * distance);
            let radius_squared = first.radius.mul_add(first.radius, -(axial * axial));
            if radius_squared <= 1e-18 {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + axial * axis[index]);
            let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::new(
                axis[0], axis[1], axis[2],
            ));
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: reference,
                    radius: radius_squared.sqrt(),
                },
                "sphere_intersection_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder)) => {
            let axis = normalized(cylinder.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = sphere.radius.max(cylinder.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > EPS_TRANSVERSE_RESIDUAL * scale
                || (sphere.radius - cylinder.radius).abs() > EPS_RADIUS_AGREEMENT * scale
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(sphere.center[0], sphere.center[1], sphere.center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_sphere_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder)) => {
            let cylinder_axis = normalized(cylinder.axis)?;
            let torus_axis = normalized(torus.axis)?;
            if (dot(cylinder_axis, torus_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
                return None;
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
            if dot(transverse, transverse).sqrt() > EPS_TRANSVERSE_RESIDUAL * scale {
                return None;
            }
            let outer_radius = torus.major_radius + torus.minor_radius;
            let inner_radius = (torus.major_radius - torus.minor_radius).abs();
            if (cylinder.radius - outer_radius).abs() > EPS_RADIUS_AGREEMENT * scale
                && (inner_radius <= EPS_RADIUS_NONZERO
                    || (cylinder.radius - inner_radius).abs() > EPS_RADIUS_AGREEMENT * scale)
            {
                return None;
            }
            let reference = normalized(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(torus.center[0], torus.center[1], torus.center[2]),
                    axis: Vector3::new(cylinder_axis[0], cylinder_axis[1], cylinder_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius: cylinder.radius,
                },
                "coaxial_cylinder_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone)) => {
            if !circular_cone(cone) {
                return None;
            }
            let cone_axis = normalized(cone.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cone.origin[index]);
            let axial = dot(relative, cone_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
            let scale = cone.radius.max(sphere.radius).max(1.0);
            if dot(transverse, transverse).sqrt() > EPS_TRANSVERSE_RESIDUAL * scale {
                return None;
            }
            let slope = cone.half_angle.tan();
            if slope.abs() <= EPS_CONE_SLOPE_NONZERO {
                return None;
            }
            let quadratic = 1.0 + slope * slope;
            let linear = 2.0 * (cone.radius * slope - axial);
            let constant =
                cone.radius * cone.radius + axial * axial - sphere.radius * sphere.radius;
            let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
            let discriminant_scale = linear
                .abs()
                .max((4.0 * quadratic * constant).abs().sqrt())
                .max(1.0);
            if discriminant.abs()
                > EPS_DISCRIMINANT_RESIDUAL * discriminant_scale * discriminant_scale
            {
                return None;
            }
            let cone_parameter = -linear / (2.0 * quadratic);
            let radius = (cone.radius + cone_parameter * slope).abs();
            if radius <= EPS_RADIUS_NONZERO * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| cone.origin[index] + cone_parameter * cone_axis[index]);
            let reference = normalized(cone.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(cone_axis[0], cone_axis[1], cone_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_cone_sphere_tangent_circle",
            ))
        }
        (CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere)) => {
            let axis = normalized(torus.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(sphere.radius)
                .max(1.0);
            if dot(transverse, transverse).sqrt() > EPS_TRANSVERSE_RESIDUAL * scale {
                return None;
            }
            let meridian_distance = torus.major_radius.hypot(axial);
            if meridian_distance <= EPS_DISTANCE_NONZERO {
                return None;
            }
            let external = sphere.radius + torus.minor_radius;
            let internal = (sphere.radius - torus.minor_radius).abs();
            if (meridian_distance - external).abs() > EPS_CARRIER_AGREEMENT * scale
                && (meridian_distance - internal).abs() > EPS_CARRIER_AGREEMENT * scale
            {
                return None;
            }
            let sphere_parameter = (meridian_distance * meridian_distance
                + sphere.radius * sphere.radius
                - torus.minor_radius * torus.minor_radius)
                / (2.0 * meridian_distance);
            let radius = (sphere_parameter * torus.major_radius / meridian_distance).abs();
            if radius <= EPS_RADIUS_NONZERO * scale {
                return None;
            }
            let center_axial = sphere_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] + center_axial * axis[index]);
            let reference = normalized(torus.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(axis[0], axis[1], axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_sphere_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) => {
            let first_axis = normalized(first.axis)?;
            let second_axis = normalized(second.axis)?;
            if (dot(first_axis, second_axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
                return None;
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
            if dot(transverse, transverse).sqrt() > EPS_TRANSVERSE_RESIDUAL * scale {
                return None;
            }
            let radial_delta = second.major_radius - first.major_radius;
            let meridian_distance = radial_delta.hypot(axial);
            if meridian_distance <= EPS_DISTANCE_NONZERO {
                return None;
            }
            let external = first.minor_radius + second.minor_radius;
            let internal = (first.minor_radius - second.minor_radius).abs();
            if (meridian_distance - external).abs() > EPS_CARRIER_AGREEMENT * scale
                && (meridian_distance - internal).abs() > EPS_CARRIER_AGREEMENT * scale
            {
                return None;
            }
            let first_parameter = (meridian_distance * meridian_distance
                + first.minor_radius * first.minor_radius
                - second.minor_radius * second.minor_radius)
                / (2.0 * meridian_distance);
            let radius =
                (first.major_radius + first_parameter * radial_delta / meridian_distance).abs();
            if radius <= EPS_RADIUS_NONZERO * scale {
                return None;
            }
            let center_axial = first_parameter * axial / meridian_distance;
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + center_axial * first_axis[index]);
            let reference = normalized(first.ref_direction)?;
            Some((
                CurveGeometry::Circle {
                    center: Point3::new(center[0], center[1], center[2]),
                    axis: Vector3::new(first_axis[0], first_axis[1], first_axis[2]),
                    ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
                    radius,
                },
                "coaxial_tori_tangent_circle",
            ))
        }
        (
            CarrierEquation::Cone(_),
            CarrierEquation::Cylinder(_) | CarrierEquation::Cone(_) | CarrierEquation::Torus(_),
        )
        | (CarrierEquation::Cylinder(_) | CarrierEquation::Torus(_), CarrierEquation::Cone(_)) => {
            None
        }
    }
}
