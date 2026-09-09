// SPDX-License-Identifier: Apache-2.0
//! Extrusion and revolution pcurves.

use super::super::native::annotate;
use super::super::sketch::section_point_in_model;
use super::nurbs::{oriented_sketch_nurbs_curve, placed_section_nurbs};
use super::profiles::{circular_pcurve, line_pcurve, profile_arc};
use super::surfaces::{revolved_nurbs_surface, revolved_section_surface};
use crate::decode::analytic::edges::nurbs_intrinsic_parameter_range;
use crate::vecmath::normalize;
use crate::vecmath::{cross, dot};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::{CurveGeometry, Pcurve, PcurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition};
use cadmpeg_ir::topology::Sense;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

const EPS_SENSE_ALIGN: f64 = 1.0e-8;
const EPS_RADIUS_NONZERO: f64 = 1.0e-12;
const EPS_RESIDUAL_AGREEMENT: f64 = 1.0e-9;
const EPS_SURFACE_DIFFERENCE_STEP: f64 = 1.0e-6;

pub(in super::super) fn add_extrusion_pcurve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    id: PcurveId,
    source_offset: usize,
    geometry: PcurveGeometry,
) -> Result<PcurveId, cadmpeg_core::CodecError> {
    let parameter_range = match &geometry {
        PcurveGeometry::Nurbs { nurbs } => usize::try_from(nurbs.degree())
            .ok()
            .and_then(|degree| {
                Some([
                    *nurbs.knots().get(degree)?,
                    *nurbs.knots().get(nurbs.control_points().len())?,
                ])
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
        metadata: cadmpeg_ir::geometry::PcurveMetadata::try_general(
            None,
            Some(parameter_range),
            None,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?,
    });
    Ok(id)
}

pub(in super::super) fn revolution_boundary_pcurve(
    surface: &SurfaceGeometry,
    point: [f64; 3],
    axis: &RevolutionAxis,
) -> Option<PcurveGeometry> {
    let axis_direction = normalize([axis.direction.x, axis.direction.y, axis.direction.z])?;
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
        SurfaceGeometry::Plane(plane_surface) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.normal();
            let u_axis = plane_surface.u_axis();
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
            Some(circular_pcurve(center, radius, start, start + direction)?)
        }
        SurfaceGeometry::Cylinder(cylinder_surface) => {
            let origin = cylinder_surface.origin();
            let axis = cylinder_surface.axis();
            let ref_direction = cylinder_surface.ref_direction();
            let carrier_axis = vector(*axis);
            let relative = point_from(*origin);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let v = dot(relative, carrier_axis);
            let direction = if dot(carrier_axis, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(line_pcurve([u, v], [u + direction, v])?)
        }
        SurfaceGeometry::Cone(cone_surface) => {
            let origin = cone_surface.origin();
            let axis = cone_surface.axis();
            let ref_direction = cone_surface.ref_direction();
            let carrier_axis = vector(*axis);
            let relative = point_from(*origin);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let v = dot(relative, carrier_axis);
            let direction = if dot(carrier_axis, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(line_pcurve([u, v], [u + direction, v])?)
        }
        SurfaceGeometry::Sphere(sphere_surface) => {
            let center = sphere_surface.center();
            let axis = sphere_surface.axis();
            let ref_direction = sphere_surface.ref_direction();
            let carrier_axis = vector(*axis);
            let relative = point_from(*center);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let axial = dot(relative, carrier_axis);
            let radial = std::array::from_fn::<_, 3, _>(|index| {
                relative[index] - axial * carrier_axis[index]
            });
            let v = axial.atan2(dot(radial, radial).sqrt());
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v])?)
        }
        SurfaceGeometry::Torus(torus_surface) => {
            let center = torus_surface.center();
            let axis = torus_surface.axis();
            let ref_direction = torus_surface.ref_direction();
            let major_radius = torus_surface.major_radius();
            let minor_radius = torus_surface.minor_radius();
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
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v])?)
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(in super::super) fn revolved_brep_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    reversed: bool,
    axis: &RevolutionAxis,
) -> Option<SurfaceGeometry> {
    if matches!(
        geometry.definition(),
        SketchGeometryDefinition::Nurbs { .. }
    ) {
        let directrix = oriented_sketch_nurbs_curve(geometry, reversed)?;
        return Some(SurfaceGeometry::Nurbs(revolved_nurbs_surface(
            &placed_section_nurbs(transform, &directrix)?,
            axis,
        )?));
    }
    revolved_section_surface(transform, geometry, axis)
}

/// An endpoint boundary of a revolved profile segment.
#[derive(Clone, Copy)]
pub(in super::super) enum RevolutionBoundary {
    Start,
    End,
}

impl RevolutionBoundary {
    /// The boundary key used in native identities.
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }

    /// The other endpoint boundary.
    pub(super) const fn opposite(self) -> Self {
        match self {
            Self::Start => Self::End,
            Self::End => Self::Start,
        }
    }
}

pub(in super::super) fn revolution_profile_boundary_pcurve(
    transform: &crate::placement::FeatureSectionTransform,
    segment: &super::profiles::ProfileEntity,
    surface: &SurfaceGeometry,
    axis: &RevolutionAxis,
    section_point: [f64; 2],
    boundary: RevolutionBoundary,
) -> Option<PcurveGeometry> {
    if matches!(
        segment.geometry(),
        super::profiles::ProfileGeometry::Nurbs { .. }
    ) {
        let nurbs =
            oriented_sketch_nurbs_curve(&segment.geometry().to_sketch()?, segment.reversed())?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = match boundary {
            RevolutionBoundary::Start => lower,
            RevolutionBoundary::End => upper,
        };
        return line_pcurve([parameter, 0.0], [parameter, std::f64::consts::TAU]);
    }
    revolution_boundary_pcurve(
        surface,
        section_point_in_model(transform, section_point),
        axis,
    )
}

pub(in super::super) fn revolution_face_sense(
    transform: &crate::placement::FeatureSectionTransform,
    segment: &super::profiles::ProfileEntity,
    surface: &SurfaceGeometry,
    axis: &RevolutionAxis,
    profile_area: f64,
) -> Option<Sense> {
    let is_nurbs = matches!(
        segment.geometry(),
        super::profiles::ProfileGeometry::Nurbs { .. }
    );
    let (point, tangent, pcurve_parameter, u_epsilon) = if is_nurbs {
        let nurbs =
            oriented_sketch_nurbs_curve(&segment.geometry().to_sketch()?, segment.reversed())?;
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
                0.5 * (segment.start()[0] + segment.end()[0]),
                0.5 * (segment.start()[1] + segment.end()[1]),
            ],
            [
                segment.end()[0] - segment.start()[0],
                segment.end()[1] - segment.start()[1],
            ],
            0.0,
            EPS_SURFACE_DIFFERENCE_STEP,
        )
    };
    let outward = if profile_area.is_sign_positive() {
        [tangent[1], -tangent[0]]
    } else {
        [-tangent[1], tangent[0]]
    };
    let outward = normalize(std::array::from_fn(|index| {
        outward[0] * transform.u_axis()[index] + outward[1] * transform.v_axis()[index]
    }))?;
    let model_point = section_point_in_model(transform, point);
    let pcurve = if is_nurbs {
        let nurbs =
            oriented_sketch_nurbs_curve(&segment.geometry().to_sketch()?, segment.reversed())?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = lower + (upper - lower) * 0.5;
        line_pcurve([parameter, 0.0], [parameter, std::f64::consts::TAU])?
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
    let carrier_normal = normalize(cross(du, dv))?;
    let alignment = dot(carrier_normal, outward);
    (alignment.abs() > EPS_SENSE_ALIGN).then_some(())?;
    Some(if alignment.is_sign_positive() {
        Sense::Forward
    } else {
        Sense::Reversed
    })
}

#[cfg(test)]
mod tests;
