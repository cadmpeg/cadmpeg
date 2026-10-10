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
use super::super::sketch::radii::trim_segment_ids;
use super::super::sketch::skamp::complete_section_segment_rows;
use super::super::sketch_ids::sketch_section_curve_id_admitted;
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
use crate::decode::source_carriers::SourceUnitCarriers;
use crate::lane_refusal::JoinedLaneRecords;
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
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::sketches::{SketchGeometry, SketchGeometryDefinition, SketchId};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::BTreeSet;

fn push_saved_spline_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    message: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let message = ctx.format_retained(format_args!("{message}"), "creo saved spline loss text")?;
    ctx.reserve_vec(losses, 1, "creo saved spline losses")?;
    losses.push(crate::loss::CreoLossCode::SectionSplineUnresolved.note(message));
    Ok(())
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    transform: Option<&crate::placement::FeatureSectionTransform>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
    geometry: &SketchGeometry,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(transform) = transform else {
        return Ok(None);
    };
    if placed_section_geometry_curve(transform, geometry).is_none() {
        return Ok(None);
    }
    sketch_section_curve_id_admitted(ctx, sketch, suffix).map(Some)
}

fn unique_feature_surface_row(
    rows: &crate::surface::SurfaceRows,
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
    let mut curve_id_storage = ctx.reserve_scoped(0, "creo model curve identity index scratch")?;
    let mut curve_id_index = None;
    for transform in ctx.admit_iter(
        &scan.features.section_transforms,
        "creo sweep transform scan",
    )? {
        if unique_feature_section_transform(
            ctx,
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )?
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let entities_owned_storage = semantic_saved_section_entities(ctx, definition)?;
        let _entity_storage = entities_owned_storage.1;
        let entities = entities_owned_storage.0;
        for spline in ctx
            .admit_iter(&entities, "creo saved section spline traversal")?
            .filter_map(|entity| match entity {
                crate::feature::definitions::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let (nurbs, _nurbs_storage) = ctx
                .with_scoped_storage("creo saved spline source curve", || {
                    saved_spline_nurbs(ctx, spline, &mut refusal)
                })?;
            let Some(nurbs) = nurbs else {
                let records = refusal.take_records_checked()?;
                if records.is_empty() {
                    push_saved_spline_loss(
                        ctx,
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve.",
                            spline.offset
                        ),
                    )?;
                } else {
                    push_saved_spline_loss(
                        ctx,
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve: {}",
                            spline.offset,
                            JoinedLaneRecords(&records)
                        ),
                    )?;
                }
                continue;
            };
            let suffix_owned_storage = if let Some(entity_id) = spline.entity_id {
                ctx.format_scoped(
                    format_args!("{entity_id}"),
                    "creo saved spline identity suffix",
                )?
            } else {
                ctx.format_scoped(
                    format_args!("offset{}", spline.offset),
                    "creo saved spline identity suffix",
                )?
            };
            let _suffix_reservation = suffix_owned_storage.1;
            let suffix = suffix_owned_storage.0;
            let (curve_id, candidate_identity_storage) = ctx.with_scoped_storage(
                "creo sweep identity candidate", || crate::identity::compose_checked::<CurveId>(
                ctx,
                &crate::identity::FEATDEFS_SAVED_SPLINE_CURVE,
                format_args!("{}:{suffix}", definition.identity.id()),
                "creo saved spline curve identity",
            ),
            )?;
            if curve_id_index.is_none() {
                curve_id_index = Some(curve_id_storage.with_storage(|| {
                    ctx.collect_string_set(
                        ir.model.curves.iter().map(|record| record.id.as_str()),
                        "creo model curve identity index",
                    )
                })?);
            }
            let Some(indexed_curve_ids) = &mut curve_id_index else {
                continue;
            };
            if ctx.contains_hash_set(
                indexed_curve_ids,
                curve_id.as_str(),
                "creo model identity comparison",
            )? {
                continue;
            }
            let (placed, placed_storage) = ctx.with_scoped_storage(
                "creo saved spline placed curve candidate",
                || placed_section_nurbs(ctx, transform, &nurbs),
            )?;
            let Some(placed) = placed else {
                continue;
            };
            annotate(
                ctx,
                annotations,
                &curve_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "placed_saved_interpolation_spline",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model curves")?;
            curve_id_storage.with_storage(|| {
                ctx.insert_string_set(
                    indexed_curve_ids,
                    curve_id.as_str(),
                    "creo model curve identity index",
                )
            })?;
            source_carriers.admit_curve(
                ctx,
                ir,
                Curve {
                    id: candidate_identity_storage.commit_value(curve_id)?,
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(placed_storage.commit_value(placed)?)),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("FeatDefs:saved_spline#{suffix}"),
                            "creo source object identity",
                        )?,
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    directrix: &NurbsCurve,
    axis: &RevolutionAxis,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Result<Option<NurbsSurface>, cadmpeg_core::CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let Some(axis_direction) = normalize([axis.direction.x, axis.direction.y, axis.direction.z])
    else {
        return Ok(None);
    };
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
    let mut lane_storage = ctx.reserve_scoped(0, "creo revolved raw NURBS lanes")?;
    let mut control_points = Vec::new();
    let mut weights = Vec::new();
    let mut poles = 0..directrix.pole_count();
    while poles.len() > 0 {
        let Some(index) = ctx.next_charged(&mut poles, "creo revolved NURBS pole projection")? else {
            break;
        };
        let Some(point) = directrix.pole_rows().point_at(index) else {
            return Ok(None);
        };
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
        ctx.reserve_scoped_vec(
            &mut lane_storage,
            &mut control_points,
            1,
            "creo revolved NURBS pole rows",
        )?;
        ctx.reserve_scoped_vec(
            &mut lane_storage,
            &mut weights,
            1,
            "creo revolved NURBS weight rows",
        )?;
        let mut point_row = Vec::new();
        let mut weight_row = Vec::new();
        ctx.reserve_scoped_vec(
            &mut lane_storage,
            &mut point_row,
            angular_poles.len(),
            "creo revolved NURBS poles",
        )?;
        ctx.reserve_scoped_vec(
            &mut lane_storage,
            &mut weight_row,
            angular_weights.len(),
            "creo revolved NURBS weights",
        )?;
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
    ctx.reserve_vec(
        &mut u_knots,
        directrix.knots().as_slice().len(),
        "creo revolved NURBS u knots",
    )?;
    u_knots.extend(
        ctx.admit_iter(
            directrix.knots().as_slice(),
            "creo revolved NURBS u knot copy",
        )?
        .copied(),
    );
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
    ctx.reserve_vec(
        &mut v_knots,
        angular_knots.len(),
        "creo revolved NURBS v knots",
    )?;
    v_knots.extend(angular_knots);
    match NurbsSurface::from_lanes(
        ctx,
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(directrix.degree(), u_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(2, v_knots, false),
        cadmpeg_ir::geometry::nurbs::NurbsSurfaceLanes::new(control_points, Some(weights)),
        false,
    )? {
        Ok(surface) => Ok(Some(surface)),
        Err(error) => {
            refusal.note_checked(
                ctx,
                format_args!("creo revolved NURBS surface record for {record}"),
                &error,
            );
            Ok(None)
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
    let mut transferred = 0;
    let mut surface_id_storage =
        ctx.reserve_scoped(0, "creo model surface identity index scratch")?;
    let mut surface_id_index = None;
    let mut curve_id_storage = ctx.reserve_scoped(0, "creo model curve identity index scratch")?;
    let mut curve_id_index = None;
    for transform in ctx.admit_iter(
        &scan.features.section_transforms,
        "creo sweep transform scan",
    )? {
        if unique_feature_section_transform(
            ctx,
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )?
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(ctx, scan, feature_id)? {
            continue;
        }
        let Some(order_table) = &definition.order_table else {
            continue;
        };
        let points_owned_storage = ctx
            .with_scoped_storage("creo extrusion section point scratch", || {
                resolved_section_points(ctx, definition)
            })?;
        let _point_storage = points_owned_storage.1;
        let points = points_owned_storage.0;
        let solved_owned_storage = ctx
            .with_scoped_storage("creo extrusion solved segment scratch", || {
                extrusion_solved_segment_ids(ctx, definition)
            })?;
        let _solved_storage = solved_owned_storage.1;
        let solved = solved_owned_storage.0;
        let segments_owned_storage = ctx
            .with_scoped_storage("creo extrusion section row scratch", || {
                complete_section_segment_rows(ctx, definition)
            })?;
        let _segment_storage = segments_owned_storage.1;
        let segments = segments_owned_storage.0;
        for segment in ctx.admit_iter(&segments, "creo extrusion section segment traversal")? {
            if !ctx.contains_btree_set(
                &solved,
                &segment.external_id,
                "creo extrusion solved segment membership",
            )? {
                continue;
            }
            let Some(section_geometry) =
                resolved_section_segment_geometry(ctx, definition, &points, segment)?
            else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(surface_id) = analytic_surface_id_for_feature(
                ctx,
                &scan.surfaces.rows,
                &scan.features.entity_tables,
                feature_id,
                segment.external_id,
                &geometry,
            )?
            else {
                continue;
            };
            let (id, candidate_identity_storage) = ctx.with_scoped_storage(
                "creo sweep identity candidate", || crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                surface_id,
                "creo extrusion surface identity",
            ),
            )?;
            if surface_id_index.is_none() {
                surface_id_index = Some(surface_id_storage.with_storage(|| {
                    ctx.collect_string_set(
                        ir.model.surfaces.iter().map(|record| record.id.as_str()),
                        "creo model surface identity index",
                    )
                })?);
            }
            let Some(indexed_surface_ids) = &mut surface_id_index else {
                continue;
            };
            if ctx.contains_hash_set(
                indexed_surface_ids,
                id.as_str(),
                "creo model identity comparison",
            )? {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(segment.offset),
                "protextrude_section_carrier",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            surface_id_storage.with_storage(|| {
                ctx.insert_string_set(
                    indexed_surface_ids,
                    id.as_str(),
                    "creo model surface identity index",
                )
            })?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: candidate_identity_storage.commit_value(id)?,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("VisibGeom:{surface_id}"),
                            "creo source object identity",
                        )?,
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

        let entities_owned_storage = semantic_saved_section_entities(ctx, definition)?;
        let _entity_storage = entities_owned_storage.1;
        let entities = entities_owned_storage.0;
        for entity in ctx.admit_iter(&entities, "creo saved section geometry traversal")? {
            let Some((internal_id, section_geometry, offset)) =
                saved_section_entity_geometry(entity)
            else {
                continue;
            };
            let Some(external_id) = order_table.external_id(internal_id) else {
                continue;
            };
            let Some(native_surface_id) = generated_surface_id_for_feature(
                ctx,
                &scan.features.entity_tables,
                feature_id,
                external_id,
            )?
            else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(expected_kind) = surface_kind_for_geometry(ctx, &geometry)? else {
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
            let (id, candidate_identity_storage) = ctx.with_scoped_storage(
                "creo sweep identity candidate", || crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                native_surface_id,
                "creo extrusion surface identity",
            ),
            )?;
            if surface_id_index.is_none() {
                surface_id_index = Some(surface_id_storage.with_storage(|| {
                    ctx.collect_string_set(
                        ir.model.surfaces.iter().map(|record| record.id.as_str()),
                        "creo model surface identity index",
                    )
                })?);
            }
            let Some(indexed_surface_ids) = &mut surface_id_index else {
                continue;
            };
            if ctx.contains_hash_set(
                indexed_surface_ids,
                id.as_str(),
                "creo model identity comparison",
            )? {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(offset),
                "protextrude_saved_section_carrier",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            surface_id_storage.with_storage(|| {
                ctx.insert_string_set(
                    indexed_surface_ids,
                    id.as_str(),
                    "creo model surface identity index",
                )
            })?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: candidate_identity_storage.commit_value(id)?,
                    geometry,
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("VisibGeom:{native_surface_id}"),
                            "creo source object identity",
                        )?,
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

        let Some(span) =
            resolved_feature_extrusion_span(ctx, scan, ir, source_carriers, definition, transform)?
        else {
            continue;
        };
        let lower_translation = transform.normal().map(|value| value * span.lower());
        let sweep = transform
            .normal()
            .map(|value| value * (span.upper() - span.lower()));
        for spline in ctx
            .admit_iter(&entities, "creo saved section spline traversal")?
            .filter_map(|entity| match entity {
                crate::feature::definitions::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let Some(internal_id) = spline.entity_id else {
                continue;
            };
            let Some(external_id) = order_table.external_id(internal_id) else {
                continue;
            };
            let Some(native_surface_id) = generated_surface_id_for_feature(
                ctx,
                &scan.features.entity_tables,
                feature_id,
                external_id,
            )?
            else {
                continue;
            };
            if !unique_feature_surface_row(
                &scan.surfaces.rows,
                native_surface_id,
                feature_id,
                crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
            ) {
                continue;
            }
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let (section_curve, _section_storage) = ctx
                .with_scoped_storage("creo saved extrusion section curve", || {
                    saved_spline_nurbs(ctx, spline, &mut refusal)
                })?;
            let Some(section_curve) = section_curve else {
                let records = refusal.take_records_checked()?;
                if records.is_empty() {
                    push_saved_spline_loss(
                        ctx,
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve.",
                            spline.offset
                        ),
                    )?;
                } else {
                    push_saved_spline_loss(
                        ctx,
                        losses,
                        format_args!(
                            "Saved section spline at offset {} cannot form a NURBS curve: {}",
                            spline.offset,
                            JoinedLaneRecords(&records)
                        ),
                    )?;
                }
                continue;
            };
            let (placed, _placed_storage) = ctx
                .with_scoped_storage("creo saved extrusion placed curve", || {
                    placed_section_nurbs(ctx, transform, &section_curve)
                })?;
            let Some(placed) = placed else {
                continue;
            };
            let (directrix, directrix_storage) = ctx.with_scoped_storage(
                "creo saved extrusion translated curve",
                || translated_nurbs_curve(ctx, &placed, lower_translation),
            )?;
            let Some(directrix) = directrix else {
                continue;
            };
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let (surface, surface_storage) = ctx.with_scoped_storage(
                "creo saved extrusion surface candidate", || extruded_nurbs_surface(
                ctx,
                &directrix,
                sweep,
                &format_args!(
                    "surface {native_surface_id} from saved-spline entity {internal_id} at offset {}",
                    spline.offset
                ),
                &mut refusal,
            ))?;
            let Some(surface) = surface else {
                for record in ctx.admit_iter(refusal.take_records_checked()?, "creo saved spline refusal records")? {
                    push_saved_spline_loss(ctx, losses, format_args!(
                        "Extruded section spline at offset {} states no surface carrier: {record}",
                        spline.offset
                    ))?;
                }
                continue;
            };
            let directrix_range = directrix
                .knots()
                .first()
                .zip(directrix.knots().last())
                .map(|(lower, upper)| (*lower, *upper));
            let (curve_id, candidate_identity_storage) = ctx.with_scoped_storage(
                "creo sweep identity candidate", || crate::identity::compose_checked::<CurveId>(
                ctx,
                &crate::identity::FEATURE_EXTRUSION_DIRECTRIX,
                format_args!("{feature_id}:{internal_id}"),
                "creo extrusion directrix identity",
            ),
            )?;
            if curve_id_index.is_none() {
                curve_id_index = Some(curve_id_storage.with_storage(|| {
                    ctx.collect_string_set(
                        ir.model.curves.iter().map(|record| record.id.as_str()),
                        "creo model curve identity index",
                    )
                })?);
            }
            let Some(indexed_curve_ids) = &mut curve_id_index else {
                continue;
            };
            if !ctx.contains_hash_set(
                indexed_curve_ids,
                curve_id.as_str(),
                "creo model identity comparison",
            )? {
                annotate(
                    ctx,
                    annotations,
                    &curve_id,
                    "FeatDefs",
                    cadmpeg_core::decode::u64_from_index(spline.offset),
                    "protextrude_spline_directrix",
                    Exactness::Derived,
                )?;
                ctx.charge_entities(1, "admit Creo model curves")?;
                curve_id_storage.with_storage(|| {
                    ctx.insert_string_set(
                        indexed_curve_ids,
                        curve_id.as_str(),
                        "creo model curve identity index",
                    )
                })?;
                source_carriers.admit_curve(
                    ctx,
                    ir,
                    Curve {
                        id: curve_id
                            .try_clone_for_decode(ctx, "creo construction curve identity copy")?,
                        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix)),
                        source_object: Some(SourceObjectAssociation {
                            format: cadmpeg_ir::CodecFormat::Creo,
                            object_id: crate::identity::source_object_id_checked(
                                ctx,
                                format_args!("FeatDefs:saved_spline#{internal_id}"),
                                "creo source object identity",
                            )?,
                            name: None,
                            color: None,
                            visible: None,
                            layer: None,
                            instance_path: Vec::new(),
                        }),
                    },
                )?;
                directrix_storage.commit()?;
            } else {
                drop(directrix);
                drop(directrix_storage);
            }
            let (surface_id, _surface_identity_storage) = ctx.with_scoped_storage(
                "creo sweep identity candidate", || crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                native_surface_id,
                "creo extrusion surface identity",
            ),
            )?;
            if surface_id_index.is_none() {
                surface_id_index = Some(surface_id_storage.with_storage(|| {
                    ctx.collect_string_set(
                        ir.model.surfaces.iter().map(|record| record.id.as_str()),
                        "creo model surface identity index",
                    )
                })?);
            }
            let Some(indexed_surface_ids) = &mut surface_id_index else {
                continue;
            };
            if ctx.contains_hash_set(
                indexed_surface_ids,
                surface_id.as_str(),
                "creo model identity comparison",
            )? {
                continue;
            }
            let (procedural_id, procedural_identity_storage) = ctx.with_scoped_storage(
                "creo sweep construction identity candidate", || crate::identity::compose_checked::<ProceduralSurfaceId>(
                ctx,
                &crate::identity::FEATURE_EXTRUSION_CONSTRUCTION,
                format_args!("{feature_id}:{internal_id}"),
                "creo extrusion construction identity",
            ))?;
            annotate(
                ctx,
                annotations,
                &surface_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "protextrude_spline_surface",
                Exactness::Derived,
            )?;
            annotate(
                ctx,
                annotations,
                &procedural_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "protextrude_spline_surface_construction",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            surface_id_storage.with_storage(|| {
                ctx.insert_string_set(
                    indexed_surface_ids,
                    surface_id.as_str(),
                    "creo model surface identity index",
                )
            })?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: surface_id
                        .try_clone_for_decode(ctx, "creo construction surface identity copy")?,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface_storage.commit_value(surface)?)),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("VisibGeom:{native_surface_id}"),
                            "creo source object identity",
                        )?,
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
                    ctx,
                    losses,
                    format_args!(
                    "Extrusion directrix for feature {feature_id} at offset {} has no knot range",
                    spline.offset
                ),
                )?;
                continue;
            };
            let construction = cadmpeg_ir::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(
                    candidate_identity_storage.commit_value(curve_id)?,
                    Some([lower_knot, upper_knot]),
                    Vector3::from(sweep),
                    None,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                ).map_err(cadmpeg_core::CodecError::malformed)?;
            source_carriers.admit_procedural_surface(
                ctx,
                ir,
                &surface_id,
                ProceduralSurface::new(
                    procedural_identity_storage.commit_value(procedural_id)?,
                    ProceduralSurfaceDefinition::Extrusion(construction),
                    None,
                ),
            )?;
            transferred += 1;
        }
    }
    Ok(transferred)
}

fn extrusion_solved_segment_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &crate::feature::definitions::FeatureDefinition,
) -> Result<BTreeSet<u32>, cadmpeg_core::CodecError> {
    let mut solved = BTreeSet::new();
    for id in ctx
        .admit_iter(
            trim_segment_ids(ctx, definition)?,
            "creo extrusion solved trim rows",
        )?
        .flatten()
    {
        ctx.insert_btree_set(&mut solved, id, "creo extrusion solved segment ID nodes")?;
    }
    Ok(solved)
}

#[cfg(test)]
mod tests;
