// SPDX-License-Identifier: Apache-2.0
//! Carrier pairwise intersection curves.

use crate::vecmath::normalize;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::decode::analytic::equations::{circular_cone, plane_cone_conic, CarrierEquation};
use crate::vecmath::{cross, dot};

use super::intersection_candidates::apex_plane_cone_generator_candidates;

const EPS_AXIS_ORTHO: f64 = 1.0e-10;
const EPS_CARRIER_AGREEMENT: f64 = 1.0e-9;
const EPS_CONE_SLOPE_NONZERO: f64 = 1.0e-12;
const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_DISTANCE_NONZERO: f64 = 1.0e-12;
const EPS_TRANSVERSE_RESIDUAL: f64 = 1.0e-9;
const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;
const EPS_DISCRIMINANT_RESIDUAL: f64 = 1.0e-9;

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
            let direction = normalize(direction)?;
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::from(origin),
                        Vector3::from(direction),
                    )
                    .ok()?,
                )),
                "plane_intersection_line",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cylinder(cylinder))
        | (CarrierEquation::Cylinder(cylinder), CarrierEquation::Plane(plane)) => {
            let normal = normalize(plane.normal)?;
            let axis = normalize(cylinder.axis)?;
            let cosine = dot(normal, axis);
            if cosine.abs() <= EPS_AXIS_ORTHO {
                let signed_distance = dot(
                    normal,
                    std::array::from_fn(|index| cylinder.origin[index] - plane.origin[index]),
                );
                let scale = cylinder.radius;
                if (signed_distance.abs() - cylinder.radius).abs() > EPS_CARRIER_AGREEMENT * scale {
                    return None;
                }
                let origin: [f64; 3] = std::array::from_fn(|index| {
                    cylinder.origin[index] - signed_distance * normal[index]
                });
                return Some((
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                            Point3::from(origin),
                            Vector3::from(axis),
                        )
                        .ok()?,
                    )),
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
                let reference = normalize(cylinder.ref_direction)?;
                return Some((
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                            Point3::from(center),
                            Vector3::from(normal),
                            Vector3::from(reference),
                            cylinder.radius,
                        )
                        .ok()?,
                    )),
                    "plane_cylinder_circle",
                ));
            }
            let projected_axis = normalize(std::array::from_fn(|index| {
                axis[index] - cosine * normal[index]
            }))?;
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                    cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                        Point3::from(center),
                        Vector3::from(normal),
                        Vector3::from(projected_axis),
                        cylinder.radius / cosine.abs(),
                        cylinder.radius,
                    )
                    .ok()?,
                )),
                "plane_cylinder_ellipse",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Plane(plane)) => {
            let normal = normalize(plane.normal)?;
            let signed_distance = dot(
                normal,
                std::array::from_fn(|index| sphere.center[index] - plane.origin[index]),
            );
            let relative_distance = (signed_distance / sphere.radius).abs();
            if !relative_distance.is_finite() || relative_distance >= 1.0 {
                return None;
            }
            let radius =
                sphere.radius * ((1.0 - relative_distance) * (1.0 + relative_distance)).sqrt();
            let center: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - signed_distance * normal[index]);
            let reference = normalize(std::array::from_fn(|index| {
                sphere.ref_direction[index] - dot(sphere.ref_direction, normal) * normal[index]
            }))
            .unwrap_or_else(|| {
                let reference =
                    cadmpeg_ir::geometry::derive_reference_direction(Vector3::from(normal));
                [reference.x, reference.y, reference.z]
            });
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(normal),
                        Vector3::from(reference),
                        radius,
                    )
                    .ok()?,
                )),
                "plane_sphere_circle",
            ))
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Cone(cone))
        | (CarrierEquation::Cone(cone), CarrierEquation::Plane(plane)) => {
            let normal = normalize(plane.normal)?;
            let axis = normalize(cone.axis())?;
            let alignment = dot(normal, axis);
            let slope = cone.half_angle().tan();
            if circular_cone(cone) && slope.abs() > EPS_CONE_SLOPE_NONZERO {
                let apex: [f64; 3] = std::array::from_fn(|index| {
                    cone.origin()[index] - (cone.radius() / slope) * axis[index]
                });
                let plane_distance = dot(
                    normal,
                    std::array::from_fn(|index| apex[index] - plane.origin[index]),
                );
                let scale = cone.radius().abs();
                if plane_distance.abs() <= EPS_CARRIER_AGREEMENT * scale
                    && (alignment.abs() - cone.half_angle().sin()).abs() <= EPS_AXIS_ORTHO
                {
                    let direction = normalize(std::array::from_fn(|index| {
                        axis[index] - alignment * normal[index]
                    }))?;
                    return Some((
                        CurveGeometry::Solved(SolvedCurveGeometry::Line(
                            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                                Point3::from(apex),
                                Vector3::from(direction),
                            )
                            .ok()?,
                        )),
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
                    std::array::from_fn(|index| plane.origin[index] - cone.origin()[index]),
                );
                let radius = (cone.radius() + axial * cone.half_angle().tan()).abs();
                if radius <= EPS_RADIUS_NONZERO {
                    return None;
                }
                let center: [f64; 3] =
                    std::array::from_fn(|index| cone.origin()[index] + axial * axis[index]);
                let reference = normalize(cone.ref_direction())?;
                let (geometry, tag) = if circular_cone(cone) {
                    (
                        CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                                Point3::from(center),
                                Vector3::from(normal),
                                Vector3::from(reference),
                                radius,
                            )
                            .ok()?,
                        )),
                        "plane_cone_circle",
                    )
                } else {
                    (
                        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
                            cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
                                Point3::from(center),
                                Vector3::from(normal),
                                Vector3::from(reference),
                                radius,
                                radius * cone.ratio(),
                            )
                            .ok()?,
                        )),
                        "plane_cone_parallel_ellipse",
                    )
                };
                return Some((geometry, tag));
            }
            plane_cone_conic(plane, cone)
        }
        (CarrierEquation::Plane(plane), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Plane(plane)) => {
            let normal = normalize(plane.normal)?;
            let axis = normalize(torus.axis)?;
            if (dot(normal, axis).abs() - 1.0).abs() > EPS_AXIS_ORTHO {
                return None;
            }
            let axial = dot(
                axis,
                std::array::from_fn(|index| plane.origin[index] - torus.center[index]),
            );
            let scale = torus.minor_radius.max(torus.major_radius);
            if (axial.abs() - torus.minor_radius).abs() > EPS_CARRIER_AGREEMENT * scale {
                return None;
            }
            let center: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] + axial * axis[index]);
            let reference = normalize(torus.ref_direction)?;
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(normal),
                        Vector3::from(reference),
                        torus.major_radius,
                    )
                    .ok()?,
                )),
                "plane_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cylinder(first), CarrierEquation::Cylinder(second)) => {
            let first_axis = normalize(first.axis)?;
            let second_axis = normalize(second.axis)?;
            let alignment = dot(first_axis, second_axis);
            if (alignment.abs() - 1.0).abs() > EPS_AXIS_ORTHO {
                return None;
            }
            let relative = std::array::from_fn(|index| second.origin[index] - first.origin[index]);
            let axial = dot(relative, first_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * first_axis[index]);
            let distance = transverse[0].hypot(transverse[1]).hypot(transverse[2]);
            if distance <= EPS_DISTANCE_NONZERO {
                return None;
            }
            let external = first.radius + second.radius;
            let internal = (first.radius - second.radius).abs();
            let scale = external.max(distance);
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
                CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::from(origin),
                        Vector3::from(first_axis),
                    )
                    .ok()?,
                )),
                "parallel_cylinder_tangent_line",
            ))
        }
        (CarrierEquation::Sphere(first), CarrierEquation::Sphere(second)) => {
            let center_delta: [f64; 3] =
                std::array::from_fn(|index| second.center[index] - first.center[index]);
            let distance = center_delta[0]
                .hypot(center_delta[1])
                .hypot(center_delta[2]);
            let scale = distance.max(first.radius).max(second.radius);
            let first_radius = first.radius / scale;
            let second_radius = second.radius / scale;
            let separation = distance / scale;
            if !distance.is_finite()
                || separation <= 0.0
                || separation >= first_radius + second_radius
                || separation <= (first_radius - second_radius).abs()
            {
                return None;
            }
            let axis = center_delta.map(|value| value / distance);
            let relative_axial = 0.5
                * (separation
                    + ((first_radius - second_radius) / separation)
                        * (first_radius + second_radius));
            let axial = relative_axial * scale;
            let radius_ratio = (relative_axial / first_radius).abs();
            if !radius_ratio.is_finite() || radius_ratio >= 1.0 {
                return None;
            }
            let radius = first.radius * ((1.0 - radius_ratio) * (1.0 + radius_ratio)).sqrt();
            let center: [f64; 3] =
                std::array::from_fn(|index| first.center[index] + axial * axis[index]);
            let reference = cadmpeg_ir::geometry::derive_reference_direction(Vector3::from(axis));
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(center),
                        Vector3::from(axis),
                        reference,
                        radius,
                    )
                    .ok()?,
                )),
                "sphere_intersection_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cylinder(cylinder)) => {
            let axis = normalize(cylinder.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cylinder.origin[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = sphere.radius.max(cylinder.radius);
            if transverse[0].hypot(transverse[1]).hypot(transverse[2])
                > EPS_TRANSVERSE_RESIDUAL * scale
                || (sphere.radius - cylinder.radius).abs() > EPS_RADIUS_AGREEMENT * scale
            {
                return None;
            }
            let reference = normalize(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(sphere.center),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        cylinder.radius,
                    )
                    .ok()?,
                )),
                "coaxial_cylinder_sphere_circle",
            ))
        }
        (CarrierEquation::Cylinder(cylinder), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Cylinder(cylinder)) => {
            let cylinder_axis = normalize(cylinder.axis)?;
            let torus_axis = normalize(torus.axis)?;
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
                .max(cylinder.radius);
            if transverse[0].hypot(transverse[1]).hypot(transverse[2])
                > EPS_TRANSVERSE_RESIDUAL * scale
            {
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
            let reference = normalize(cylinder.ref_direction)?;
            Some((
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                    cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                        Point3::from(torus.center),
                        Vector3::from(cylinder_axis),
                        Vector3::from(reference),
                        cylinder.radius,
                    )
                    .ok()?,
                )),
                "coaxial_cylinder_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Cone(cone), CarrierEquation::Sphere(sphere))
        | (CarrierEquation::Sphere(sphere), CarrierEquation::Cone(cone)) => {
            if !circular_cone(cone) {
                return None;
            }
            let cone_axis = normalize(cone.axis())?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| sphere.center[index] - cone.origin()[index]);
            let axial = dot(relative, cone_axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * cone_axis[index]);
            let scale = cone.radius().max(sphere.radius);
            if transverse[0].hypot(transverse[1]).hypot(transverse[2])
                > EPS_TRANSVERSE_RESIDUAL * scale
            {
                return None;
            }
            let slope = cone.half_angle().tan();
            if slope.abs() <= EPS_CONE_SLOPE_NONZERO {
                return None;
            }
            let quadratic = 1.0 + slope * slope;
            let linear = 2.0 * (cone.radius() * slope - axial);
            let constant =
                cone.radius() * cone.radius() + axial * axial - sphere.radius * sphere.radius;
            let discriminant = linear.mul_add(linear, -4.0 * quadratic * constant);
            let discriminant_scale = linear.abs().max((4.0 * quadratic * constant).abs().sqrt());
            if discriminant.abs()
                > EPS_DISCRIMINANT_RESIDUAL * discriminant_scale * discriminant_scale
            {
                return None;
            }
            let cone_parameter = -linear / (2.0 * quadratic);
            let radius = (cone.radius() + cone_parameter * slope).abs();
            if radius <= EPS_RADIUS_NONZERO * scale {
                return None;
            }
            let center: [f64; 3] = std::array::from_fn(|index| {
                cone.origin()[index] + cone_parameter * cone_axis[index]
            });
            let reference = normalize(cone.ref_direction())?;
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
                "coaxial_cone_sphere_tangent_circle",
            ))
        }
        (CarrierEquation::Sphere(sphere), CarrierEquation::Torus(torus))
        | (CarrierEquation::Torus(torus), CarrierEquation::Sphere(sphere)) => {
            let axis = normalize(torus.axis)?;
            let relative: [f64; 3] =
                std::array::from_fn(|index| torus.center[index] - sphere.center[index]);
            let axial = dot(relative, axis);
            let transverse: [f64; 3] =
                std::array::from_fn(|index| relative[index] - axial * axis[index]);
            let scale = torus
                .major_radius
                .max(torus.minor_radius)
                .max(sphere.radius);
            if transverse[0].hypot(transverse[1]).hypot(transverse[2])
                > EPS_TRANSVERSE_RESIDUAL * scale
            {
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
            let reference = normalize(torus.ref_direction)?;
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
                "coaxial_sphere_torus_tangent_circle",
            ))
        }
        (CarrierEquation::Torus(first), CarrierEquation::Torus(second)) => {
            let first_axis = normalize(first.axis)?;
            let second_axis = normalize(second.axis)?;
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
                .max(second.minor_radius);
            if transverse[0].hypot(transverse[1]).hypot(transverse[2])
                > EPS_TRANSVERSE_RESIDUAL * scale
            {
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
            let reference = normalize(first.ref_direction)?;
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

#[cfg(test)]
mod tests {
    use super::{carrier_intersection_curve, CarrierEquation, CurveGeometry, SolvedCurveGeometry};
    use crate::decode::analytic::equations::{CylinderEquation, PlaneEquation, SphereEquation};

    fn cylinder(x: f64, radius: f64) -> CarrierEquation {
        CarrierEquation::Cylinder(CylinderEquation {
            origin: [x, 0., 0.],
            axis: [0., 0., 1.],
            ref_direction: [1., 0., 0.],
            radius,
        })
    }
    fn sphere(x: f64, radius: f64) -> CarrierEquation {
        CarrierEquation::Sphere(SphereEquation {
            center: [x, 0., 0.],
            ref_direction: [1., 0., 0.],
            radius,
        })
    }
    #[test]
    fn audit_regression_small_disjoint_carriers_have_no_tangent() {
        let radius = 1e-10;
        assert!(
            carrier_intersection_curve(cylinder(0., radius), cylinder(5. * radius, radius))
                .is_none()
        );
        assert!(
            carrier_intersection_curve(cylinder(0., radius), cylinder(2. * radius, radius))
                .is_some()
        );
        assert!(
            carrier_intersection_curve(cylinder(0., 2. * radius), sphere(0., radius)).is_none()
        );
        assert!(carrier_intersection_curve(cylinder(0., radius), sphere(0., radius)).is_some());
    }
    #[test]
    fn audit_regression_spherical_sections_keep_their_relative_radius() {
        for radius in [1e-200, 5e-10, 1.0, 1e200] {
            let plane = CarrierEquation::Plane(PlaneEquation {
                origin: [0., 0., 0.],
                normal: [0., 0., 1.],
            });
            let (section, _) = carrier_intersection_curve(plane, sphere(0., radius))
                .expect("nondegenerate plane section");
            let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = section else {
                panic!("circle section")
            };
            assert_eq!(circle.radius().get(), radius);
            let (section, _) =
                carrier_intersection_curve(sphere(0., radius), sphere(radius, radius))
                    .expect("nondegenerate sphere section");
            let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = section else {
                panic!("circle section")
            };
            assert!(
                (circle.radius().get() / radius - 3.0_f64.sqrt() / 2.).abs() <= 8. * f64::EPSILON
            );
        }
    }
}
