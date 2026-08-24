// SPDX-License-Identifier: Apache-2.0
//! Extrusion and revolution pcurves.

use super::super::analytic::{cross, dot, nurbs_intrinsic_parameter_range};
use super::super::native::annotate;
use super::super::sketch::{normalized, section_point_in_model};
use super::nurbs::{oriented_sketch_nurbs_curve, placed_section_nurbs};
use super::profiles::{circular_pcurve, line_pcurve, profile_arc};
use super::surfaces::{revolved_nurbs_surface, revolved_section_surface};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::{CurveGeometry, Pcurve, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

const EPS_SENSE_ALIGN: f64 = 1e-8;
const EPS_RADIUS_NONZERO: f64 = 1e-12;
const EPS_RESIDUAL_AGREEMENT: f64 = 1e-9;
const EPS_SURFACE_DIFFERENCE_STEP: f64 = 1e-6;

pub(in super::super) fn add_extrusion_pcurve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    id: PcurveId,
    source_offset: usize,
    geometry: PcurveGeometry,
) -> PcurveId {
    let parameter_range = match &geometry {
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            ..
        } => usize::try_from(*degree)
            .ok()
            .and_then(|degree| {
                (control_points.len() > degree && knots.len() == control_points.len() + degree + 1)
                    .then_some(())?;
                Some([*knots.get(degree)?, *knots.get(control_points.len())?])
            })
            .filter(|range| range[0] < range[1])
            .unwrap_or([0.0, 1.0]),
        _ => [0.0, 1.0],
    };
    annotate(
        annotations,
        &id,
        "FeatDefs",
        source_offset as u64,
        "extrusion_trim_pcurve",
        Exactness::Derived,
    );
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry,
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some(parameter_range),
        fit_tolerance: None,
    });
    id
}

pub(in super::super) fn revolution_boundary_pcurve(
    surface: &SurfaceGeometry,
    point: [f64; 3],
    axis: RevolutionAxis,
) -> Option<PcurveGeometry> {
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let point_from = |origin: Point3| {
        [
            point[0] - origin.x,
            point[1] - origin.y,
            point[2] - origin.z,
        ]
    };
    let vector = |value: Vector3| [value.x, value.y, value.z];
    let azimuth = |relative: [f64; 3], carrier_axis: [f64; 3], reference: [f64; 3]| {
        let tangent = cross(carrier_axis, reference);
        dot(relative, tangent).atan2(dot(relative, reference))
    };
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let normal = vector(*normal);
            let u_axis = vector(*u_axis);
            let v_axis = cross(normal, u_axis);
            let axis_relative = [
                axis_origin[0] - origin.x,
                axis_origin[1] - origin.y,
                axis_origin[2] - origin.z,
            ];
            let center = [dot(axis_relative, u_axis), dot(axis_relative, v_axis)];
            let relative = point_from(*origin);
            let uv = [dot(relative, u_axis), dot(relative, v_axis)];
            let radial = [uv[0] - center[0], uv[1] - center[1]];
            let radius = radial[0].hypot(radial[1]);
            (radius > EPS_RADIUS_NONZERO).then_some(())?;
            let start = radial[1].atan2(radial[0]);
            let direction = if dot(normal, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(circular_pcurve(center, radius, start, start + direction))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ..
        } => {
            let carrier_axis = vector(*axis);
            let relative = point_from(*origin);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let v = dot(relative, carrier_axis);
            let direction = if dot(carrier_axis, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(line_pcurve([u, v], [u + direction, v]))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            ..
        } => {
            let carrier_axis = vector(*axis);
            let relative = point_from(*center);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let axial = dot(relative, carrier_axis);
            let radial = std::array::from_fn::<_, 3, _>(|index| {
                relative[index] - axial * carrier_axis[index]
            });
            let v = axial.atan2(dot(radial, radial).sqrt());
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v]))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let carrier_axis = vector(*axis);
            let reference = vector(*ref_direction);
            let relative = point_from(*center);
            let axial = dot(relative, carrier_axis);
            let radial = std::array::from_fn::<_, 3, _>(|index| {
                relative[index] - axial * carrier_axis[index]
            });
            let radial_distance = dot(radial, radial).sqrt();
            let positive_residual = ((radial_distance - major_radius)
                .mul_add(radial_distance - major_radius, axial * axial)
                - minor_radius * minor_radius)
                .abs();
            let negative_residual = ((-radial_distance - major_radius)
                .mul_add(-radial_distance - major_radius, axial * axial)
                - minor_radius * minor_radius)
                .abs();
            let base_u = azimuth(relative, carrier_axis, reference);
            let (u, signed_ring) = if negative_residual < positive_residual {
                (base_u + std::f64::consts::PI, -radial_distance)
            } else {
                (base_u, radial_distance)
            };
            let scale = minor_radius.abs().max(radial_distance).max(1.0);
            (positive_residual.min(negative_residual) <= EPS_RESIDUAL_AGREEMENT * scale * scale)
                .then_some(())?;
            let v = axial.atan2(signed_ring - major_radius);
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v]))
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(in super::super) fn revolved_brep_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    reversed: bool,
    axis: RevolutionAxis,
) -> Option<SurfaceGeometry> {
    if matches!(geometry, SketchGeometry::Nurbs { .. }) {
        let directrix = oriented_sketch_nurbs_curve(geometry, reversed)?;
        return Some(SurfaceGeometry::Nurbs(revolved_nurbs_surface(
            &placed_section_nurbs(transform, &directrix),
            axis,
        )?));
    }
    revolved_section_surface(transform, geometry, axis)
}

pub(in super::super) fn revolution_profile_boundary_pcurve(
    transform: &crate::placement::FeatureSectionTransform,
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    surface: &SurfaceGeometry,
    axis: RevolutionAxis,
    section_point: [f64; 2],
    at_start: bool,
) -> Option<PcurveGeometry> {
    if matches!(segment.0, SketchGeometry::Nurbs { .. }) {
        let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = if at_start { lower } else { upper };
        return Some(line_pcurve(
            [parameter, 0.0],
            [parameter, std::f64::consts::TAU],
        ));
    }
    revolution_boundary_pcurve(
        surface,
        section_point_in_model(transform, section_point),
        axis,
    )
}

pub(in super::super) fn revolution_face_sense(
    transform: &crate::placement::FeatureSectionTransform,
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    surface: &SurfaceGeometry,
    axis: RevolutionAxis,
    profile_area: f64,
) -> Option<Sense> {
    let is_nurbs = matches!(segment.0, SketchGeometry::Nurbs { .. });
    let (point, tangent, pcurve_parameter, u_epsilon) = if is_nurbs {
        let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = lower + (upper - lower) * 0.5;
        let carrier = CurveGeometry::Nurbs(nurbs);
        let point = cadmpeg_ir::eval::curve_point(&carrier, parameter)?;
        let tangent = cadmpeg_ir::eval::curve_tangent(&carrier, parameter)?;
        (
            [point.x, point.y],
            [tangent.x, tangent.y],
            0.5,
            (upper - lower).abs() * EPS_SURFACE_DIFFERENCE_STEP,
        )
    } else if let Some((center, radius, start, delta)) = profile_arc(segment) {
        let angle = start + 0.5 * delta;
        (
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ],
            [-delta.signum() * angle.sin(), delta.signum() * angle.cos()],
            0.0,
            EPS_SURFACE_DIFFERENCE_STEP,
        )
    } else {
        (
            [
                0.5 * (segment.2[0] + segment.3[0]),
                0.5 * (segment.2[1] + segment.3[1]),
            ],
            [segment.3[0] - segment.2[0], segment.3[1] - segment.2[1]],
            0.0,
            EPS_SURFACE_DIFFERENCE_STEP,
        )
    };
    let outward = if profile_area.is_sign_positive() {
        [tangent[1], -tangent[0]]
    } else {
        [-tangent[1], tangent[0]]
    };
    let outward = normalized(std::array::from_fn(|index| {
        outward[0] * transform.u_axis[index] + outward[1] * transform.v_axis[index]
    }))?;
    let model_point = section_point_in_model(transform, point);
    let pcurve = if is_nurbs {
        let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = lower + (upper - lower) * 0.5;
        line_pcurve([parameter, 0.0], [parameter, std::f64::consts::TAU])
    } else {
        revolution_boundary_pcurve(surface, model_point, axis)?
    };
    let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve, pcurve_parameter)?;
    let before_u = cadmpeg_ir::eval::surface_point(surface, uv.u - u_epsilon, uv.v)?;
    let after_u = cadmpeg_ir::eval::surface_point(surface, uv.u + u_epsilon, uv.v)?;
    let before_v =
        cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v - EPS_SURFACE_DIFFERENCE_STEP)?;
    let after_v =
        cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v + EPS_SURFACE_DIFFERENCE_STEP)?;
    let du = [
        after_u.x - before_u.x,
        after_u.y - before_u.y,
        after_u.z - before_u.z,
    ];
    let dv = [
        after_v.x - before_v.x,
        after_v.y - before_v.y,
        after_v.z - before_v.z,
    ];
    let carrier_normal = normalized(cross(du, dv))?;
    let alignment = dot(carrier_normal, outward);
    (alignment.abs() > EPS_SENSE_ALIGN).then_some(())?;
    Some(if alignment.is_sign_positive() {
        Sense::Forward
    } else {
        Sense::Reversed
    })
}
