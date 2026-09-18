// SPDX-License-Identifier: Apache-2.0
//! Resolved revolution and extrusion surface and vertex-orbit transfer.

use super::super::native::annotate;
use super::super::sketch::coordinates::resolved_section_points;
use super::super::sketch::geometry::{
    resolved_section_segment_geometry, saved_section_entity_geometry,
};
use super::super::sketch::radii::trim_segment_id;
use super::super::sketch::skamp::complete_section_segment_rows;
use super::super::sketch_ids::model_sketch_id;
use super::super::sweep::profiles::connected_sketch_profile_vertices;
use super::super::sweep::surfaces::{
    extruded_section_line, revolved_nurbs_surface, revolved_section_circle,
    revolved_section_surface,
};
use super::super::uniqueness::{
    exactly_one, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use super::axes::revolution_axis_for_transfer;
use super::draft::feature_allows_linear_extrusion;
use super::link::{
    ordered_analytic_surface_id_for_feature, ordered_family_surface_bindings_for_feature,
    profile_segment_ids,
};
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::identity::semantic_saved_section_entities;
use crate::decode::sketch_transfer::recipe::{
    feature_recipe, feature_revolution_extent, unique_feature_revolution_extent,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

/// Transfer one exact surface carrier per resolved revolution generator.
///
/// A saved spline whose revolved lanes the IR carrier refuses states no
/// surface. The model carries the feature and its directrix curve without that
/// surface, so the refusal is a loss note naming the feature and the spline
/// offset.
pub(in super::super) fn transfer_resolved_revolution_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    losses: &mut Vec<cadmpeg_ir::report::LossNote>,
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
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve) {
            continue;
        }
        if unique_feature_revolution_extent(&scan.features.revolution_extents, feature_id).is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let extent = feature_revolution_extent(scan, feature_id);
        let Some(axis) = revolution_axis_for_transfer(
            scan,
            ir,
            feature_id,
            definition,
            transform,
            extent.as_ref(),
        ) else {
            continue;
        };
        let points = resolved_section_points(definition);
        let mut generating_ids = definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        let Some(sketch_id) = model_sketch_id(scan, definition) else {
            continue;
        };
        if let Some(sketch) = exactly_one(
            ir.model
                .sketches
                .iter()
                .filter(|sketch| sketch.id == sketch_id),
        ) {
            let segments = complete_section_segment_rows(definition);
            generating_ids.extend(profile_segment_ids(
                definition.identity.id(),
                &segments,
                &sketch.profiles,
            ));
        }
        let arc_bindings = definition
            .order_table
            .as_ref()
            .map_or_else(BTreeMap::new, |order| {
                ordered_family_surface_bindings_for_feature(
                    &scan.surfaces.rows,
                    feature_id,
                    &scan.features.entity_tables,
                    order,
                    complete_section_segment_rows(definition)
                        .iter()
                        .filter(|segment| {
                            generating_ids.contains(&segment.external_id)
                                && matches!(
                                    segment.kind,
                                    crate::feature::FeatureSegmentKind::Arc(_)
                                )
                        })
                        .map(|segment| segment.external_id),
                    crate::surface::SurfaceKind::TorusOrSphere,
                )
            });
        let spline_bindings = definition
            .order_table
            .as_ref()
            .map_or_else(BTreeMap::new, |order| {
                ordered_family_surface_bindings_for_feature(
                    &scan.surfaces.rows,
                    feature_id,
                    &scan.features.entity_tables,
                    order,
                    semantic_saved_section_entities(definition).filter_map(|entity| match entity {
                        crate::feature::FeatureSavedEntity::Spline(spline) => {
                            order.external_id(spline.entity_id?)
                        }
                        _ => None,
                    }),
                    crate::surface::SurfaceKind::Spline,
                )
            });
        for segment in complete_section_segment_rows(definition)
            .iter()
            .filter(|segment| generating_ids.contains(&segment.external_id))
        {
            let Some(geometry) = resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(surface) = revolved_section_surface(transform, &geometry, &axis) else {
                continue;
            };
            let native_surface = match segment.kind {
                crate::feature::FeatureSegmentKind::Line(_) => {
                    definition.order_table.as_ref().and_then(|order| {
                        ordered_analytic_surface_id_for_feature(
                            &scan.surfaces.rows,
                            &scan.features.entity_tables,
                            feature_id,
                            order,
                            segment.external_id,
                            &surface,
                        )
                    })
                }
                crate::feature::FeatureSegmentKind::Arc(_) => {
                    arc_bindings.get(&segment.external_id).copied()
                }
                crate::feature::FeatureSegmentKind::Point(_) => None,
            };
            let surface_id = native_surface.map_or_else(
                || {
                    SurfaceId::compose(
                        &crate::identity::FEATURE_REVOLUTION_SURFACE,
                        cadmpeg_ir::ids::IdentityKey::from(feature_id)
                            .colon(cadmpeg_ir::identity_key!("segment"))
                            .then(segment.external_id),
                    )
                },
                |id| SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, id),
            );
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                segment.offset as u64,
                "evaluated_analytic_revolution_surface",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id,
                geometry: surface,
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(native_surface.map_or_else(
                        || {
                            format!(
                                "FeatDefs:revolution#{feature_id}:segment{}",
                                segment.external_id
                            )
                        },
                        |id| format!("VisibGeom:{id}"),
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
        if let Some(order) = definition.order_table.as_ref() {
            for (internal_id, section_geometry, offset) in
                semantic_saved_section_entities(definition)
                    .filter_map(saved_section_entity_geometry)
            {
                let Some(external_id) = order.external_id(internal_id) else {
                    continue;
                };
                let Some(surface) = revolved_section_surface(transform, &section_geometry, &axis)
                else {
                    continue;
                };
                let Some(native_surface) = ordered_analytic_surface_id_for_feature(
                    &scan.surfaces.rows,
                    &scan.features.entity_tables,
                    feature_id,
                    order,
                    external_id,
                    &surface,
                ) else {
                    continue;
                };
                let surface_id =
                    SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, native_surface);
                if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                    continue;
                }
                annotate(
                    annotations,
                    &surface_id,
                    "FeatDefs",
                    offset as u64,
                    "evaluated_saved_analytic_revolution_surface",
                    Exactness::Derived,
                );
                ir.model.surfaces.push(Surface {
                    id: surface_id,
                    geometry: surface,
                    source_object: Some(SourceObjectAssociation {
                        format: cadmpeg_ir::CodecFormat::Creo,
                        object_id: cadmpeg_core::text::NonBlankString::new(format!(
                            "VisibGeom:{native_surface}"
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
                transferred += 1;
            }
        }
        for spline in
            semantic_saved_section_entities(definition).filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let suffix_key = match spline.entity_id {
                Some(entity_id) => cadmpeg_ir::ids::IdentityKey::from(entity_id),
                None => cadmpeg_ir::ids::IdentityKey::try_new(format!("offset{}", spline.offset))
                    .map_err(|error| {
                    cadmpeg_core::CodecError::malformed(format!(
                        "FeatDefs saved spline identity at offset {}: {error}",
                        spline.offset
                    ))
                })?,
            };
            let curve_id = CurveId::compose(
                &crate::identity::FEATDEFS_SAVED_SPLINE_CURVE,
                cadmpeg_ir::ids::IdentityKey::from(definition.identity.id())
                    .colon(suffix_key.clone()),
            );
            let Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(directrix))) =
                exactly_one(ir.model.curves.iter().filter(|curve| curve.id == curve_id))
                    .map(|curve| &curve.geometry)
            else {
                continue;
            };
            let mut refusal = crate::lane_refusal::LaneRefusals::new();
            let surface = revolved_nurbs_surface(
                directrix,
                &axis,
                &format!(
                    "feature {feature_id} saved spline at offset {}",
                    spline.offset
                ),
                &mut refusal,
            );
            let refused = refusal.take_records();
            let Some(surface) = surface.filter(|_| refused.is_empty()) else {
                for record in &refused {
                    losses.push(
                        crate::loss::CreoLossCode::FeatureSurfaceOperationIncomplete.note(format!(
                            "Feature {feature_id} states a revolved saved spline at offset {} \
                             that forms no surface carrier: {record}",
                            spline.offset
                        )),
                    );
                }
                continue;
            };
            let native_surface = definition
                .order_table
                .as_ref()
                .and_then(|order| order.external_id(spline.entity_id?))
                .and_then(|external_id| spline_bindings.get(&external_id).copied());
            let Some(native_surface) = native_surface else {
                continue;
            };
            let surface_id =
                SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, native_surface);
            let procedural_id = ProceduralSurfaceId::compose(
                &crate::identity::FEATURE_REVOLUTION_CONSTRUCTION,
                cadmpeg_ir::ids::IdentityKey::from(feature_id).colon(suffix_key),
            );
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "evaluated_revolution_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "revolution_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)),
                source_object: Some(SourceObjectAssociation {
                    format: cadmpeg_ir::CodecFormat::Creo,
                    object_id: cadmpeg_core::text::NonBlankString::new(format!(
                        "VisibGeom:{native_surface}"
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
            let _attached = ir.model.add_procedural_surface(
                surface_id,
                cadmpeg_ir::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(
                    curve_id,
                    (axis.origin.get(), axis.direction.get()),
                    [0.0, std::f64::consts::TAU],
                    None,
                    [
                        *directrix.knots().first().ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(format!(
                                "FeatDefs saved spline at offset {} has no knots",
                                spline.offset
                            ))
                        })?,
                        *directrix.knots().last().ok_or_else(|| {
                            cadmpeg_core::CodecError::malformed(format!(
                                "FeatDefs saved spline at offset {} has no knots",
                                spline.offset
                            ))
                        })?,
                    ]
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
            );
            transferred += 1;
        }
    }
    Ok(transferred)
}

#[cfg(test)]
mod tests;

pub(in super::super) fn transfer_resolved_revolution_vertex_orbit_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut pending = Vec::new();
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
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if feature_recipe(scan, feature_id) != Some(crate::feature::FeatureRecipeKind::Revolve) {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let extent = feature_revolution_extent(scan, feature_id);
        let Some(axis) = revolution_axis_for_transfer(
            scan,
            ir,
            feature_id,
            definition,
            transform,
            extent.as_ref(),
        ) else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(scan, definition) else {
            continue;
        };
        for (profile_index, vertices) in connected_sketch_profile_vertices(ir, &sketch_id) {
            for (vertex_index, point) in vertices.iter().enumerate() {
                let Some(geometry) = revolved_section_circle(transform, *point, &axis) else {
                    continue;
                };
                let geometry = CurveGeometry::try_from(geometry)
                    .map_err(cadmpeg_core::CodecError::malformed)?;
                pending.push((
                    CurveId::compose(
                        &crate::identity::FEATURE_REVOLUTION_VERTEX_ORBIT,
                        cadmpeg_ir::ids::IdentityKey::from(feature_id)
                            .colon(cadmpeg_ir::identity_key!("profile"))
                            .then(profile_index)
                            .colon(cadmpeg_ir::identity_key!("vertex"))
                            .then(vertex_index),
                    ),
                    geometry,
                    transform.offset,
                    format!(
                        "FeatDefs:revolution#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    ),
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "FeatDefs",
            offset as u64,
            "evaluated_revolution_profile_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(object_id).ok_or_else(|| {
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
    Ok(transferred)
}

pub(in super::super) fn transfer_resolved_extrusion_vertex_orbit_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut pending = Vec::new();
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
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(scan, feature_id) {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let Some(sketch_id) = model_sketch_id(scan, definition) else {
            continue;
        };
        for (profile_index, vertices) in connected_sketch_profile_vertices(ir, &sketch_id) {
            for (vertex_index, point) in vertices.iter().enumerate() {
                let Some(geometry) = extruded_section_line(transform, *point) else {
                    continue;
                };
                pending.push((
                    CurveId::compose(
                        &crate::identity::FEATURE_EXTRUSION_VERTEX_ORBIT,
                        cadmpeg_ir::ids::IdentityKey::from(feature_id)
                            .colon(cadmpeg_ir::identity_key!("profile"))
                            .then(profile_index)
                            .colon(cadmpeg_ir::identity_key!("vertex"))
                            .then(vertex_index),
                    ),
                    geometry,
                    transform.offset,
                    format!(
                        "FeatDefs:extrusion#{feature_id}:profile{profile_index}:vertex{vertex_index}"
                    ),
                ));
            }
        }
    }
    let mut transferred = 0;
    for (id, geometry, offset, object_id) in pending {
        if ir.model.curves.iter().any(|curve| curve.id == id) {
            continue;
        }
        annotate(
            annotations,
            &id,
            "FeatDefs",
            offset as u64,
            "evaluated_extrusion_profile_vertex_orbit",
            Exactness::Derived,
        );
        ir.model.curves.push(Curve {
            id,
            geometry,
            source_object: Some(SourceObjectAssociation {
                format: cadmpeg_ir::CodecFormat::Creo,
                object_id: cadmpeg_core::text::NonBlankString::new(object_id).ok_or_else(|| {
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
    Ok(transferred)
}
