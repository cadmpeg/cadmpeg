// SPDX-License-Identifier: Apache-2.0
//! Section sketch entity emission and placed section curves.

use super::super::feature_history::{
    section_entity_is_generated_profile, section_generated_profile_surface_kinds,
};
use super::super::native::annotate;
use super::super::sketch::{saved_profile_chains, saved_section_entity_geometry};
use super::super::sketch_ids::{
    sketch_entity_id, sketch_identity_scope, sketch_native_ref, sketch_point_ref,
    sketch_section_curve_id,
};
use super::super::sweep::{
    placed_section_geometry_curve, placed_sketch_curve_ref, saved_spline_sketch_geometry,
};
use super::{
    opaque_section_segment_identity_suffix, saved_section_external_id,
    section_degenerate_axis_line, section_segment_identity_suffix, semantic_saved_section_entities,
    unique_section_incidence_curve_family, unresolved_saved_section_entity,
    SectionEntityIncidenceFamily,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::Curve;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry, SketchId,
};
use cadmpeg_ir::{AnnotationBuilder, Exactness, SourceObjectAssociation};
use std::collections::{BTreeMap, BTreeSet};

#[allow(clippy::too_many_arguments)] // mechanical extract from transfer_sketches
pub(super) fn transfer_section_entities(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    definition: &crate::feature::FeatureDefinition,
    transform: Option<&crate::placement::FeatureSectionTransform>,
    sketch_id: &SketchId,
    segments: &[crate::feature::FeatureSegment],
    unique_segment_ids: &BTreeSet<u32>,
    unique_saved_ids: &BTreeSet<u32>,
    ambiguous_segment_ids: &BTreeSet<u32>,
    complete_segment_table: bool,
    solved: &BTreeSet<u32>,
    segment_geometries: &BTreeMap<usize, Option<SketchGeometry>>,
    resolved_segment_geometries: &BTreeMap<usize, Option<SketchGeometry>>,
    circle_geometries: &BTreeMap<usize, SketchGeometry>,
    point_geometries: &BTreeMap<usize, SketchGeometry>,
    centered_line_geometries: &BTreeMap<usize, SketchGeometry>,
    reference_line_geometries: &BTreeMap<usize, SketchGeometry>,
    materialized_saved_section_external_ids: &BTreeSet<u32>,
    mut profiles: Vec<Vec<SketchEntityUse>>,
    profile_entities: &BTreeSet<SketchEntityId>,
) -> (Vec<SketchEntity>, Vec<Vec<SketchEntityUse>>) {
    let segment_geometry = |segment: &crate::feature::FeatureSegment| {
        if section_degenerate_axis_line(definition, segment) {
            return segment_geometries
                .get(&segment.offset)
                .cloned()
                .flatten()
                .or_else(|| {
                    Some(SketchGeometry::Native {
                        native_kind: "line".to_string(),
                    })
                });
        }
        segment_geometries.get(&segment.offset).cloned().flatten()
    };
    let mut entities = segments
        .iter()
        .filter_map(|segment| {
            let geometry = segment_geometry(segment)?;
            let suffix = section_segment_identity_suffix(unique_segment_ids, segment);
            let id = sketch_entity_id(sketch_id, &suffix);
            annotate(
                annotations,
                &id.0,
                "FeatDefs",
                segment.offset as u64,
                match (&geometry, segment.kind) {
                    (SketchGeometry::Native { native_kind }, _) if native_kind == "line" => {
                        "section_degenerate_axis_line"
                    }
                    (SketchGeometry::ReferenceLine { .. }, _) => {
                        "solved_section_axis_reference_line"
                    }
                    (_, crate::feature::FeatureSegmentKind::Line) => "solved_section_line",
                    (_, crate::feature::FeatureSegmentKind::Arc) => "solved_section_arc",
                    (_, crate::feature::FeatureSegmentKind::Point) => "solved_section_point",
                },
                if matches!(&geometry, SketchGeometry::Native { .. }) {
                    Exactness::ByteExact
                } else {
                    Exactness::Derived
                },
            );
            let construction = matches!(geometry, SketchGeometry::ReferenceLine { .. })
                || !unique_segment_ids.contains(&segment.external_id)
                || (!solved.contains(&segment.external_id) && !profile_entities.contains(&id));
            let endpoint_refs = match (&geometry, segment.kind) {
                (SketchGeometry::Native { native_kind }, _) if native_kind == "line" => {
                    vec![segment.point_ids[0]]
                }
                (SketchGeometry::ReferenceLine { .. }, _)
                    if section_degenerate_axis_line(definition, segment) =>
                {
                    vec![segment.point_ids[0]]
                }
                (_, crate::feature::FeatureSegmentKind::Arc) => {
                    vec![segment.point_ids[1], segment.point_ids[0]]
                }
                (_, crate::feature::FeatureSegmentKind::Line) => segment.point_ids.to_vec(),
                (_, crate::feature::FeatureSegmentKind::Point) => {
                    vec![segment.point_ids[0]]
                }
            }
            .into_iter()
            .map(|point| sketch_point_ref(sketch_id, point))
            .collect();
            let geometry_ref = placed_sketch_curve_ref(transform, sketch_id, suffix, &geometry);
            Some(
                SketchEntity::new(id, sketch_id.clone(), geometry)
                    .with_construction(construction)
                    .with_native_ref(Some(sketch_native_ref(sketch_id)))
                    .with_geometry_ref(geometry_ref)
                    .with_endpoint_refs(endpoint_refs),
            )
        })
        .collect::<Vec<_>>();
    for segment in segments
        .iter()
        .filter(|segment| segment_geometry(segment).is_none())
    {
        let id = sketch_entity_id(
            sketch_id,
            section_segment_identity_suffix(unique_segment_ids, segment),
        );
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            "unresolved_section_segment",
            Exactness::ByteExact,
        );
        let endpoint_refs = match segment.kind {
            crate::feature::FeatureSegmentKind::Arc => {
                vec![segment.point_ids[1], segment.point_ids[0]]
            }
            crate::feature::FeatureSegmentKind::Line => segment.point_ids.to_vec(),
            crate::feature::FeatureSegmentKind::Point => vec![segment.point_ids[0]],
        }
        .into_iter()
        .map(|point| sketch_point_ref(sketch_id, point))
        .collect();
        entities.push(
            SketchEntity::new(
                id,
                sketch_id.clone(),
                SketchGeometry::Native {
                    native_kind: match segment.kind {
                        crate::feature::FeatureSegmentKind::Line => "line",
                        crate::feature::FeatureSegmentKind::Arc => "arc",
                        crate::feature::FeatureSegmentKind::Point => "point",
                    }
                    .to_string(),
                },
            )
            .with_construction(true)
            .with_native_ref(Some(sketch_native_ref(sketch_id)))
            .with_endpoint_refs(endpoint_refs),
        );
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.circle_rows)
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = if unique_external_id {
            segment.external_id.to_string()
        } else {
            format!("circle:offset:{}", segment.offset)
        };
        let id = sketch_entity_id(sketch_id, &suffix);
        let geometry = circle_geometries
            .get(&segment.offset)
            .cloned()
            .unwrap_or_else(|| SketchGeometry::Native {
                native_kind: "circle".to_string(),
            });
        let solved_geometry = matches!(geometry, SketchGeometry::Circle { .. });
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            if solved_geometry {
                "solved_section_circle"
            } else {
                "unresolved_section_circle"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        let construction = !unique_external_id || !profile_entities.contains(&id);
        let geometry_ref = placed_sketch_curve_ref(transform, sketch_id, suffix, &geometry);
        entities.push(
            SketchEntity::new(id, sketch_id.clone(), geometry)
                .with_construction(construction)
                .with_native_ref(Some(sketch_native_ref(sketch_id)))
                .with_geometry_ref(geometry_ref),
        );
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.point_rows)
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = if unique_external_id {
            segment.external_id.to_string()
        } else {
            format!("point:offset:{}", segment.offset)
        };
        let id = sketch_entity_id(sketch_id, &suffix);
        let geometry = point_geometries
            .get(&segment.offset)
            .cloned()
            .unwrap_or_else(|| SketchGeometry::Native {
                native_kind: "point".to_string(),
            });
        let solved_geometry = matches!(geometry, SketchGeometry::Point { .. });
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            if solved_geometry {
                "solved_section_point"
            } else {
                "unresolved_section_point"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        let construction = !unique_external_id || !profile_entities.contains(&id);
        entities.push(
            SketchEntity::new(id, sketch_id.clone(), geometry)
                .with_construction(construction)
                .with_native_ref(Some(sketch_native_ref(sketch_id)))
                .with_endpoint_refs(vec![sketch_point_ref(sketch_id, segment.point_id)]),
        );
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.centered_line_rows)
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = if unique_external_id {
            segment.external_id.to_string()
        } else {
            format!("centered_line:offset:{}", segment.offset)
        };
        let id = sketch_entity_id(sketch_id, &suffix);
        let geometry = centered_line_geometries
            .get(&segment.offset)
            .cloned()
            .unwrap_or_else(|| SketchGeometry::Native {
                native_kind: "line".to_string(),
            });
        let solved_geometry = matches!(geometry, SketchGeometry::Line { .. });
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            if solved_geometry {
                "solved_section_centered_line"
            } else {
                "unresolved_section_centered_line"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        let geometry_ref = placed_sketch_curve_ref(transform, sketch_id, suffix, &geometry);
        let endpoint_refs = [0, 1]
            .into_iter()
            .map(|point| sketch_point_ref(sketch_id, point))
            .collect();
        entities.push(
            SketchEntity::new(id, sketch_id.clone(), geometry)
                .with_construction(true)
                .with_native_ref(Some(sketch_native_ref(sketch_id)))
                .with_geometry_ref(geometry_ref)
                .with_endpoint_refs(endpoint_refs),
        );
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.reference_line_rows)
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = if unique_external_id {
            segment.external_id.to_string()
        } else {
            format!("reference_line:offset:{}", segment.offset)
        };
        let id = sketch_entity_id(sketch_id, &suffix);
        let geometry = reference_line_geometries
            .get(&segment.offset)
            .cloned()
            .unwrap_or_else(|| SketchGeometry::Native {
                native_kind: "reference_line".to_string(),
            });
        let solved_geometry = matches!(geometry, SketchGeometry::ReferenceLine { .. });
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            if solved_geometry {
                "solved_section_reference_line"
            } else {
                "unresolved_section_reference_line"
            },
            if solved_geometry {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        );
        let geometry_ref = placed_sketch_curve_ref(transform, sketch_id, suffix, &geometry);
        let endpoint_refs = segment
            .point_ids
            .into_iter()
            .flatten()
            .map(|point| sketch_point_ref(sketch_id, point))
            .collect();
        entities.push(
            SketchEntity::new(id, sketch_id.clone(), geometry)
                .with_construction(true)
                .with_native_ref(Some(sketch_native_ref(sketch_id)))
                .with_geometry_ref(geometry_ref)
                .with_endpoint_refs(endpoint_refs),
        );
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.bounded_curve_rows)
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = if unique_external_id {
            segment.external_id.to_string()
        } else {
            format!("bounded_curve:offset:{}", segment.offset)
        };
        let id = sketch_entity_id(sketch_id, &suffix);
        let construction = !unique_external_id || !profile_entities.contains(&id);
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            "unresolved_section_bounded_curve",
            Exactness::ByteExact,
        );
        let endpoint_refs = segment
            .point_ids
            .into_iter()
            .map(|point| sketch_point_ref(sketch_id, point))
            .collect();
        entities.push(
            SketchEntity::new(
                id,
                sketch_id.clone(),
                SketchGeometry::Native {
                    native_kind: "bounded_curve".to_string(),
                },
            )
            .with_construction(construction)
            .with_native_ref(Some(sketch_native_ref(sketch_id)))
            .with_endpoint_refs(endpoint_refs),
        );
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.conic_rows)
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = if unique_external_id {
            segment.external_id.to_string()
        } else {
            format!("conic:offset:{}", segment.offset)
        };
        let id = sketch_entity_id(sketch_id, suffix);
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            "unresolved_section_conic",
            Exactness::ByteExact,
        );
        entities.push(
            SketchEntity::new(
                id,
                sketch_id.clone(),
                SketchGeometry::Native {
                    native_kind: "conic".to_string(),
                },
            )
            .with_construction(true)
            .with_native_ref(Some(sketch_native_ref(sketch_id))),
        );
    }
    for segment in definition
        .segments
        .iter()
        .flat_map(|table| &table.opaque_rows)
    {
        let unique_external_id = unique_segment_ids.contains(&segment.external_id);
        if unique_external_id
            && materialized_saved_section_external_ids.contains(&segment.external_id)
        {
            continue;
        }
        let suffix = opaque_section_segment_identity_suffix(unique_segment_ids, segment);
        let id = sketch_entity_id(sketch_id, suffix);
        let geometry = if unique_external_id {
            let native_kind =
                match unique_section_incidence_curve_family(definition, segment.external_id) {
                    Some(SectionEntityIncidenceFamily::Point) => "point".to_string(),
                    Some(SectionEntityIncidenceFamily::BoundedCurve) => "bounded_curve".to_string(),
                    Some(SectionEntityIncidenceFamily::Line) => "line".to_string(),
                    Some(SectionEntityIncidenceFamily::Arc) => "arc".to_string(),
                    Some(SectionEntityIncidenceFamily::Circular) => "circle".to_string(),
                    _ => format!("segment_type:{}", segment.kind),
                };
            SketchGeometry::Native { native_kind }
        } else {
            SketchGeometry::Native {
                native_kind: format!("segment_type:{}", segment.kind),
            }
        };
        let construction = !unique_external_id || !profile_entities.contains(&id);
        annotate(
            annotations,
            &id.0,
            "FeatDefs",
            segment.offset as u64,
            "opaque_section_segment",
            Exactness::ByteExact,
        );
        let geometry_ref = placed_sketch_curve_ref(
            transform,
            sketch_id,
            if unique_external_id {
                segment.external_id.to_string()
            } else {
                format!("opaque:offset:{}", segment.offset)
            },
            &geometry,
        );
        entities.push(
            SketchEntity::new(id, sketch_id.clone(), geometry)
                .with_construction(construction)
                .with_native_ref(Some(sketch_native_ref(sketch_id)))
                .with_geometry_ref(geometry_ref),
        );
    }
    let mut saved_section_geometries = Vec::new();
    let mut generated_saved_geometries = Vec::new();
    for (internal_id, geometry, offset) in
        semantic_saved_section_entities(definition).filter_map(saved_section_entity_geometry)
    {
        let unique_internal_id = unique_saved_ids.contains(&internal_id);
        let external_id = if unique_internal_id {
            definition.order_table.as_ref().and_then(|order| {
                saved_section_external_id(
                    order,
                    unique_saved_ids,
                    ambiguous_segment_ids,
                    internal_id,
                )
            })
        } else {
            None
        };
        let suffix = if unique_internal_id {
            external_id.map_or_else(
                || format!("saved{internal_id}"),
                |external_id| external_id.to_string(),
            )
        } else {
            format!("saved:offset:{offset}")
        };
        let entity_id = sketch_entity_id(sketch_id, &suffix);
        if entities.iter().any(|entity| entity.id() == &entity_id) {
            continue;
        }
        let generated = external_id.is_some_and(|external_id| {
            section_generated_profile_surface_kinds(&geometry).is_some_and(|expected_kinds| {
                section_entity_is_generated_profile(
                    complete_segment_table,
                    definition.owner_feature_id,
                    external_id,
                    expected_kinds,
                    &scan.features.entity_tables,
                    &scan.surfaces.rows,
                )
            })
        });
        let curve_id = CurveId(sketch_section_curve_id(sketch_id, &suffix));
        annotate(
            annotations,
            &entity_id.0,
            "FeatDefs",
            offset as u64,
            "saved_section_entity",
            Exactness::Derived,
        );
        if let Some(external_id) = external_id.filter(|_| generated) {
            generated_saved_geometries.push((external_id, geometry.clone()));
        }
        entities.push(
            SketchEntity::new(entity_id, sketch_id.clone(), geometry.clone())
                .with_construction(!generated)
                .with_native_ref(Some(format!(
                    "{}:saved_entity#{internal_id}",
                    sketch_native_ref(sketch_id)
                )))
                .with_geometry_ref(placed_sketch_curve_ref(
                    transform, sketch_id, &suffix, &geometry,
                )),
        );
        saved_section_geometries.push((internal_id, external_id, geometry, offset, curve_id));
    }
    for spline in semantic_saved_section_entities(definition).filter_map(|entity| match entity {
        crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
        _ => None,
    }) {
        let Some(geometry) = saved_spline_sketch_geometry(spline) else {
            continue;
        };
        let unique_internal_id = spline
            .entity_id
            .is_some_and(|id| unique_saved_ids.contains(&id));
        let suffix = if unique_internal_id {
            spline
                .entity_id
                .expect("unique saved spline has an internal id")
                .to_string()
        } else {
            format!("offset{}", spline.offset)
        };
        let external_id = if unique_internal_id {
            definition.order_table.as_ref().and_then(|order| {
                saved_section_external_id(
                    order,
                    unique_saved_ids,
                    ambiguous_segment_ids,
                    spline.entity_id?,
                )
            })
        } else {
            None
        };
        let generated = external_id.is_some_and(|external_id| {
            let Some(expected_kinds) = section_generated_profile_surface_kinds(&geometry) else {
                return false;
            };
            section_entity_is_generated_profile(
                complete_segment_table,
                definition.owner_feature_id,
                external_id,
                expected_kinds,
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            )
        });
        let entity_id = external_id.map_or_else(
            || {
                SketchEntityId(format!(
                    "creo:featdefs:saved_spline#{}:{suffix}",
                    sketch_identity_scope(sketch_id)
                ))
            },
            |external_id| sketch_entity_id(sketch_id, external_id),
        );
        let curve_id = CurveId(format!(
            "creo:featdefs:saved_spline_curve#{}:{suffix}",
            sketch_identity_scope(sketch_id)
        ));
        if entities.iter().any(|entity| entity.id() == &entity_id) {
            continue;
        }
        annotate(
            annotations,
            &entity_id.0,
            "FeatDefs",
            spline.offset as u64,
            "saved_interpolation_spline",
            Exactness::Derived,
        );
        entities.push(
            SketchEntity::new(entity_id, sketch_id.clone(), geometry.clone())
                .with_construction(!generated)
                .with_native_ref(Some(format!(
                    "{}:saved_spline#{suffix}",
                    sketch_native_ref(sketch_id)
                )))
                .with_geometry_ref(transform.map(|_| curve_id.0.clone())),
        );
        if let Some(external_id) = external_id.filter(|_| generated) {
            generated_saved_geometries.push((external_id, geometry));
        }
    }
    for saved in semantic_saved_section_entities(definition) {
        let (entity, offset) = unresolved_saved_section_entity(
            definition,
            sketch_id,
            saved,
            unique_saved_ids,
            ambiguous_segment_ids,
        );
        if entities.iter().any(|existing| existing.id() == entity.id()) {
            continue;
        }
        annotate(
            annotations,
            entity.id().0.as_str(),
            "FeatDefs",
            offset as u64,
            "unresolved_saved_section_entity",
            Exactness::ByteExact,
        );
        entities.push(entity);
    }
    profiles.extend(saved_profile_chains(sketch_id, &generated_saved_geometries));
    if let Some(transform) = transform {
        for segment in segments {
            let Some(section_geometry) = resolved_segment_geometries
                .get(&segment.offset)
                .cloned()
                .flatten()
                .or_else(|| {
                    segment_geometries
                        .get(&segment.offset)
                        .cloned()
                        .flatten()
                        .filter(|geometry| matches!(geometry, SketchGeometry::ReferenceLine { .. }))
                })
            else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            let suffix = section_segment_identity_suffix(unique_segment_ids, segment);
            let id = CurveId(sketch_section_curve_id(sketch_id, &suffix));
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "placed_section_curve",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!(
                        "FeatDefs:section#{}:{suffix}",
                        sketch_identity_scope(sketch_id)
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|segments| &segments.circle_rows)
        {
            let Some(section_geometry) = circle_geometries.get(&segment.offset).cloned() else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                segment.external_id.to_string()
            } else {
                format!("circle:offset:{}", segment.offset)
            };
            let id = CurveId(sketch_section_curve_id(sketch_id, &suffix));
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "placed_section_circle",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!(
                        "FeatDefs:section#{}:{suffix}",
                        sketch_identity_scope(sketch_id)
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        for segment in definition
            .segments
            .iter()
            .flat_map(|segments| &segments.centered_line_rows)
        {
            let Some(section_geometry) = centered_line_geometries.get(&segment.offset).cloned()
            else {
                continue;
            };
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            let suffix = if unique_segment_ids.contains(&segment.external_id) {
                segment.external_id.to_string()
            } else {
                format!("centered_line:offset:{}", segment.offset)
            };
            let id = CurveId(sketch_section_curve_id(sketch_id, &suffix));
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "placed_section_line",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!(
                        "FeatDefs:section#{}:{suffix}",
                        sketch_identity_scope(sketch_id)
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
        for (internal_id, external_id, section_geometry, offset, id) in saved_section_geometries {
            if ir.model.curves.iter().any(|existing| existing.id == id) {
                continue;
            }
            let Some(geometry) = placed_section_geometry_curve(transform, &section_geometry) else {
                continue;
            };
            annotate(
                annotations,
                &id,
                "FeatDefs",
                offset as u64,
                "placed_saved_section_curve",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: external_id.map_or_else(
                        || format!("FeatDefs:saved_entity#{internal_id}"),
                        |external_id| {
                            format!(
                                "FeatDefs:section#{}:{external_id}",
                                sketch_identity_scope(sketch_id)
                            )
                        },
                    ),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
        }
    }
    (entities, profiles)
}
