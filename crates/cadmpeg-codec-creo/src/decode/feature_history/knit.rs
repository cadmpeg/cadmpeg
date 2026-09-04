// SPDX-License-Identifier: Apache-2.0
//! Filled, knit, draft, thicken, and result-topology feature recipes.

use super::super::analytic::{dot, PlaneEquation};
use super::super::sketch::normalized;
use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{exactly_one, unique_feature_profile_definition};
use super::{
    feature_result_edge_ids, model_feature_ids, preceding_feature_entity_producers,
    surface_merge_quilt_ids, surface_merge_quilt_state_offset, unique_positive_length,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, FeatureResultTopology, GeneratedFaceRef, PathRef, SurfaceBoundary,
    SurfaceContinuity, ThickenSide,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use std::collections::{BTreeMap, BTreeSet};

const EPS_NORMAL_ALIGNMENT: f64 = 1.0e-9;
const EPS_OFFSET_AGREEMENT: f64 = 1.0e-9;

pub(in super::super) fn filled_surface_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> IrFeatureDefinition {
    let boundary = unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )
    .map(|definition| model_sketch_id(scan, definition))
    .filter(|sketch| {
        ir.model
            .sketches
            .iter()
            .any(|candidate| candidate.id == *sketch)
    })
    .map_or(
        SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        |sketch| SurfaceBoundary::Path(PathRef::Sketch(sketch)),
    );
    IrFeatureDefinition::FilledSurface {
        boundary,
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
            SurfaceContinuity::Contact,
        ),
        merge_result: Some(false),
    }
}

pub(in super::super) fn class_100_operand_producers(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
) -> Option<Vec<(u32, u32)>> {
    let consumer_tables = tables
        .iter()
        .enumerate()
        .filter(|(_, table)| table.feature_id == Some(feature_id) && table.table_class_id == 100)
        .collect::<Vec<_>>();
    let consumers = consumer_tables
        .iter()
        .flat_map(|(table_index, table)| {
            table
                .entries
                .iter()
                .enumerate()
                .map(move |(entry_index, entry)| {
                    (
                        (table.offset, entry.offset, *table_index, entry_index),
                        entry.entity_id,
                    )
                })
        })
        .collect::<Vec<_>>();
    if consumers.is_empty()
        || consumers
            .iter()
            .map(|(_, entity_id)| entity_id)
            .collect::<BTreeSet<_>>()
            .len()
            != consumers.len()
    {
        return None;
    }
    consumers
        .into_iter()
        .map(|(consumer_position, entity_id)| {
            let producers = tables
                .iter()
                .enumerate()
                .flat_map(|(table_index, table)| {
                    let Some(owner) = table.feature_id else {
                        return Vec::new();
                    };
                    if owner == feature_id {
                        return Vec::new();
                    }
                    table
                        .entries
                        .iter()
                        .enumerate()
                        .filter_map(|(entry_index, entry)| {
                            let position = (table.offset, entry.offset, table_index, entry_index);
                            (position < consumer_position
                                && entry.class_id == 200
                                && entry.entity_id == entity_id)
                                .then_some(owner)
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            let [producer] = producers.as_slice() else {
                return None;
            };
            Some((entity_id, *producer))
        })
        .collect()
}

pub(in super::super) fn knit_class_100_operand_entity_ids(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
) -> Option<Vec<u32>> {
    class_100_operand_producers(feature_id, tables).map(|operands| {
        operands
            .into_iter()
            .map(|(entity_id, _)| entity_id)
            .collect()
    })
}

pub(in super::super) fn knit_operand_entity_ids(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<(Vec<u32>, &'static str)> {
    if let Some(ids) = surface_merge_quilt_ids(
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
    ) {
        let ids = ids.to_vec();
        if ids.iter().collect::<BTreeSet<_>>().len() == ids.len() {
            return Some((ids, "surface_merge_quilts"));
        }
        return None;
    }
    knit_class_100_operand_entity_ids(feature_id, &scan.features.entity_tables)
        .map(|ids| (ids, "surface_merge_entities"))
}

pub(in super::super) fn knit_operand_surface_ids(
    scan: &ContainerScan,
    feature_id: u32,
    quilt_ids: &[u32],
) -> Option<Vec<u32>> {
    let consumer_offset = surface_merge_quilt_state_offset(
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
        quilt_ids,
    )?;
    let surface_ids = quilt_ids
        .iter()
        .map(|quilt_id| {
            let producers = preceding_feature_entity_producers(
                &scan.features.entity_tables,
                *quilt_id,
                consumer_offset,
            );
            let [producer] = producers.as_slice() else {
                return None;
            };
            if *producer == feature_id {
                return None;
            }
            let matching_entries = scan
                .features
                .entity_tables
                .iter()
                .filter(|table| {
                    table.feature_id == Some(*producer)
                        && table.table_class_id == 100
                        && table.offset < consumer_offset
                })
                .flat_map(|table| table.entries.iter())
                .filter(|entry| entry.entity_id == *quilt_id && entry.offset < consumer_offset)
                .collect::<Vec<_>>();
            let [entry] = matching_entries.as_slice() else {
                return None;
            };
            let surface = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.class_id)?;
            (surface.feature_id == *producer).then_some(entry.class_id)
        })
        .collect::<Option<Vec<_>>>()?;
    (surface_ids.iter().collect::<BTreeSet<_>>().len() == surface_ids.len()).then_some(surface_ids)
}

pub(in super::super) fn knit_surface_feature_definition(
    scan: &ContainerScan,
    feature_id: u32,
) -> IrFeatureDefinition {
    let faces = knit_operand_entity_ids(scan, feature_id).map_or(
        FaceSelection::Unresolved,
        |(quilt_ids, namespace)| {
            let native = format!(
                "creo:allfeatur:{namespace}#{feature_id}:{}",
                quilt_ids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            );
            let available_features = model_feature_ids(scan);
            let result_surface_ids = feature_result_surface_ids_by_feature(
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            );
            let generated =
                knit_operand_surface_ids(scan, feature_id, &quilt_ids).and_then(|surface_ids| {
                    generated_surface_face_refs(
                        &surface_ids,
                        &scan.surfaces.rows,
                        &result_surface_ids,
                        &available_features,
                    )
                });
            match generated {
                Some(faces) => FaceSelection::Generated { faces, native },
                None => FaceSelection::Native(native),
            }
        },
    );
    IrFeatureDefinition::KnitSurface {
        faces,
        merge_entities: Some(true),
        create_solid: Some(false),
        gap_tolerance: None,
    }
}

/// Select the neutral plane carried by a Draft feature's class-209 entity.
///
/// The class is a neutral-plane carrier only when it has one unambiguous
/// feature-owned surface row and that row is a plane. The table class is not
/// part of the rule: Draft records use more than one enclosing table class.
pub(in super::super) fn draft_neutral_plane_selection(
    scan: &ContainerScan,
    feature_id: u32,
) -> FaceSelection {
    let Some((table, entry)) = exactly_one(
        scan.features
            .entity_tables
            .iter()
            .filter(|table| table.feature_id == Some(feature_id))
            .flat_map(|table| {
                table
                    .entries
                    .iter()
                    .filter(|entry| entry.class_id == 209)
                    .map(move |entry| (table, entry))
            }),
    ) else {
        return FaceSelection::Unresolved;
    };
    if table
        .surface_ids
        .iter()
        .filter(|surface_id| **surface_id == entry.entity_id)
        .count()
        != 1
    {
        return FaceSelection::Unresolved;
    }
    let Some(surface) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
        .filter(|surface| {
            surface.feature_id == feature_id && surface.kind == crate::surface::SurfaceKind::Plane
        })
    else {
        return FaceSelection::Unresolved;
    };
    FaceSelection::Native(format!("creo:visibgeom:surface#{}", surface.id))
}

pub(in super::super) fn feature_surface_transitions(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Option<Vec<(u32, u32)>> {
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let outputs = owned
        .iter()
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| entry.class_id == 210)
                .map(move |entry| (*table, entry))
        })
        .collect::<Vec<_>>();
    if outputs.is_empty() {
        return None;
    }
    let predecessors = owned
        .iter()
        .flat_map(|table| table.entries.iter())
        .filter(|entry| entry.class_id == 214 && entry.related_entity_id.is_some())
        .count();
    if predecessors != outputs.len() {
        return None;
    }

    let mut output_ids = BTreeSet::new();
    let mut intermediate_ids = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    let mut transitions = Vec::with_capacity(outputs.len());
    for (output_table, output) in outputs {
        let intermediate_id = output.related_entity_id?;
        if output.related_entity_state != Some(0)
            || output_table
                .surface_ids
                .iter()
                .filter(|surface_id| **surface_id == output.entity_id)
                .count()
                != 1
            || crate::surface::unique_surface_row(surface_rows, output.entity_id)
                .is_none_or(|row| row.feature_id != feature_id)
            || !output_ids.insert(output.entity_id)
            || !intermediate_ids.insert(intermediate_id)
        {
            return None;
        }
        let mut matches = output_table.entries.iter().filter(|predecessor| {
            predecessor.class_id == 214
                && predecessor.entity_id == intermediate_id
                && predecessor.related_entity_state == Some(0)
                && output_table
                    .non_surface_entity_ids
                    .contains(&predecessor.entity_id)
                && crate::surface::unique_surface_row(surface_rows, predecessor.entity_id).is_none()
        });
        let predecessor = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let source_id = predecessor.related_entity_id?;
        if crate::surface::unique_surface_row(surface_rows, source_id)
            .is_none_or(|row| row.feature_id == feature_id)
            || !source_ids.insert(source_id)
        {
            return None;
        }
        transitions.push((source_id, output.entity_id));
    }
    output_ids.is_disjoint(&source_ids).then_some(transitions)
}

pub(in super::super) fn surface_transition_dependencies(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Vec<u32> {
    feature_surface_transitions(feature_id, tables, surface_rows)
        .into_iter()
        .flatten()
        .filter_map(|(source_id, _)| {
            crate::surface::unique_surface_row(surface_rows, source_id).map(|row| row.feature_id)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(in super::super) fn thicken_plane_offset(
    transitions: &[(u32, u32)],
    planes: &BTreeMap<u32, PlaneEquation>,
    rows: &[crate::surface::SurfaceRow],
) -> Option<(f64, ThickenSide)> {
    let mut offsets = Vec::new();
    for &(source_id, output_id) in transitions {
        let (Some(source), Some(output)) = (planes.get(&source_id), planes.get(&output_id)) else {
            continue;
        };
        let source_row = crate::surface::unique_surface_row(rows, source_id)?;
        let output_row = crate::surface::unique_surface_row(rows, output_id)?;
        (source_row.reversed != output_row.reversed).then_some(())?;
        let source_normal = normalized(source.normal)?.map(|component| {
            if source_row.reversed {
                -component
            } else {
                component
            }
        });
        let output_normal = normalized(output.normal)?;
        if dot(source_normal, output_normal).abs() < 1.0 - EPS_NORMAL_ALIGNMENT {
            return None;
        }
        let displacement = std::array::from_fn(|index| output.origin[index] - source.origin[index]);
        offsets.push(dot(displacement, source_normal));
    }
    let magnitude = unique_positive_length(
        &offsets
            .iter()
            .map(|offset| offset.abs())
            .collect::<Vec<_>>(),
    )?;
    let tolerance = EPS_OFFSET_AGREEMENT * magnitude.max(1.0);
    let side = if offsets
        .iter()
        .all(|offset| (*offset - magnitude).abs() <= tolerance)
    {
        ThickenSide::Forward
    } else if offsets
        .iter()
        .all(|offset| (*offset + magnitude).abs() <= tolerance)
    {
        ThickenSide::Reverse
    } else {
        return None;
    };
    Some((magnitude, side))
}

/// Return the materialized surface identities that one feature can expose as
/// faces in its regenerated result.
///
/// Every materialized surface in an owned generated-entity table is a
/// result-face identity when its surface row is unique and names the same
/// owning feature. Duplicate identifiers or malformed materialized rows
/// invalidate the complete result state for that feature.
pub(in super::super) fn feature_result_surface_ids(
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
) -> Option<Vec<u32>> {
    let mut surface_ids = Vec::new();
    let mut seen = BTreeSet::new();
    for table in tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
    {
        for &surface_id in &table.surface_ids {
            let row = crate::surface::unique_surface_row(rows, surface_id)?;
            if row.feature_id != feature_id || !seen.insert(surface_id) {
                return None;
            }
            surface_ids.push(surface_id);
        }
    }
    (!surface_ids.is_empty()).then_some(surface_ids)
}

pub(in super::super) fn feature_result_surface_ids_by_feature(
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> BTreeMap<u32, Vec<u32>> {
    tables
        .iter()
        .filter_map(|table| table.feature_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|feature_id| {
            feature_result_surface_ids(tables, rows, feature_id)
                .map(|surface_ids| (feature_id, surface_ids))
        })
        .collect()
}

pub(in super::super) fn feature_result_topology(
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
    curve_rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Option<FeatureResultTopology> {
    let faces = feature_result_surface_ids(tables, surface_rows, feature_id)
        .unwrap_or_default()
        .into_iter()
        .map(|surface_id| format!("surface#{surface_id}"))
        .collect::<Vec<_>>();
    let edges = feature_result_edge_ids(curve_rows, feature_id)
        .unwrap_or_default()
        .into_iter()
        .map(|curve_id| format!("curve#{curve_id}"))
        .collect::<Vec<_>>();
    (!faces.is_empty() || !edges.is_empty()).then_some(())?;
    Some(FeatureResultTopology {
        id: FeatureResultTopologyId::mint(format!(
            "creo:model:feature-result-topology#{feature_id}"
        ))
        .expect("identity grammar"),
        output_of: IrFeatureId(format!("creo:model:feature#{feature_id}")),
        bodies: Vec::new(),
        faces,
        edges,
        vertices: Vec::new(),
        native_ref: None,
    })
}

pub(in super::super) fn generated_surface_face_refs(
    source_ids: &[u32],
    rows: &[crate::surface::SurfaceRow],
    result_surface_ids: &BTreeMap<u32, Vec<u32>>,
    available_features: &BTreeSet<IrFeatureId>,
) -> Option<Vec<GeneratedFaceRef>> {
    source_ids
        .iter()
        .map(|surface_id| {
            let row = crate::surface::unique_surface_row(rows, *surface_id)?;
            let feature = IrFeatureId(format!("creo:model:feature#{}", row.feature_id));
            (available_features.contains(&feature)
                && result_surface_ids
                    .get(&row.feature_id)
                    .is_some_and(|ids| ids.contains(surface_id)))
            .then_some(GeneratedFaceRef {
                feature,
                local_id: format!("surface#{surface_id}"),
            })
        })
        .collect()
}

pub(in super::super) fn emit_feature_result_topologies(
    scan: &ContainerScan,
    ir: &mut CadIr,
) -> usize {
    let mut emitted = 0;
    for feature in &ir.model.features {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(state) = feature_result_topology(
            &scan.features.entity_tables,
            &scan.surfaces.rows,
            &scan.curves.topology_rows,
            feature_id,
        ) else {
            continue;
        };
        ir.model.feature_result_topologies.push(state);
        emitted += 1;
    }
    emitted
}

#[cfg(test)]
mod tests;
