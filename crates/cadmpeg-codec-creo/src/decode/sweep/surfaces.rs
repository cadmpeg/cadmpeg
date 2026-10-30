// SPDX-License-Identifier: Apache-2.0
//! Section surface and curve construction and extrusion surface transfer.

use super::super::feature_history::draft::feature_allows_linear_extrusion_with;
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
use crate::curve::TwoChartPcurveSamples;
use crate::decode::analytic::pcurves::{
    linear_pcurve_carrier, unresolved_plane_chart_ids, ModelSurfaceIndex,
};
use crate::decode::sketch_transfer::identity::semantic_saved_section_entities;
use crate::decode::source_carriers::SourceUnitCarriers;
use crate::decode::surfaces::intersection_resolve::curve_contains_points;
use crate::decode::surfaces::prototypes::{
    surface_prototype_frame_bounds, unique_surface_prototype_associations,
};
use crate::lane_refusal::JoinedLaneRecords;
use crate::loss::CreoLossCode;
use crate::scalar;
use crate::vecmath::normalize;
use crate::vecmath::{cross, dot};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::RevolutionAxis;
use cadmpeg_ir::geometry::analytic::CircleCurve;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

const EPS_RADIUS_NONZERO: f64 = 1.0e-10;
const EPS_COPLANAR_RESIDUAL: f64 = 1.0e-9;
const EPS_RADIAL_SPEED: f64 = 1.0e-10;
const EPS_AXIAL_RATE: f64 = 1.0e-10;
const EPS_MAJOR_RADIUS: f64 = 1.0e-10;
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::BTreeSet;

fn push_saved_spline_loss(
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    message: impl std::fmt::Display,
) {
    let message = format!("{message}");
    losses.push(crate::loss::CreoLossCode::SectionSplineUnresolved.note(message));
}

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
    let transform = transform?;
    placed_section_geometry_curve(transform, geometry)?;
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
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
            let Some(nurbs) = saved_spline_nurbs(ctx, spline, &mut refusal)? else {
                let records = refusal.take_records();
                if records.is_empty() {
                    push_saved_spline_loss(
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve.",
                            spline.offset
                        ),
                    );
                } else {
                    push_saved_spline_loss(
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve: {}",
                            spline.offset,
                            JoinedLaneRecords(&records)
                        ),
                    );
                }
                continue;
            };
            let suffix = if let Some(entity_id) = spline.entity_id {
                format!("{entity_id}")
            } else {
                format!("offset{}", spline.offset)
            };
            let curve_id = crate::identity::compose_checked::<CurveId>(
                &crate::identity::FEATDEFS_SAVED_SPLINE_CURVE,
                format_args!("{}:{suffix}", definition.identity.id()),
            )?;
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
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "placed_saved_interpolation_spline",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    parameter_range: None,
                    id: curve_id,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(placed)),
                    source_object: Some(SourceObjectAssociation {
                        geometry_role: None,
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(format_args!(
                            "FeatDefs:saved_spline#{suffix}"
                        ))?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
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
    let mut control_points = Vec::new();
    let mut weights = Vec::new();
    for index in 0..directrix.pole_count() {
        let point = directrix.pole_rows().point_at(index)?;
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
            .pole_rows()
            .weight_at(index)
            .map_or(1.0, |weight| weight);
        let mut point_row = Vec::new();
        let mut weight_row = Vec::new();
        for ([radial_scale, tangent_scale], angular_weight) in
            angular_poles.into_iter().zip(angular_weights)
        {
            point_row.push(Point3::new(
                center[0] + radial_scale * radial[0] + tangent_scale * tangent[0],
                center[1] + radial_scale * radial[1] + tangent_scale * tangent[1],
                center[2] + radial_scale * radial[2] + tangent_scale * tangent[2],
            ));
            weight_row.push(directrix_weight * angular_weight);
        }
        control_points.push(point_row);
        weights.push(weight_row);
    }
    let mut u_knots = Vec::new();
    u_knots.extend_from_slice(directrix.knots().as_slice());
    let angular_knots = [
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
    ];
    let mut v_knots = Vec::new();
    v_knots.extend(angular_knots);
    match NurbsSurface::from_lanes(
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(directrix.degree(), u_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(2, v_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, Some(weights)),
        false,
    ) {
        Ok(surface) => Some(surface),
        Err(error) => {
            refusal.note(
                format_args!("creo revolved NURBS surface record for {record}"),
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let schema_classes = crate::decode::sketch_transfer::recipe::FeatureSchemaClasses::new(scan);
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
        if !feature_allows_linear_extrusion_with(scan, &schema_classes, feature_id) {
            continue;
        }
        let Some(order_table) = &definition.order_table else {
            continue;
        };
        let points = resolved_section_points(ctx, definition)?;
        let solved = extrusion_solved_segment_ids(definition);
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
            let id = crate::identity::compose_checked::<SurfaceId>(
                &crate::identity::VISIBGEOM_SURFACE,
                surface_id,
            )?;
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(segment.offset),
                "protextrude_section_carrier",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        geometry_role: None,
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(format_args!(
                            "VisibGeom:{surface_id}"
                        ))?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
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
            let id = crate::identity::compose_checked::<SurfaceId>(
                &crate::identity::VISIBGEOM_SURFACE,
                native_surface_id,
            )?;
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(offset),
                "protextrude_saved_section_carrier",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        geometry_role: None,
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(format_args!(
                            "VisibGeom:{native_surface_id}"
                        ))?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
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
            });
        let Some(span) =
            resolved_feature_extrusion_span(scan, ir, source_carriers, definition, transform)?
        else {
            continue;
        };
        let lower_translation = transform.normal().map(|value| value * span.lower());
        let sweep = transform
            .normal()
            .map(|value| value * (span.upper() - span.lower()));
        for (native_surface_id, internal_id, spline) in splines {
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let Some(section_curve) = saved_spline_nurbs(ctx, spline, &mut refusal)? else {
                let records = refusal.take_records();
                if records.is_empty() {
                    push_saved_spline_loss(
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve.",
                            spline.offset
                        ),
                    );
                } else {
                    push_saved_spline_loss(
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve: {}",
                            spline.offset,
                            JoinedLaneRecords(&records)
                        ),
                    );
                }
                continue;
            };
            let Some(placed) = placed_section_nurbs(transform, &section_curve) else {
                continue;
            };
            let Some(directrix) = translated_nurbs_curve(&placed, lower_translation) else {
                continue;
            };
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let Some(surface) = extruded_nurbs_surface(
                &directrix,
                sweep,
                &format_args!(
                    "surface {native_surface_id} from saved-spline entity {internal_id} at offset {}",
                    spline.offset
                ),
                &mut refusal,
            )
            else {
                for record in refusal.take_records() {
                    push_saved_spline_loss(losses, format_args!(
                        "Extruded section spline at offset {} states no surface carrier: {record}",
                        spline.offset
                    ));
                }
                continue;
            };
            let directrix_range = directrix
                .knots()
                .first()
                .zip(directrix.knots().last())
                .map(|(lower, upper)| (*lower, *upper));
            let curve_id = crate::identity::compose_checked::<CurveId>(
                &crate::identity::FEATURE_EXTRUSION_DIRECTRIX,
                format_args!("{feature_id}:{internal_id}"),
            )?;
            if !ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                annotate(
                    annotations,
                    &curve_id,
                    "FeatDefs",
                    cadmpeg_core::decode::u64_from_index(spline.offset),
                    "protextrude_spline_directrix",
                    Exactness::Derived,
                )?;
                ctx.charge_entities(1, "admit Creo model curves")?;
                source_carriers.admit_curve(
                    ctx,
                    ir,
                    Curve {
                        parameter_range: None,
                        id: curve_id.clone(),
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix)),
                        source_object: Some(SourceObjectAssociation {
                            geometry_role: None,
                            format: cadmpeg_ir::CodecFormat::Creo,
                            object_id: crate::identity::source_object_id_checked(format_args!(
                                "FeatDefs:saved_spline#{internal_id}"
                            ))?,
                            name: None,
                            color: None,
                            visible: None,
                            layer: None,
                            instance_path: Vec::new(),
                        }),
                    },
                )?;
            }
            let surface_id = crate::identity::compose_checked::<SurfaceId>(
                &crate::identity::VISIBGEOM_SURFACE,
                native_surface_id,
            )?;
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            let procedural_id = crate::identity::compose_checked::<ProceduralSurfaceId>(
                &crate::identity::FEATURE_EXTRUSION_CONSTRUCTION,
                format_args!("{feature_id}:{internal_id}"),
            )?;
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "protextrude_spline_surface",
                Exactness::Derived,
            )?;
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "protextrude_spline_surface_construction",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: surface_id.clone(),
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
                    source_object: Some(SourceObjectAssociation {
                        geometry_role: None,
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(format_args!(
                            "VisibGeom:{native_surface_id}"
                        ))?,
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                },
            )?;
            let Some((lower_knot, upper_knot)) = directrix_range else {
                push_saved_spline_loss(
                    losses,
                    format_args!(
                    "Extrusion directrix for feature {feature_id} at offset {} has no knot range",
                    spline.offset
                ),
                );
                continue;
            };
            source_carriers.admit_procedural_surface(
                ir,
                &surface_id,
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
            )?;
            transferred += 1;
        }
    }
    Ok(transferred)
}

fn extrusion_solved_segment_ids(
    definition: &crate::feature::definitions::FeatureDefinition,
) -> BTreeSet<u32> {
    let mut solved = BTreeSet::new();
    for id in definition
        .trim_entities
        .iter()
        .flat_map(|trim_entities| &trim_entities.rows)
        .filter_map(|row| trim_segment_id(definition, row))
    {
        solved.insert(id);
    }
    solved
}

const EPS_ENVELOPE_AGREEMENT: f64 = 1.0e-9;

fn envelope(body: &[u8]) -> Option<[f64; 10]> {
    let cache = scalar::ScalarCache::default();
    let mut values = [0.0; 10];
    let mut slot = 0usize;
    let mut cursor = 0;
    let mut previous = None;
    let mut angular = false;
    while cursor < body.len() {
        let token = *body.get(cursor)?;
        angular |= token != 0x12 && scalar::sweep::angle(token).is_some();
        let (value, repeat, end) = scalar::sweep::run(body, cursor, previous, &cache)?;
        value.is_finite().then_some(())?;
        let next = slot.checked_add(repeat)?;
        values.get_mut(slot..next)?.fill(value);
        slot = next;
        cursor = end;
        previous = Some(value);
    }
    (angular && slot == values.len()).then_some(values)
}

fn axis_permutation(geometry: &SurfaceGeometry) -> bool {
    let (axis, reference) = match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(surface)) => (
            *surface.frame().axis().as_raw(),
            *surface.frame().reference().as_raw(),
        ),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(surface))
            if surface.major_radius().get() > surface.minor_radius().get().abs() =>
        {
            (
                *surface.frame().axis().as_raw(),
                *surface.frame().reference().as_raw(),
            )
        }
        _ => return false,
    };
    [axis, reference].into_iter().all(|direction| {
        [direction.x, direction.y, direction.z]
            .into_iter()
            .filter(|value| value.abs() == 1.0)
            .count()
            == 1
            && [direction.x, direction.y, direction.z]
                .into_iter()
                .all(|value| matches!(value, -1.0 | 0.0 | 1.0))
    })
}

fn parameters(bounds: [f64; 2], angular: bool) -> Option<Vec<f64>> {
    let [start, end] = bounds;
    (start <= end).then_some(())?;
    let mut result = vec![start, end];
    if angular {
        (end - start <= std::f64::consts::TAU).then_some(())?;
        for angle in [
            -std::f64::consts::TAU,
            -3.0 * std::f64::consts::FRAC_PI_2,
            -std::f64::consts::PI,
            -std::f64::consts::FRAC_PI_2,
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
            std::f64::consts::TAU,
        ] {
            if (start..=end).contains(&angle) {
                result.push(angle);
            }
        }
        (start >= -std::f64::consts::TAU && end <= std::f64::consts::TAU).then_some(())?;
    }
    Some(result)
}

fn envelope_matches(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    geometry: &SurfaceGeometry,
) -> Result<bool, CodecError> {
    if !axis_permutation(geometry) {
        return Ok(false);
    }
    let Some(values) = envelope(body) else {
        return Ok(false);
    };
    let torus = matches!(
        geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
    );
    let Some(u) = parameters([values[0], values[2]], true) else {
        return Ok(false);
    };
    let Some(v) = parameters([values[1], values[3]], torus) else {
        return Ok(false);
    };
    let mut minimum = [f64::INFINITY; 3];
    let mut maximum = [f64::NEG_INFINITY; 3];
    for u in u {
        for v in &v {
            let Some(point) = cadmpeg_ir::eval::finite_or_refusal(
                cadmpeg_ir::eval::decode::surface_point(ctx, geometry, u, *v),
            )?
            else {
                return Ok(false);
            };
            for (axis, coordinate) in [point.get().x, point.get().y, point.get().z]
                .into_iter()
                .enumerate()
            {
                minimum[axis] = minimum[axis].min(coordinate);
                maximum[axis] = maximum[axis].max(coordinate);
            }
        }
    }
    let scale = values.into_iter().map(f64::abs).fold(1.0, f64::max);
    Ok(minimum
        .into_iter()
        .chain(maximum)
        .zip(values[4..].iter())
        .all(|(actual, stored)| (actual - stored).abs() <= EPS_ENVELOPE_AGREEMENT * scale))
}

fn sweep_envelope_replay_matches(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    geometry: &SurfaceGeometry,
    surface_id: u32,
    offset: usize,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<bool, CodecError> {
    let admitted = envelope_matches(ctx, body, geometry)?;
    if !admitted {
        losses.push(CreoLossCode::SweepEnvelopeReplayUnresolved.note(format!("Surface {surface_id} at byte {offset} has an angular envelope that conflicts with its first-instance carrier")));
    }
    Ok(admitted)
}

pub(in crate::decode) fn transfer_envelope_replay_surfaces(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, CodecError> {
    if scan.framing.layout != crate::container::Layout::Nd {
        return Ok(0);
    }
    let schema_classes = crate::decode::sketch_transfer::recipe::FeatureSchemaClasses::new(scan);
    let associations = unique_surface_prototype_associations(scan)?;
    let mut transferred = 0;
    for row in &scan.surfaces.rows {
        if schema_classes.get(row.feature_id)
            != Some(crate::feature::schema::SchemaClass::Protrusion)
        {
            continue;
        }
        let mut records = scan
            .surfaces
            .parameters
            .iter()
            .filter(|record| record.surface_id == row.id);
        let (Some(record), None) = (records.next(), records.next()) else {
            continue;
        };
        if envelope(&record.body).is_none() {
            continue;
        }
        if !matches!(
            row.kind,
            crate::surface::SurfaceKind::Cylinder | crate::surface::SurfaceKind::TorusOrSphere
        ) {
            continue;
        }
        let mut candidates = Vec::new();
        for (prototype, first, section) in &associations {
            if first.feature_id != row.feature_id
                || first.kind != row.kind
                || first.offset >= row.offset
                || prototype.record().offset >= row.offset
            {
                continue;
            }
            let Some((start, end)) =
                surface_prototype_frame_bounds(scan, section, prototype.record().offset)?
            else {
                continue;
            };
            if !(start..end).contains(&row.offset) {
                continue;
            }
            candidates.push((first, section));
        }
        let id = crate::identity::compose_checked::<SurfaceId>(
            &crate::identity::VISIBGEOM_SURFACE,
            row.id,
        )?;
        if ir.model.surfaces.iter().any(|surface| surface.id == id) {
            continue;
        }
        let [(first, section)] = candidates.as_slice() else {
            if !candidates.is_empty() {
                losses.push(CreoLossCode::SweepEnvelopeReplayUnresolved.note(format!(
                    "Surface {} at byte {} has {} qualifying first-instance prototypes",
                    row.id,
                    row.offset,
                    candidates.len()
                )));
            }
            continue;
        };
        let first_id = crate::identity::compose_checked::<SurfaceId>(
            &crate::identity::VISIBGEOM_SURFACE,
            first.id,
        )?;
        let Some(first_surface) = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == first_id)
        else {
            losses.push(CreoLossCode::SweepEnvelopeReplayUnresolved.note(format!(
                "Surface {} at byte {} has no resolved first-instance carrier for surface {}",
                row.id, row.offset, first.id
            )));
            continue;
        };
        let geometry = source_carriers.surface_geometry(first_surface).clone();
        if !sweep_envelope_replay_matches(ctx, &record.body, &geometry, row.id, row.offset, losses)?
        {
            continue;
        }
        annotate(
            annotations,
            &id,
            section.name(),
            cadmpeg_core::decode::u64_from_index(row.offset),
            "angular_envelope_carrier_replay",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo sweep envelope replay surfaces")?;
        source_carriers.admit_surface(
            ctx,
            ir,
            Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    geometry_role: None,
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: crate::identity::source_object_id_checked(format_args!(
                        "{}:{}",
                        section.name(),
                        row.id
                    ))?,
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            },
        )?;
        transferred += 1;
    }
    if transferred != 0 {
        losses.push(CreoLossCode::CarrierSweepEnvelopeReplays.note(format!(
            "Transferred {transferred} analytic surfaces from complete angular-envelope replays."
        )));
    }
    Ok(transferred)
}

const EPS_SAMPLE_POINT: f64 = 1.0e-9;
const EPS_SAMPLE_PARAMETER: f64 = 1.0e-9;
const EPS_SAMPLE_ANGLE: f64 = 1.0e-9;

fn points_agree(left: [f64; 3], right: [f64; 3]) -> bool {
    let scale = left
        .into_iter()
        .chain(right)
        .map(f64::abs)
        .fold(1., f64::max);
    left.into_iter()
        .zip(right)
        .all(|(a, b)| a.is_finite() && b.is_finite() && (a - b).abs() <= EPS_SAMPLE_POINT * scale)
}

fn circle_agrees(left: &CurveGeometry, right: &CurveGeometry) -> bool {
    let (
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(left)),
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(right)),
    ) = (left, right)
    else {
        return false;
    };
    let a = left.frame().axis().as_raw();
    let b = right.frame().axis().as_raw();
    points_agree(left.center().get().into(), right.center().get().into())
        && (left.radius().get() - right.radius().get()).abs()
            <= EPS_SAMPLE_POINT * left.radius().get().max(1.)
        && (a.x * b.x + a.y * b.y + a.z * b.z - 1.).abs() <= EPS_SAMPLE_POINT
}

fn owner_is_protrusion(scan: &ContainerScan, curve_id: u32) -> bool {
    let classes = crate::decode::sketch_transfer::recipe::FeatureSchemaClasses::new(scan);
    let mut rows = scan
        .curves
        .topology_rows
        .iter()
        .filter(|row| row.id == curve_id);
    let (Some(row), None) = (rows.next(), rows.next()) else {
        return false;
    };
    classes.get(row.feature_id) == Some(crate::feature::schema::SchemaClass::Protrusion)
}

fn paired_points(
    ctx: &DecodeContext<'_>,
    record: &TwoChartPcurveSamples,
    surfaces: &[Surface],
    source_carriers: &SourceUnitCarriers,
) -> Result<Option<Vec<[f64; 3]>>, CodecError> {
    if record.samples.len() < 2 {
        return Ok(None);
    }
    let index = ModelSurfaceIndex::new(surfaces);
    let [Some(first), Some(second)] = record.faces.map(|face| index.unique(face)) else {
        return Ok(None);
    };
    let mut points = ctx.alloc_filled(
        record.samples.len(),
        [0.; 3],
        "map Creo sweep chart samples",
    )?;
    for (sample, point) in record.samples.iter().zip(&mut points) {
        let mut pair = [[0.; 3]; 2];
        for ((surface, uv), output) in [first, second].into_iter().zip(sample).zip(&mut pair) {
            let Some(mapped) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::surface_point(
                    ctx,
                    source_carriers.surface_geometry(surface),
                    uv[0],
                    uv[1],
                ))?
            else {
                return Ok(None);
            };
            *output = [mapped.get().x, mapped.get().y, mapped.get().z];
        }
        if !points_agree(pair[0], pair[1]) {
            return Ok(None);
        }
        *point = pair[0];
    }
    Ok(Some(points))
}

fn admit_angular_chart_pair(
    ctx: &DecodeContext<'_>,
    record: &TwoChartPcurveSamples,
    surfaces: &[Surface],
    source_carriers: &SourceUnitCarriers,
    unresolved_planes: &BTreeSet<u32>,
    losses: Option<&mut Vec<cadmpeg_ir::report::loss::LossNote>>,
) -> Result<bool, CodecError> {
    let admitted = if record
        .faces
        .iter()
        .any(|face| unresolved_planes.contains(face))
    {
        false
    } else {
        paired_points(ctx, record, surfaces, source_carriers)?.is_some()
    };
    if !admitted {
        if let Some(losses) = losses {
            losses.push(CreoLossCode::AngularPcurvePairUnresolved.note(format!("Curve {} at byte {} has no complete agreeing pair of admitted angular charts; its model constraints are withheld",record.curve_id,record.offset)));
        }
    }
    Ok(admitted)
}

fn endpoints_on_incident_carriers(
    points: [[f64; 3]; 2],
    carriers: &[Vec<crate::decode::analytic::equations::CarrierEquation>; 2],
) -> bool {
    points.into_iter().zip(carriers).all(|(point, incident)| {
        incident
            .iter()
            .all(|carrier| crate::decode::analytic::planes::point_on_carrier(point, *carrier))
    })
}

fn angular_endpoints_agree_with_topology(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    record: &TwoChartPcurveSamples,
    surfaces: &[Surface],
    source_carriers: &SourceUnitCarriers,
) -> Result<bool, CodecError> {
    let mut rows = scan
        .curves
        .topology_rows
        .iter()
        .filter(|row| row.id == record.curve_id);
    let (Some(row), None) = (rows.next(), rows.next()) else {
        return Ok(false);
    };
    let Some(points) = paired_points(ctx, record, surfaces, source_carriers)? else {
        return Ok(false);
    };
    let Some(points) = crate::decode::analytic::pcurves::directed_pcurve_points(
        row.directions,
        [points[0], points[points.len() - 1]],
    ) else {
        return Ok(false);
    };
    let vertices =
        crate::topology::edge_start_vertex_pairs(&scan.topology.half_edge_vertex_incidence);
    let Some(vertices) = vertices.get(&record.curve_id) else {
        return Ok(false);
    };
    let incident =
        crate::topology::vertex_incident_faces(&scan.topology.vertices, &scan.topology.half_edges);
    let index = ModelSurfaceIndex::new(surfaces);
    let carriers = vertices.map(|vertex| {
        incident
            .get(&vertex)
            .into_iter()
            .flatten()
            .filter_map(|face| {
                let surface = index.unique(*face)?;
                crate::decode::analytic::carriers::surface_carrier(
                    source_carriers.surface_geometry(surface),
                )
            })
            .collect()
    });
    Ok(endpoints_on_incident_carriers(points, &carriers))
}

pub(in crate::decode) fn admit_angular_constraints(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    record: &TwoChartPcurveSamples,
    surfaces: &[Surface],
    source_carriers: &SourceUnitCarriers,
    mut losses: Option<&mut Vec<cadmpeg_ir::report::loss::LossNote>>,
) -> Result<bool, CodecError> {
    let unresolved = unresolved_plane_chart_ids(scan);
    if !admit_angular_chart_pair(
        ctx,
        record,
        surfaces,
        source_carriers,
        &unresolved,
        losses.as_deref_mut(),
    )? {
        return Ok(false);
    }
    let admitted =
        angular_endpoints_agree_with_topology(ctx, scan, record, surfaces, source_carriers)?;
    if !admitted {
        if let Some(losses) = losses {
            losses.push(CreoLossCode::AngularPcurvePairUnresolved.note(format!("Curve {} at byte {} has no unique directed angular endpoint binding compatible with every admitted incident analytic carrier; its model constraints are withheld", record.curve_id, record.offset)));
        }
    }
    Ok(admitted)
}

pub(in crate::decode) fn orient_sampled_circle(
    geometry: &CurveGeometry,
    samples: &[[f64; 3]],
    endpoints: [[f64; 3]; 2],
) -> Option<(CurveGeometry, [f64; 2])> {
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = geometry else {
        return None;
    };
    let first = *samples.first()?;
    let last = *samples.last()?;
    let forward = points_agree(first, endpoints[0]) && points_agree(last, endpoints[1]);
    let reversed = points_agree(first, endpoints[1]) && points_agree(last, endpoints[0]);
    if !forward && !reversed {
        return None;
    }
    if samples.len() < 3
        || samples
            .iter()
            .any(|point| !curve_contains_points(geometry, [*point; 2]))
    {
        return None;
    }
    let center: [f64; 3] = circle.center().get().into();
    let reference = normalize(std::array::from_fn(|i| endpoints[0][i] - center[i]))?;
    let original_axis = *circle.frame().axis().as_raw();
    let original_axis = [original_axis.x, original_axis.y, original_axis.z];
    let closed = points_agree(endpoints[0], endpoints[1]);
    let mut selected = None;
    for sign in [1., -1.] {
        let axis = original_axis.map(|v| v * sign);
        let transverse = cross(axis, reference);
        let angle = |point: [f64; 3]| {
            let relative = std::array::from_fn(|i| point[i] - center[i]);
            dot(relative, transverse)
                .atan2(dot(relative, reference))
                .rem_euclid(std::f64::consts::TAU)
        };
        let span = if closed {
            std::f64::consts::TAU
        } else {
            angle(endpoints[1])
        };
        if span <= EPS_SAMPLE_ANGLE {
            continue;
        }
        let mut previous = 0.;
        let mut valid = true;
        for step in 1..samples.len() - 1 {
            let index = if forward {
                step
            } else {
                samples.len() - 1 - step
            };
            let current = angle(samples[index]);
            if current <= previous + EPS_SAMPLE_ANGLE || current >= span - EPS_SAMPLE_ANGLE {
                valid = false;
                break;
            }
            previous = current;
        }
        if !valid {
            continue;
        }
        if selected.is_some() {
            return None;
        }
        let oriented = CircleCurve::try_new(
            Point3::from(center),
            Vector3::from(axis),
            Vector3::from(reference),
            circle.radius().get(),
        )
        .ok()?;
        selected = Some((
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(oriented)),
            [0., span],
        ));
    }
    selected
}

fn circle_on_surface(circle: &CircleCurve, surface: &SurfaceGeometry) -> bool {
    let center: [f64; 3] = circle.center().get().into();
    let axis = *circle.frame().axis().as_raw();
    let axis = [axis.x, axis.y, axis.z];
    let radius = circle.radius().get();
    let vector = |v: &Vector3| [v.x, v.y, v.z];
    let length = |v: [f64; 3]| v[0].hypot(v[1]).hypot(v[2]);
    let relative = |origin: Point3| {
        let origin: [f64; 3] = origin.into();
        let offset: [f64; 3] = std::array::from_fn(|i| center[i] - origin[i]);
        offset.into_iter().all(f64::is_finite).then_some(offset)
    };
    let parallel = |other: [f64; 3]| length(cross(axis, other)) <= EPS_SAMPLE_ANGLE;
    let tolerance = |offset: [f64; 3], scale: f64| {
        EPS_SAMPLE_POINT
            * offset
                .into_iter()
                .map(f64::abs)
                .fold(radius.max(scale).max(1.), f64::max)
    };
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) => {
            let normal = vector(plane.frame().axis().as_raw());
            let Some(offset) = relative(plane.origin().get()) else {
                return false;
            };
            parallel(normal) && dot(offset, normal).abs() <= tolerance(offset, 0.)
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) => {
            let other_axis = vector(cylinder.frame().axis().as_raw());
            let Some(offset) = relative(cylinder.origin().get()) else {
                return false;
            };
            let tolerance = tolerance(offset, cylinder.radius().get());
            parallel(other_axis)
                && length(cross(offset, other_axis)) <= tolerance
                && (radius - cylinder.radius().get()).abs() <= tolerance
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) => {
            let other_axis = vector(torus.frame().axis().as_raw());
            let Some(offset) = relative(torus.center().get()) else {
                return false;
            };
            let major = torus.major_radius().get();
            let minor = torus.minor_radius().get().abs();
            let tolerance = tolerance(offset, major.max(minor));
            let height = dot(offset, other_axis);
            if parallel(other_axis) {
                length(cross(offset, other_axis)) <= tolerance
                    && ((radius - major).hypot(height) - minor).abs() <= tolerance
            } else {
                dot(axis, other_axis).abs() <= EPS_SAMPLE_ANGLE
                    && height.abs() <= tolerance
                    && (length(offset) - major).abs() <= tolerance
                    && dot(offset, axis).abs() <= tolerance
                    && (radius - minor).abs() <= tolerance
            }
        }
        _ => false,
    }
}

fn chart_circle(
    ctx: &DecodeContext<'_>,
    record: &TwoChartPcurveSamples,
    path: usize,
    surface: &SurfaceGeometry,
) -> Result<Option<CurveGeometry>, CodecError> {
    if record.samples.len() < 3
        || !matches!(
            surface,
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Torus(_)
            )
        )
    {
        return Ok(None);
    }
    let mut endpoints = [
        record.samples[0][path],
        record.samples[record.samples.len() - 1][path],
    ];
    let constant = (0..2).find(|coordinate| {
        record.samples.iter().all(|sample| {
            (sample[path][*coordinate] - endpoints[0][*coordinate]).abs() <= EPS_SAMPLE_PARAMETER
        })
    });
    let Some(constant) = constant else {
        return Ok(None);
    };
    endpoints[1][constant] = endpoints[0][constant];
    Ok(
        linear_pcurve_carrier(ctx, surface, endpoints)?.filter(|candidate| {
            matches!(
                candidate,
                CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
            )
        }),
    )
}

fn sampled_carrier(
    ctx: &DecodeContext<'_>,
    record: &TwoChartPcurveSamples,
    surfaces: &[Surface],
    source_carriers: &SourceUnitCarriers,
) -> Result<Option<CurveGeometry>, CodecError> {
    let Some(points) = paired_points(ctx, record, surfaces, source_carriers)? else {
        return Ok(None);
    };
    let index = ModelSurfaceIndex::new(surfaces);
    let mut selected = None;
    for (path, face) in record.faces.into_iter().enumerate() {
        let Some(surface) = index.unique(face) else {
            return Ok(None);
        };
        let Some(candidate) =
            chart_circle(ctx, record, path, source_carriers.surface_geometry(surface))?
        else {
            continue;
        };
        let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &candidate else {
            continue;
        };
        if !record.faces.into_iter().all(|face| {
            index.unique(face).is_some_and(|surface| {
                circle_on_surface(circle, source_carriers.surface_geometry(surface))
            })
        }) {
            return Ok(None);
        }
        if points
            .iter()
            .any(|point| !curve_contains_points(&candidate, [*point; 2]))
        {
            return Ok(None);
        }
        let Some((candidate, _)) =
            orient_sampled_circle(&candidate, &points, [points[0], points[points.len() - 1]])
        else {
            return Ok(None);
        };
        if selected
            .as_ref()
            .is_some_and(|previous| !circle_agrees(previous, &candidate))
        {
            return Ok(None);
        }
        selected = Some(candidate);
    }
    Ok(selected)
}

pub(in crate::decode) fn transfer_sampled_sweep_circles(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, CodecError> {
    let mut transferred = 0;
    for record in &scan.curves.two_chart_pcurves {
        if !scan
            .curves
            .angular_two_chart_curve_ids
            .contains(&record.curve_id)
            || !admit_angular_constraints(
                ctx,
                scan,
                record,
                &ir.model.surfaces,
                source_carriers,
                None,
            )?
            || !owner_is_protrusion(scan, record.curve_id)
        {
            continue;
        }
        if scan
            .curves
            .two_chart_pcurves
            .iter()
            .filter(|candidate| candidate.curve_id == record.curve_id)
            .count()
            != 1
        {
            losses.push(CreoLossCode::SampledPcurveCarrierUnresolved.note(format!("Curve {} at byte {} has multiple complete sweep sample paths; its circular construction remains native",record.curve_id,record.offset)));
            continue;
        }
        let id = crate::identity::compose_checked::<CurveId>(
            &crate::identity::VISIBGEOM_CURVE,
            record.curve_id,
        )?;
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        let index = ModelSurfaceIndex::new(&ir.model.surfaces);
        let mut circular = false;
        for (path, face) in record.faces.into_iter().enumerate() {
            if let Some(surface) = index.unique(face) {
                circular |=
                    chart_circle(ctx, record, path, source_carriers.surface_geometry(surface))?
                        .is_some();
            }
        }
        if !circular {
            continue;
        }
        let Some(geometry) = sampled_carrier(ctx, record, &ir.model.surfaces, source_carriers)?
        else {
            losses.push(CreoLossCode::SampledPcurveCarrierUnresolved.note(format!("Curve {} at byte {} has no unique circular sweep construction from its complete paired charts",record.curve_id,record.offset)));
            continue;
        };
        annotate(
            annotations,
            &id,
            "VisibGeom",
            cadmpeg_core::decode::u64_from_index(record.offset),
            "sampled_sweep_circle",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo sampled sweep circles")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id,
                geometry,
                parameter_range: None,
                source_object: None,
            },
        )?;
        transferred += 1;
    }
    if transferred != 0 {
        losses.push(CreoLossCode::CarrierSampledSweepCircles.note(format!(
            "Transferred {} circular boundaries from complete paired sweep charts.",
            transferred
        )));
    }
    Ok(transferred)
}

#[derive(Clone, Copy)]
pub(in crate::decode) enum SampledCircleRange {
    Unrelated,
    Refused,
    Admitted([f64; 2]),
}

pub(in crate::decode) fn sampled_sweep_circle_range(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    geometry: &mut CurveGeometry,
    incidence: (u32, [u32; 2]),
    surfaces: &[Surface],
    endpoints: [[f64; 3]; 2],
    source_carriers: &SourceUnitCarriers,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<SampledCircleRange, CodecError> {
    if !scan
        .curves
        .angular_two_chart_curve_ids
        .contains(&incidence.0)
        || !owner_is_protrusion(scan, incidence.0)
        || !matches!(
            geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
        )
    {
        return Ok(SampledCircleRange::Unrelated);
    }
    let Some(record) = scan
        .curves
        .two_chart_pcurves
        .iter()
        .find(|record| record.curve_id == incidence.0 && record.samples.len() >= 3)
    else {
        return Ok(SampledCircleRange::Unrelated);
    };
    if !admit_angular_constraints(ctx, scan, record, surfaces, source_carriers, None)? {
        return Ok(SampledCircleRange::Unrelated);
    }
    if sampled_carrier(ctx, record, surfaces, source_carriers)?.is_none() {
        return Ok(SampledCircleRange::Unrelated);
    }
    match sampled_sweep_circle_range_impl(
        ctx,
        scan,
        geometry,
        incidence,
        surfaces,
        endpoints,
        source_carriers,
    )? {
        Some(range) => Ok(SampledCircleRange::Admitted(range)),
        None => {
            losses.push(CreoLossCode::SampledPcurveCarrierUnresolved.note(format!("Curve {} at byte {} has no unique ordered circular sweep interval; its interval remains native",record.curve_id,record.offset)));
            Ok(SampledCircleRange::Refused)
        }
    }
}

fn sampled_sweep_circle_range_impl(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    geometry: &mut CurveGeometry,
    incidence: (u32, [u32; 2]),
    surfaces: &[Surface],
    endpoints: [[f64; 3]; 2],
    source_carriers: &SourceUnitCarriers,
) -> Result<Option<[f64; 2]>, CodecError> {
    if !owner_is_protrusion(scan, incidence.0)
        || !matches!(
            geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
        )
    {
        return Ok(None);
    }
    if scan
        .curves
        .two_chart_pcurves
        .iter()
        .filter(|record| record.curve_id == incidence.0)
        .count()
        != 1
    {
        return Ok(None);
    }
    let mut selected: Option<(CurveGeometry, [f64; 2])> = None;
    for record in scan
        .curves
        .two_chart_pcurves
        .iter()
        .filter(|record| record.curve_id == incidence.0)
    {
        if record.faces != incidence.1 && record.faces != [incidence.1[1], incidence.1[0]] {
            return Ok(None);
        }
        if sampled_carrier(ctx, record, surfaces, source_carriers)?.is_none() {
            return Ok(None);
        }
        let Some(points) = paired_points(ctx, record, surfaces, source_carriers)? else {
            return Ok(None);
        };
        let Some(candidate) = orient_sampled_circle(geometry, &points, endpoints) else {
            return Ok(None);
        };
        if selected.as_ref().is_some_and(|previous| {
            !circle_agrees(&previous.0, &candidate.0)
                || previous
                    .1
                    .into_iter()
                    .zip(candidate.1)
                    .any(|(a, b)| (a - b).abs() > EPS_SAMPLE_ANGLE)
        }) {
            return Ok(None);
        }
        selected = Some(candidate);
    }
    if let Some((candidate, range)) = selected {
        *geometry = candidate;
        Ok(Some(range))
    } else {
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
