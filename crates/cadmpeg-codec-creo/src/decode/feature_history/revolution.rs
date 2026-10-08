// SPDX-License-Identifier: Apache-2.0
//! Resolved revolution and extrusion surface and vertex-orbit transfer.

use super::super::native::annotate;
use super::super::sketch::coordinates::resolved_section_points;
use super::super::sketch::geometry::{
    resolved_section_segment_geometry, saved_section_entity_geometry,
};
use super::super::sketch::radii::trim_segment_ids;
use super::super::sketch::skamp::complete_section_segment_rows;
use super::super::sketch_ids::model_sketch_id;
use super::super::sweep::profiles::connected_sketch_profile_vertices;
use super::super::sweep::surfaces::{
    extruded_section_line, revolved_nurbs_surface, revolved_section_circle,
    revolved_section_surface,
};
use super::super::uniqueness::{
    unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::axes::revolution_axis_for_transfer;
use super::draft::feature_allows_linear_extrusion;
use super::link::{
    insert_ordered_family_surface_binding, ordered_analytic_surface_id_for_feature,
    ordered_family_surface_bindings_for_feature, profile_segment_ids, SurfaceBindingSource,
};
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::identity::visit_semantic_saved_section_entities;
use crate::decode::sketch_transfer::recipe::{
    feature_recipe, feature_revolution_extent, unique_feature_revolution_extent,
};
use crate::decode::source_carriers::SourceUnitCarriers;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

fn revolution_unit_axis(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature_id: u32,
    direction: cadmpeg_ir::features::FeatureDirection3,
) -> Result<cadmpeg_ir::units::UnitVector3, cadmpeg_core::CodecError> {
    let Some(axis) = cadmpeg_ir::units::UnitVector3::new(direction.get()) else {
        return Err(cadmpeg_core::CodecError::Malformed(ctx.format_retained(
            format_args!(
                "feature {feature_id} revolution axis direction does not have unit length"
            ),
            "creo revolution axis error text",
        )?));
    };
    Ok(axis)
}

fn directrix_parameter_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    offset: usize,
    knots: &[f64],
) -> Result<[f64; 2], cadmpeg_core::CodecError> {
    let Some((first, last)) = knots.first().zip(knots.last()) else {
        return Err(cadmpeg_core::CodecError::Malformed(ctx.format_retained(
            format_args!("FeatDefs saved spline at offset {offset} has no knots"),
            "creo revolution knot error text",
        )?));
    };
    Ok([*first, *last])
}

fn push_revolution_surface_loss(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    message: impl std::fmt::Display,
) -> Result<(), cadmpeg_core::CodecError> {
    let message = ctx.format_retained(
        format_args!("{message}"),
        "creo revolved saved spline loss text",
    )?;
    ctx.reserve_vec(losses, 1, "creo revolved saved spline losses")?;
    losses.push(crate::loss::CreoLossCode::FeatureSurfaceOperationIncomplete.note(message));
    Ok(())
}

fn insert_generating_segment_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ids: &mut BTreeSet<u32>,
    id: u32,
) -> Result<(), cadmpeg_core::CodecError> {
    ctx.insert_btree_set(ids, id, "creo revolution generating segment IDs")?;
    Ok(())
}

/// Transfer one exact surface carrier per resolved revolution generator.
///
/// A saved spline whose revolved lanes the IR carrier refuses states no
/// surface. The model carries the feature and its directrix curve without that
/// surface, so the refusal is a loss note naming the feature and the spline
/// offset.
pub(in super::super) fn transfer_resolved_revolution_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut transferred = 0;
    for transform in ctx.admit_iter(
        &scan.features.section_transforms,
        "creo revolution surface transforms",
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
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id)
            != Some(crate::feature::operations::FeatureRecipeKind::Revolve)
        {
            continue;
        }
        if unique_feature_revolution_extent(ctx, &scan.features.revolution_extents, feature_id)?
            .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let extent = feature_revolution_extent(ctx, scan, feature_id)?;
        let Some(axis) = revolution_axis_for_transfer(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            (definition, transform),
            extent.as_ref(),
        )?
        else {
            continue;
        };
        let points = resolved_section_points(ctx, definition)?;
        let mut generating_ids = BTreeSet::new();
        for id in ctx
            .admit_iter(
                trim_segment_ids(ctx, definition)?,
                "creo revolution trim rows",
            )?
            .flatten()
        {
            insert_generating_segment_id(ctx, &mut generating_ids, id)?;
        }
        let Some(sketch_id) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let mut matching_sketch = None;
        for sketch in ctx.admit_iter(&ir.model.sketches, "creo revolution model sketches")? {
            if !ctx.equal(
                &sketch.id,
                &sketch_id,
                "creo revolution sketch identity comparison",
            )? {
                continue;
            }
            if matching_sketch.is_some() {
                matching_sketch = None;
                break;
            }
            matching_sketch = Some(sketch);
        }
        if let Some(sketch) = matching_sketch {
            let segments = complete_section_segment_rows(ctx, definition)?;
            let profile_ids =
                profile_segment_ids(ctx, definition.identity.id(), &segments, &sketch.profiles)?;
            for id in ctx.admit_iter(&profile_ids, "creo revolution profile segment IDs")? {
                insert_generating_segment_id(ctx, &mut generating_ids, *id)?;
            }
        }
        let arc_bindings = match definition.order_table.as_ref() {
            None => BTreeMap::new(),
            Some(order) => {
                let arc_segments = complete_section_segment_rows(ctx, definition)?;
                ordered_family_surface_bindings_for_feature(
                    ctx,
                    &scan.surfaces.rows,
                    feature_id,
                    &scan.features.entity_tables,
                    order,
                    ctx.admit_iter(&arc_segments, "creo revolution arc generator segments")?
                        .filter(|segment| {
                            generating_ids.contains(&segment.external_id)
                                && matches!(
                                    segment.kind,
                                    crate::feature::definitions::FeatureSegmentKind::Arc(_)
                                )
                        })
                        .map(|segment| segment.external_id),
                    crate::surface::SurfaceKind::TorusOrSphere,
                )?
            }
        };
        let spline_bindings = match definition.order_table.as_ref() {
            None => BTreeMap::new(),
            Some(order) => {
                let mut bindings = BTreeMap::new();
                let mut bound_surfaces = BTreeSet::new();
                let outcome =
                    visit_semantic_saved_section_entities::<()>(ctx, definition, |entity| {
                        if let crate::feature::definitions::FeatureSavedEntity::Spline(spline) =
                            entity
                        {
                            let external_id = match spline.entity_id {
                                Some(entity_id) => order.external_id(entity_id),
                                None => None,
                            };
                            if let Some(external_id) = external_id {
                                if !insert_ordered_family_surface_binding(
                                    ctx,
                                    &SurfaceBindingSource {
                                        surface_rows: &scan.surfaces.rows,
                                        feature_id,
                                        tables: &scan.features.entity_tables,
                                        order,
                                        expected_kind: crate::surface::SurfaceKind::Spline,
                                    },
                                    external_id,
                                    &mut bindings,
                                    &mut bound_surfaces,
                                )? {
                                    return Ok(std::ops::ControlFlow::Break(()));
                                }
                            }
                        }
                        Ok(std::ops::ControlFlow::Continue(()))
                    })?;
                match outcome {
                    std::ops::ControlFlow::Continue(()) => bindings,
                    std::ops::ControlFlow::Break(()) => BTreeMap::new(),
                }
            }
        };
        let segments = complete_section_segment_rows(ctx, definition)?;
        for segment in ctx
            .admit_iter(&segments, "creo revolution generator segments")?
            .filter(|segment| generating_ids.contains(&segment.external_id))
        {
            let Some(geometry) =
                resolved_section_segment_geometry(ctx, definition, &points, segment)?
            else {
                continue;
            };
            let Some(surface) = revolved_section_surface(transform, &geometry, &axis) else {
                continue;
            };
            let native_surface = match segment.kind {
                crate::feature::definitions::FeatureSegmentKind::Line(_) => {
                    match definition.order_table.as_ref() {
                        Some(order) => ordered_analytic_surface_id_for_feature(
                            ctx,
                            &scan.surfaces.rows,
                            &scan.features.entity_tables,
                            feature_id,
                            order,
                            segment.external_id,
                            &surface,
                        )?,
                        None => None,
                    }
                }
                crate::feature::definitions::FeatureSegmentKind::Arc(_) => {
                    ctx.get_btree_map(&arc_bindings, &segment.external_id, "creo revolution arc binding lookup")?.copied()
                }
                crate::feature::definitions::FeatureSegmentKind::Point(_) => None,
            };
            let surface_id = if let Some(id) = native_surface {
                crate::identity::compose_checked::<SurfaceId>(
                    ctx,
                    &crate::identity::VISIBGEOM_SURFACE,
                    id,
                    "creo revolution surface identity",
                )?
            } else {
                crate::identity::compose_checked::<SurfaceId>(
                    ctx,
                    &crate::identity::FEATURE_REVOLUTION_SURFACE,
                    format_args!("{feature_id}:segment{}", segment.external_id),
                    "creo revolution surface identity",
                )?
            };
            let mut surface_exists = false;
            for item in ctx.admit_iter(&ir.model.surfaces, "creo revolution surface lookup")? {
                if ctx.equal(
                    &item.id,
                    &surface_id,
                    "creo revolution surface identity comparison",
                )? {
                    surface_exists = true;
                    break;
                }
            }
            if surface_exists {
                continue;
            }
            annotate(
                ctx,
                annotations,
                &surface_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(segment.offset),
                "evaluated_analytic_revolution_surface",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: surface_id,
                    geometry: surface,
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: if let Some(id) = native_surface {
                            crate::identity::source_object_id_checked(
                                ctx,
                                format_args!("VisibGeom:{id}"),
                                "creo source object identity",
                            )?
                        } else {
                            crate::identity::source_object_id_checked(
                                ctx,
                                format_args!(
                                    "FeatDefs:revolution#{feature_id}:segment{}",
                                    segment.external_id
                                ),
                                "creo source object identity",
                            )?
                        },
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
        if let Some(order) = definition.order_table.as_ref() {
            let std::ops::ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
                std::convert::Infallible,
            >(ctx, definition, |entity| {
                let Some((internal_id, section_geometry, offset)) =
                    saved_section_entity_geometry(entity)
                else {
                    return Ok(std::ops::ControlFlow::Continue(()));
                };
                let Some(external_id) = order.external_id(internal_id) else {
                    return Ok(std::ops::ControlFlow::Continue(()));
                };
                let Some(surface) = revolved_section_surface(transform, &section_geometry, &axis)
                else {
                    return Ok(std::ops::ControlFlow::Continue(()));
                };
                let Some(native_surface) = ordered_analytic_surface_id_for_feature(
                    ctx,
                    &scan.surfaces.rows,
                    &scan.features.entity_tables,
                    feature_id,
                    order,
                    external_id,
                    &surface,
                )?
                else {
                    return Ok(std::ops::ControlFlow::Continue(()));
                };
                let surface_id = crate::identity::compose_checked::<SurfaceId>(
                    ctx,
                    &crate::identity::VISIBGEOM_SURFACE,
                    native_surface,
                    "creo revolution surface identity",
                )?;
                let mut surface_exists = false;
                for item in
                    ctx.admit_iter(&ir.model.surfaces, "creo saved revolution surface lookup")?
                {
                    if ctx.equal(
                        &item.id,
                        &surface_id,
                        "creo saved revolution surface identity comparison",
                    )? {
                        surface_exists = true;
                        break;
                    }
                }
                if surface_exists {
                    return Ok(std::ops::ControlFlow::Continue(()));
                }
                annotate(
                    ctx,
                    annotations,
                    &surface_id,
                    "FeatDefs",
                    cadmpeg_core::decode::u64_from_index(offset),
                    "evaluated_saved_analytic_revolution_surface",
                    Exactness::Derived,
                )?;
                ctx.charge_entities(1, "admit Creo model surfaces")?;
                source_carriers.admit_surface(
                    ctx,
                    ir,
                    Surface {
                        id: surface_id,
                        geometry: surface,
                        source_object: Some(SourceObjectAssociation {
                            format: cadmpeg_ir::CodecFormat::Creo,
                            object_id: crate::identity::source_object_id_checked(
                                ctx,
                                format_args!("VisibGeom:{native_surface}"),
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
                Ok(std::ops::ControlFlow::Continue(()))
            })?;
        }
        let std::ops::ControlFlow::Continue(()) = visit_semantic_saved_section_entities::<
            std::convert::Infallible,
        >(ctx, definition, |entity| {
            let crate::feature::definitions::FeatureSavedEntity::Spline(spline) = entity else {
                return Ok(std::ops::ControlFlow::Continue(()));
            };
            let (suffix, _suffix_reservation) = if let Some(entity_id) = spline.entity_id {
                ctx.format_scoped(
                    format_args!("{entity_id}"),
                    "creo revolved spline identity suffix",
                )?
            } else {
                ctx.format_scoped(
                    format_args!("offset{}", spline.offset),
                    "creo revolved spline identity suffix",
                )?
            };
            let curve_id = crate::identity::compose_checked::<CurveId>(
                ctx,
                &crate::identity::FEATDEFS_SAVED_SPLINE_CURVE,
                format_args!("{}:{suffix}", definition.identity.id()),
                "creo revolved spline curve identity",
            )?;
            let mut matching_curve = None;
            for curve in
                ctx.admit_iter(&ir.model.curves, "creo revolved spline directrix lookup")?
            {
                if !ctx.equal(
                    &curve.id,
                    &curve_id,
                    "creo revolved spline curve identity comparison",
                )? {
                    continue;
                }
                if matching_curve.is_some() {
                    matching_curve = None;
                    break;
                }
                matching_curve = Some(curve);
            }
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix))) =
                matching_curve.map(|curve| source_carriers.curve_geometry(curve))
            else {
                return Ok(std::ops::ControlFlow::Continue(()));
            };
            let directrix_knots = directrix
                .knots()
                .first()
                .zip(directrix.knots().last())
                .map(|(lower, upper)| [*lower, *upper]);
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let surface = revolved_nurbs_surface(
                ctx,
                directrix,
                &axis,
                &format_args!(
                    "feature {feature_id} saved spline at offset {}",
                    spline.offset
                ),
                &mut refusal,
            )?;
            let refused = refusal.take_records_checked()?;
            let Some(surface) = surface.filter(|_| refused.is_empty()) else {
                for record in ctx.admit_iter(&refused, "creo revolution lane refusals")? {
                    push_revolution_surface_loss(
                        ctx,
                        losses,
                        format_args!(
                            "Feature {feature_id} states a revolved saved spline at offset {} \
                                 that forms no surface carrier: {record}",
                            spline.offset
                        ),
                    )?;
                }
                return Ok(std::ops::ControlFlow::Continue(()));
            };
            let external_id = match (definition.order_table.as_ref(), spline.entity_id) {
                (Some(order), Some(entity_id)) => order.external_id(entity_id),
                _ => None,
            };
            let native_surface = match external_id {
                Some(external_id) => ctx
                    .get_btree_map(
                        &spline_bindings,
                        &external_id,
                        "creo revolution spline binding lookup",
                    )?
                    .copied(),
                None => None,
            };
            let Some(native_surface) = native_surface else {
                return Ok(std::ops::ControlFlow::Continue(()));
            };
            let surface_id = crate::identity::compose_checked::<SurfaceId>(
                ctx,
                &crate::identity::VISIBGEOM_SURFACE,
                native_surface,
                "creo revolution surface identity",
            )?;
            let procedural_id = crate::identity::compose_checked::<ProceduralSurfaceId>(
                ctx,
                &crate::identity::FEATURE_REVOLUTION_CONSTRUCTION,
                format_args!("{feature_id}:{suffix}"),
                "creo revolution construction identity",
            )?;
            let mut surface_exists = false;
            for item in ctx.admit_iter(&ir.model.surfaces, "creo saved spline surface lookup")? {
                if ctx.equal(
                    &item.id,
                    &surface_id,
                    "creo saved spline surface identity comparison",
                )? {
                    surface_exists = true;
                    break;
                }
            }
            if surface_exists {
                return Ok(std::ops::ControlFlow::Continue(()));
            }
            annotate(
                ctx,
                annotations,
                &surface_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "evaluated_revolution_surface",
                Exactness::Derived,
            )?;
            annotate(
                ctx,
                annotations,
                &procedural_id,
                "FeatDefs",
                cadmpeg_core::decode::u64_from_index(spline.offset),
                "revolution_surface_construction",
                Exactness::Derived,
            )?;
            ctx.charge_entities(1, "admit Creo model surfaces")?;
            source_carriers.admit_surface(
                ctx,
                ir,
                Surface {
                    id: surface_id
                        .try_clone_for_decode(ctx, "creo construction surface identity copy")?,
                    geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: crate::identity::source_object_id_checked(
                            ctx,
                            format_args!("VisibGeom:{native_surface}"),
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
            source_carriers.admit_procedural_surface(
                ctx,
                ir,
                &surface_id,
                cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                    curve_id,
                    (
                        axis.origin,
                        revolution_unit_axis(ctx, feature_id, axis.direction)?,
                    ),
                    [0.0, std::f64::consts::TAU],
                    None,
                    directrix_parameter_range(
                        ctx,
                        spline.offset,
                        directrix_knots
                            .as_ref()
                            .map_or(&[][..], |knots| knots.as_slice()),
                    )?
                    .into(),
                    false,
                    cadmpeg_ir::geometry::CacheContract::from_form(None),
                )
                .map(|admitted_payload| {
                    ProceduralSurface::new(
                        procedural_id,
                        ProceduralSurfaceDefinition::Revolution(admitted_payload),
                        None,
                    )
                })
                .map_err(cadmpeg_core::CodecError::malformed)?,
            )?;
            transferred += 1;
            Ok(std::ops::ControlFlow::Continue(()))
        })?;
    }
    Ok(transferred)
}

#[cfg(test)]
mod tests;

pub(in super::super) fn transfer_resolved_revolution_vertex_orbit_curves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut pending = Vec::new();
    for transform in ctx.admit_iter(
        &scan.features.section_transforms,
        "creo revolution vertex orbit transforms",
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
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id)
            != Some(crate::feature::operations::FeatureRecipeKind::Revolve)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let extent = feature_revolution_extent(ctx, scan, feature_id)?;
        let Some(axis) = revolution_axis_for_transfer(
            ctx,
            scan,
            ir,
            source_carriers,
            feature_id,
            (definition, transform),
            extent.as_ref(),
        )?
        else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let profiles = connected_sketch_profile_vertices(ctx, ir, source_carriers, &sketch_id)?;
        for (profile_index, vertices) in
            ctx.admit_iter(&profiles, "creo revolution profile rows")?
        {
            for (vertex_index, point) in ctx
                .admit_iter(vertices, "creo revolution profile vertices")?
                .enumerate()
            {
                let Some(geometry) = revolved_section_circle(transform, *point, &axis) else {
                    continue;
                };
                let geometry = CurveGeometry::try_from(geometry)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                ctx.reserve_vec(&mut pending, 1, "creo revolution vertex orbit candidates")?;
                pending.push((
                    crate::identity::compose_checked::<CurveId>(
                        ctx, &crate::identity::FEATURE_REVOLUTION_VERTEX_ORBIT,
                        format_args!("{feature_id}:profile{profile_index}:vertex{vertex_index}"),
                        "creo revolution orbit identity",
                    )?,
                    geometry,
                    transform.offset,
                    ctx.format_retained(
                        format_args!("FeatDefs:revolution#{feature_id}:profile{profile_index}:vertex{vertex_index}"),
                        "creo revolution orbit source identity",
                    )?,
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        let mut curve_exists = false;
        for curve in ctx.admit_iter(&ir.model.curves, "creo revolution orbit curve lookup")? {
            if ctx.equal(
                &curve.id,
                &id,
                "creo revolution orbit curve identity comparison",
            )? {
                curve_exists = true;
                break;
            }
        }
        if curve_exists {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(offset),
            "evaluated_revolution_profile_vertex_orbit",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        object_id,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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
    Ok(transferred)
}

pub(in super::super) fn transfer_resolved_extrusion_vertex_orbit_curves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    source_carriers: &mut SourceUnitCarriers,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut pending = Vec::new();
    for transform in ctx.admit_iter(
        &scan.features.section_transforms,
        "creo extrusion vertex orbit transforms",
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
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(ctx, scan, feature_id)? {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let profiles = connected_sketch_profile_vertices(ctx, ir, source_carriers, &sketch_id)?;
        for (profile_index, vertices) in ctx.admit_iter(&profiles, "creo extrusion profile rows")? {
            for (vertex_index, point) in ctx
                .admit_iter(vertices, "creo extrusion profile vertices")?
                .enumerate()
            {
                let Some(geometry) = extruded_section_line(transform, *point) else {
                    continue;
                };
                ctx.reserve_vec(&mut pending, 1, "creo extrusion vertex orbit candidates")?;
                pending.push((
                    crate::identity::compose_checked::<CurveId>(
                        ctx, &crate::identity::FEATURE_EXTRUSION_VERTEX_ORBIT,
                        format_args!("{feature_id}:profile{profile_index}:vertex{vertex_index}"),
                        "creo extrusion orbit identity",
                    )?,
                    geometry,
                    transform.offset,
                    ctx.format_retained(
                        format_args!("FeatDefs:extrusion#{feature_id}:profile{profile_index}:vertex{vertex_index}"),
                        "creo extrusion orbit source identity",
                    )?,
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        let mut curve_exists = false;
        for curve in ctx.admit_iter(&ir.model.curves, "creo extrusion orbit curve lookup")? {
            if ctx.equal(
                &curve.id,
                &id,
                "creo extrusion orbit curve identity comparison",
            )? {
                curve_exists = true;
                break;
            }
        }
        if curve_exists {
            continue;
        }
        annotate(
            ctx,
            annotations,
            &id,
            "FeatDefs",
            cadmpeg_core::decode::u64_from_index(offset),
            "evaluated_extrusion_profile_vertex_orbit",
            Exactness::Derived,
        )?;
        ctx.charge_entities(1, "admit Creo model curves")?;
        source_carriers.admit_curve(
            ctx,
            ir,
            Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::for_decode(
                        ctx,
                        object_id,
                        "validate nonblank text",
                    )?
                    .ok_or_else(|| {
                        cadmpeg_core::CodecError::malformed("source object_id must not be empty")
                    })?,
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
    Ok(transferred)
}
