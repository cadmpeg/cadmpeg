// SPDX-License-Identifier: Apache-2.0
//! Section surface and curve construction and extrusion surface transfer.

use super::super::feature_history::draft::feature_allows_linear_extrusion;
use super::super::feature_history::link::{
    analytic_surface_id_for_feature, generated_surface_id_for_feature, surface_kind_for_geometry,
};
use super::super::native::annotate;
use super::super::sketch::coordinates::resolved_section_points;
use super::super::sketch::geometry::{
    resolved_section_segment_geometry, saved_section_entity_geometry,
};
use super::super::sketch::intersect::section_point_in_model;
use super::super::sketch::radii::trim_segment_id;
use super::super::sketch::skamp::complete_section_segment_rows;
use super::super::sketch_ids::sketch_section_curve_id;
use super::super::uniqueness::{
    unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::extent::resolved_feature_extrusion_span;
use super::nurbs::{
    extruded_geometry_surface, extruded_nurbs_surface, placed_section_nurbs, saved_spline_nurbs,
    translated_nurbs_curve,
};
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::identity::semantic_saved_section_entities;
use crate::vecmath::normalize;
use crate::vecmath::{cross, dot};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};

const EPS_RADIUS_NONZERO: f64 = 1.0e-10;
const EPS_COPLANAR_RESIDUAL: f64 = 1.0e-9;
const EPS_RADIAL_SPEED: f64 = 1.0e-10;
const EPS_AXIAL_RATE: f64 = 1.0e-10;
const EPS_MAJOR_RADIUS: f64 = 1.0e-10;
use cadmpeg_ir::ids::{CurveId, IdentityKey, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::BTreeSet;

pub(in super::super) fn revolved_section_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    revolution_axis: &RevolutionAxis,
) -> Option<SurfaceGeometry> {
    let axis = normalize([
        revolution_axis.direction.x,
        revolution_axis.direction.y,
        revolution_axis.direction.z,
    ])?;
    let axis_origin = [
        revolution_axis.origin.x,
        revolution_axis.origin.y,
        revolution_axis.origin.z,
    ];
    let project = |point: [f64; 3]| {
        let displacement = std::array::from_fn(|index| point[index] - axis_origin[index]);
        let axial = dot(displacement, axis);
        let on_axis = std::array::from_fn(|index| axis_origin[index] + axial * axis[index]);
        let radial = std::array::from_fn(|index| point[index] - on_axis[index]);
        (on_axis, radial)
    };
    let point = |values: [f64; 3]| Point3::from(values);
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalize(std::array::from_fn(|index| end[index] - start[index]))?;
            let (mut on_axis, mut radial) = project(start);
            let mut radius = Vector3::from(radial).norm();
            if radius <= EPS_RADIUS_NONZERO {
                (on_axis, radial) = project(end);
                radius = Vector3::from(radial).norm();
            }
            let axial_rate = dot(direction, axis);
            let radial_rate =
                std::array::from_fn(|index| direction[index] - axial_rate * axis[index]);
            let radial_speed = dot(radial_rate, radial_rate).sqrt();
            let scale = radius;
            if radius > EPS_RADIUS_NONZERO {
                let coplanar_residual = dot(cross(radial, radial_rate), axis).abs();
                (coplanar_residual <= EPS_COPLANAR_RESIDUAL * scale).then_some(())?;
            }
            let reference = normalize(radial).or_else(|| normalize(radial_rate))?;
            if radial_speed <= EPS_RADIAL_SPEED {
                (radius > EPS_RADIUS_NONZERO).then_some(())?;
                return Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        point(on_axis),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        radius,
                    )
                    .ok()?,
                )));
            }
            if axial_rate.abs() <= EPS_AXIAL_RATE {
                return Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        point(on_axis),
                        Vector3::from(axis),
                        Vector3::from(reference),
                    )
                    .ok()?,
                )));
            }
            let radial_rate = dot(radial_rate, reference);
            let cone_axis = if radial_rate / axial_rate < 0.0 {
                std::array::from_fn(|index| -axis[index])
            } else {
                axis
            };
            Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    point(on_axis),
                    Vector3::from(cone_axis),
                    Vector3::from(reference),
                    radius,
                    1.0,
                    radial_rate.abs().atan2(axial_rate.abs()),
                )
                .ok()?,
            )))
        }
        SketchGeometryDefinition::Arc { center, radius, .. }
        | SketchGeometryDefinition::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            let (on_axis, radial) = project(center);
            let major_radius = Vector3::from(radial).norm();
            let reference = normalize(radial).or_else(|| {
                [transform.u_axis(), transform.v_axis()]
                    .into_iter()
                    .find_map(|candidate| {
                        let axial = dot(candidate, axis);
                        normalize(std::array::from_fn(|index| {
                            candidate[index] - axial * axis[index]
                        }))
                    })
            })?;
            if major_radius <= EPS_MAJOR_RADIUS {
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
                    cadmpeg_ir::geometry::analytic::SphereSurface::try_new(
                        point(center),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        radius.get(),
                    )
                    .ok()?,
                )))
            } else {
                Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
                    cadmpeg_ir::geometry::analytic::TorusSurface::try_new(
                        point(on_axis),
                        Vector3::from(axis),
                        Vector3::from(reference),
                        major_radius,
                        radius.get(),
                    )
                    .ok()?,
                )))
            }
        }
        _ => None,
    }
}

pub(in super::super) fn placed_section_geometry_curve(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<CurveGeometry> {
    match geometry.definition() {
        SketchGeometryDefinition::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalize(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::from(start),
                    Vector3::from(direction),
                )
                .ok()?,
            )))
        }
        SketchGeometryDefinition::ReferenceLine { origin, direction } => {
            let origin = section_point_in_model(transform, [origin.u, origin.v]);
            let direction = normalize([
                direction.u * transform.u_axis()[0] + direction.v * transform.v_axis()[0],
                direction.u * transform.u_axis()[1] + direction.v * transform.v_axis()[1],
                direction.u * transform.u_axis()[2] + direction.v * transform.v_axis()[2],
            ])?;
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::from(origin),
                    Vector3::from(direction),
                )
                .ok()?,
            )))
        }
        SketchGeometryDefinition::Arc { center, radius, .. }
        | SketchGeometryDefinition::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                    Point3::from(center),
                    transform.normal_vector(),
                    transform.u_axis_vector(),
                    radius.get(),
                )
                .ok()?,
            )))
        }
        _ => None,
    }
}

pub(in super::super) fn placed_sketch_curve_ref(
    transform: Option<&crate::placement::FeatureSectionTransform>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
    geometry: &SketchGeometry,
) -> Option<String> {
    placed_section_geometry_curve(transform?, geometry)?;
    Some(sketch_section_curve_id(sketch, suffix))
}

fn unique_feature_surface_row(
    rows: &[crate::surface::SurfaceRow],
    surface_id: u32,
    feature_id: u32,
    expected_kind: crate::surface::SurfaceKind,
) -> bool {
    crate::surface::unique_surface_row(rows, surface_id)
        .is_some_and(|row| row.feature_id == feature_id && row.kind.same_family(expected_kind))
}

pub(in super::super) fn transfer_saved_spline_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        for spline in
            semantic_saved_section_entities(definition).filter_map(|entity| match entity {
                crate::feature::definitions::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let Some(nurbs) = saved_spline_nurbs(spline, &mut refusal) else {
                let records = refusal.take_records();
                losses.push(crate::loss::CreoLossCode::SectionSplineUnresolved.note(
                    if records.is_empty() {
                        format!(
                            "Saved section spline at offset {} cannot form a NURBS curve.",
                            spline.offset
                        )
                    } else {
                        format!(
                            "Saved section spline at offset {} cannot form a NURBS curve: {}",
                            spline.offset,
                            records.join("; ")
                        )
                    },
                ));
                continue;
            };
            let suffix_key = spline
                .entity_id
                .map_or_else(
                    || IdentityKey::try_new(format!("offset{}", spline.offset)),
                    |entity_id| Ok(IdentityKey::from(entity_id)),
                )
                .map_err(cadmpeg_core::CodecError::malformed)?;
            let curve_id = CurveId::compose(
                &crate::identity::FEATDEFS_SAVED_SPLINE_CURVE,
                IdentityKey::from(definition.identity.id()).colon(suffix_key.clone()),
            );
            if ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                continue;
            }
            let Some(placed) = placed_section_nurbs(transform, &nurbs) else {
                continue;
            };
            annotate(
                annotations,
                &curve_id,
                "FeatDefs",
                spline.offset as u64,
                "placed_saved_interpolation_spline",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: curve_id,
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(placed)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "FeatDefs:saved_spline#{}",
                        suffix_key.as_str()
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    Ok(transferred)
}

pub(in super::super) fn revolved_nurbs_surface(
    directrix: &NurbsCurve,
    axis: &RevolutionAxis,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<NurbsSurface> {
    let axis_direction = normalize([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let angular_poles = [
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [-1.0, 1.0],
        [-1.0, 0.0],
        [-1.0, -1.0],
        [0.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
    ];
    let diagonal_weight = std::f64::consts::FRAC_1_SQRT_2;
    let angular_weights = [
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
    ];
    let mut control_points = Vec::with_capacity(directrix.control_points().len() * 9);
    let mut weights = Vec::with_capacity(directrix.control_points().len() * 9);
    for (index, point) in directrix.control_points().iter().enumerate() {
        let relative = [
            point.x - axis_origin[0],
            point.y - axis_origin[1],
            point.z - axis_origin[2],
        ];
        let axial_distance = dot(relative, axis_direction);
        let center: [f64; 3] = std::array::from_fn(|component| {
            axis_origin[component] + axial_distance * axis_direction[component]
        });
        let radial = [
            point.x - center[0],
            point.y - center[1],
            point.z - center[2],
        ];
        let tangent = cross(axis_direction, radial);
        let directrix_weight = directrix
            .weights()
            .map_or(1.0, |curve_weights| curve_weights[index]);
        for ([radial_scale, tangent_scale], angular_weight) in
            angular_poles.into_iter().zip(angular_weights)
        {
            control_points.push(Point3::new(
                center[0] + radial_scale * radial[0] + tangent_scale * tangent[0],
                center[1] + radial_scale * radial[1] + tangent_scale * tangent[1],
                center[2] + radial_scale * radial[2] + tangent_scale * tangent[2],
            ));
            weights.push(directrix_weight * angular_weight);
        }
    }
    match NurbsSurface::from_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            directrix.degree(),
            directrix.knots().to_vec(),
            false,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
            2,
            vec![
                0.0,
                0.0,
                0.0,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::FRAC_PI_2,
                std::f64::consts::PI,
                std::f64::consts::PI,
                3.0 * std::f64::consts::FRAC_PI_2,
                3.0 * std::f64::consts::FRAC_PI_2,
                std::f64::consts::TAU,
                std::f64::consts::TAU,
                std::f64::consts::TAU,
            ],
            false,
        ),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(
            control_points.chunks(9_usize).map(<[_]>::to_vec).collect(),
            Some(weights).map(|values| values.chunks(9_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    ) {
        Ok(surface) => Some(surface),
        Err(error) => {
            refusal.note(
                format!("creo revolved NURBS surface record for {record}"),
                &error,
            );
            None
        }
    }
}

pub(in super::super) struct RevolvedSectionCircle {
    center: Point3,
    axis: Vector3,
    ref_direction: Vector3,
    radius: f64,
}

impl TryFrom<RevolvedSectionCircle> for CurveGeometry {
    type Error = &'static str;

    fn try_from(circle: RevolvedSectionCircle) -> Result<Self, Self::Error> {
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            circle.center,
            circle.axis,
            circle.ref_direction,
            circle.radius,
        )
        .map(|curve| Self::Solved(SolvedCurveGeometry::Circle(curve)))
    }
}

pub(in super::super) fn revolved_section_circle(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
    axis: &RevolutionAxis,
) -> Option<RevolvedSectionCircle> {
    let axis_direction = normalize([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let point = section_point_in_model(transform, point);
    let relative: [f64; 3] =
        std::array::from_fn(|component| point[component] - axis_origin[component]);
    let axial_distance = dot(relative, axis_direction);
    let center: [f64; 3] = std::array::from_fn(|component| {
        axis_origin[component] + axial_distance * axis_direction[component]
    });
    let radial: [f64; 3] = std::array::from_fn(|component| point[component] - center[component]);
    let radius = dot(radial, radial).sqrt();
    let scale = point
        .iter()
        .chain(&axis_origin)
        .map(|coordinate| coordinate.abs())
        .fold(1.0, f64::max);
    (radius > EPS_RADIUS_NONZERO * scale).then_some(())?;
    let reference = radial.map(|component| component / radius);
    Some(RevolvedSectionCircle {
        center: Point3::from(center),
        axis: Vector3::from(axis_direction),
        ref_direction: Vector3::from(reference),
        radius,
    })
}

pub(in super::super) fn extruded_section_line(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> Option<CurveGeometry> {
    let direction = transform.normal();
    let origin = section_point_in_model(transform, point);
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            Point3::from(origin),
            Vector3::from(direction),
        )
        .ok()?,
    )))
}

pub(in super::super) fn transfer_feature_extrusion_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(scan, feature_id) {
            continue;
        }
        let Some(order_table) = &definition.order_table else {
            continue;
        };
        let points = resolved_section_points(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|trim_entities| &trim_entities.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        for segment in complete_section_segment_rows(definition)
            .iter()
            .filter(|segment| solved.contains(&segment.external_id))
        {
            let Some(section_geometry) =
                resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(surface_id) = analytic_surface_id_for_feature(
                &scan.surfaces.rows,
                &scan.features.entity_tables,
                feature_id,
                segment.external_id,
                &geometry,
            ) else {
                continue;
            };
            let id = SurfaceId::compose(
                &crate::identity::VISIBGEOM_SURFACE,
                IdentityKey::from(surface_id),
            );
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "protextrude_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{surface_id}"
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }

        for (internal_id, section_geometry, offset) in
            semantic_saved_section_entities(definition).filter_map(saved_section_entity_geometry)
        {
            let Some(external_id) = order_table.external_id(internal_id) else {
                continue;
            };
            let Some(native_surface_id) = generated_surface_id_for_feature(
                &scan.features.entity_tables,
                feature_id,
                external_id,
            ) else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(expected_kind) = surface_kind_for_geometry(&geometry) else {
                continue;
            };
            if !unique_feature_surface_row(
                &scan.surfaces.rows,
                native_surface_id,
                feature_id,
                expected_kind,
            ) {
                continue;
            }
            let id = SurfaceId::compose(
                &crate::identity::VISIBGEOM_SURFACE,
                IdentityKey::from(native_surface_id),
            );
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                offset as u64,
                "protextrude_saved_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{native_surface_id}"
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }

        let splines = semantic_saved_section_entities(definition)
            .filter_map(|entity| match entity {
                crate::feature::definitions::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
            .filter_map(|spline| {
                let internal_id = spline.entity_id?;
                let external_id = order_table.external_id(internal_id)?;
                let surface_id = generated_surface_id_for_feature(
                    &scan.features.entity_tables,
                    feature_id,
                    external_id,
                )?;
                unique_feature_surface_row(
                    &scan.surfaces.rows,
                    surface_id,
                    feature_id,
                    crate::surface::SurfaceKind::Extrusion(
                        crate::surface::ExtrusionVariant::Linear,
                    ),
                )
                .then_some((surface_id, internal_id, spline))
            })
            .collect::<Vec<_>>();
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let lower_translation = transform.normal().map(|value| value * span.lower());
        let sweep = transform
            .normal()
            .map(|value| value * (span.upper() - span.lower()));
        for (native_surface_id, internal_id, spline) in splines {
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let Some(section_curve) = saved_spline_nurbs(spline, &mut refusal) else {
                let records = refusal.take_records();
                losses.push(crate::loss::CreoLossCode::SectionSplineUnresolved.note(
                    if records.is_empty() {
                        format!(
                            "Saved section spline at offset {} cannot form a NURBS curve.",
                            spline.offset
                        )
                    } else {
                        format!(
                            "Saved section spline at offset {} cannot form a NURBS curve: {}",
                            spline.offset,
                            records.join("; ")
                        )
                    },
                ));
                continue;
            };
            let Some(placed) = placed_section_nurbs(transform, &section_curve) else {
                continue;
            };
            let Some(directrix) = translated_nurbs_curve(&placed, lower_translation) else {
                continue;
            };
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let surface_record = format!(
                "surface {native_surface_id} from saved-spline entity {internal_id} at offset {}",
                spline.offset
            );
            let Some(surface) =
                extruded_nurbs_surface(&directrix, sweep, &surface_record, &mut refusal)
            else {
                for record in refusal.take_records() {
                    losses.push(
                        crate::loss::CreoLossCode::SectionSplineUnresolved.note(format!(
                            "Extruded section spline at offset {} states no surface carrier: {record}",
                            spline.offset
                        )),
                    );
                }
                continue;
            };
            let suffix_key = IdentityKey::from(internal_id);
            let curve_id = CurveId::compose(
                &crate::identity::FEATURE_EXTRUSION_DIRECTRIX,
                IdentityKey::from(feature_id).colon(suffix_key.clone()),
            );
            if !ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                annotate(
                    annotations,
                    &curve_id,
                    "FeatDefs",
                    spline.offset as u64,
                    "protextrude_spline_directrix",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix.clone())),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::new(format!(
                            "FeatDefs:saved_spline#{}",
                            suffix_key.as_str()
                        ))
                        .ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(
                                "source object_id must not be empty",
                            )
                        })?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            let surface_id = SurfaceId::compose(
                &crate::identity::VISIBGEOM_SURFACE,
                IdentityKey::from(native_surface_id),
            );
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            let procedural_id = ProceduralSurfaceId::compose(
                &crate::identity::FEATURE_EXTRUSION_CONSTRUCTION,
                IdentityKey::from(feature_id).colon(suffix_key),
            );
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{native_surface_id}"
                    ))
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            let Some((&lower_knot, &upper_knot)) =
                directrix.knots().first().zip(directrix.knots().last())
            else {
                losses.push(
                    crate::loss::CreoLossCode::SectionSplineUnresolved.note(format!(
                    "Extrusion directrix for feature {feature_id} at offset {} has no knot range",
                    spline.offset
                )),
                );
                continue;
            };
            let _attached = ir.model.add_procedural_surface(
                surface_id,
                cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                    curve_id,
                    Some([lower_knot, upper_knot]),
                    Vector3::from(sweep),
                    None,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .map(|admitted_payload| {
                    ProceduralSurface::new(
                        procedural_id,
                        ProceduralSurfaceDefinition::Extrusion(admitted_payload),
                        None,
                    )
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            );
            transferred += 1;
        }
    }
    Ok(transferred)
}

#[cfg(test)]
mod tests;
