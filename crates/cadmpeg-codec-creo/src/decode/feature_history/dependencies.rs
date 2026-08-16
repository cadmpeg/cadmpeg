// SPDX-License-Identifier: Apache-2.0
//! Feature dependency graphs, affected ids, and link reconciliation.

use super::super::sketch_transfer::current_feature_recipe_parent;
use super::super::surfaces::unique_surface_prototype_associations;
use super::surface_transition_dependencies;
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId,
};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn feature_dependencies(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) -> Vec<IrFeatureId> {
    native_feature_dependency_ids(
        &scan.features.affected_ids,
        &scan.features.operations,
        &scan.features.entity_tables,
        &scan.features.surface_merge_replay_affected_ids,
        &scan.surfaces.rows,
        feature_id,
        prototype_dependencies
            .get(&feature_id)
            .map_or(&[], Vec::as_slice),
    )
    .into_iter()
    .filter_map(|dependency| {
        let id = IrFeatureId(format!("creo:model:feature#{dependency}"));
        ir.model
            .features
            .iter()
            .any(|feature| feature.id == id)
            .then_some(id)
    })
    .collect()
}

pub(in super::super) fn native_feature_dependency_ids(
    affected_ids: &[crate::feature::FeatureAffectedIds],
    operations: &[crate::feature::FeatureOperation],
    entity_tables: &[crate::feature::FeatureEntityTable],
    surface_merge_replay_affected_ids: &[crate::feature::FeatureSurfaceMergeAffectedIds],
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    prototype_dependencies: &[u32],
) -> Vec<u32> {
    agreed_feature_parent_ids(affected_ids, feature_id)
        .into_iter()
        .chain(current_feature_recipe_parent(operations, feature_id))
        .chain(prototype_dependencies.iter().copied())
        .chain(surface_merge_entity_dependencies(
            affected_ids,
            surface_merge_replay_affected_ids,
            entity_tables,
            feature_id,
        ))
        .chain(feature_entity_dependencies(entity_tables, feature_id))
        .chain(feature_output_surface_dependencies(
            entity_tables,
            surface_rows,
            feature_id,
        ))
        .chain(surface_transition_dependencies(
            feature_id,
            entity_tables,
            surface_rows,
        ))
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(in super::super) fn feature_output_surface_dependencies(
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
) -> Vec<u32> {
    let owned_entities = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 67)
        .flat_map(|table| &table.entries)
        .filter(|entry| entry.class_id == 200 && entry.source_entity_id == Some(feature_id))
        .map(|entry| entry.entity_id)
        .collect::<BTreeSet<_>>();
    tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 100)
        .flat_map(|table| &table.entries)
        .filter(|entry| owned_entities.contains(&entry.entity_id))
        .filter_map(|entry| {
            let row = crate::surface::unique_surface_row(surface_rows, entry.class_id)?;
            (row.feature_id != feature_id).then_some(row.feature_id)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(in super::super) fn feature_entity_dependencies(
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
) -> Vec<u32> {
    let mut dependencies = Vec::new();
    for (table_index, table) in tables.iter().enumerate() {
        if table.feature_id != Some(feature_id) || table.table_class_id != 100 {
            continue;
        }
        for (entry_index, entry) in table.entries.iter().enumerate() {
            let consumer_position = (table.offset, entry.offset, table_index, entry_index);
            let producers = tables
                .iter()
                .enumerate()
                .flat_map(|(producer_table_index, producer_table)| {
                    let Some(producer_feature_id) = producer_table.feature_id else {
                        return Vec::new();
                    };
                    if producer_feature_id == feature_id {
                        return Vec::new();
                    }
                    producer_table
                        .entries
                        .iter()
                        .enumerate()
                        .filter_map(|(producer_entry_index, producer_entry)| {
                            let producer_position = (
                                producer_table.offset,
                                producer_entry.offset,
                                producer_table_index,
                                producer_entry_index,
                            );
                            (producer_position < consumer_position
                                && producer_entry.class_id == 200
                                && producer_entry.entity_id == entry.entity_id
                                && producer_entry.source_entity_id.is_some())
                            .then_some(producer_feature_id)
                        })
                        .collect::<Vec<_>>()
                })
                .fold(Vec::new(), |mut producers, producer| {
                    if !producers.contains(&producer) {
                        producers.push(producer);
                    }
                    producers
                });
            let [producer] = producers.as_slice() else {
                continue;
            };
            if !dependencies.contains(producer) {
                dependencies.push(*producer);
            }
        }
    }
    dependencies
}

pub(in super::super) fn preceding_feature_entity_producers(
    tables: &[crate::feature::FeatureEntityTable],
    entity_id: u32,
    consumer_offset: usize,
) -> Vec<u32> {
    tables
        .iter()
        .filter_map(|table| table.feature_id.map(|owner| (owner, table)))
        .flat_map(|(owner, table)| {
            table.entries.iter().filter_map(move |entry| {
                (entry.class_id == 200
                    && entry.entity_id == entity_id
                    && entry.source_entity_id.is_some()
                    && entry.offset < consumer_offset)
                    .then_some(owner)
            })
        })
        .collect()
}

pub(in super::super) fn agreed_surface_merge_replay_quilt_ids(
    records: &[crate::feature::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.quilt_ids.as_slice();
    matches
        .all(|record| record.quilt_ids.as_slice() == ids)
        .then_some(ids)
}

pub(in super::super) fn surface_merge_quilt_ids<'a>(
    affected_ids: &'a [crate::feature::FeatureAffectedIds],
    replay: &'a [crate::feature::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Option<&'a [u32]> {
    if let Some(ids) = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Quilts,
    ) {
        return (!ids.is_empty()).then_some(ids);
    }
    if has_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Quilts,
    ) {
        return None;
    }
    agreed_surface_merge_replay_quilt_ids(replay, feature_id).filter(|ids| !ids.is_empty())
}

pub(in super::super) fn surface_merge_quilt_state_offset(
    affected_ids: &[crate::feature::FeatureAffectedIds],
    replay: &[crate::feature::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
    quilt_ids: &[u32],
) -> Option<usize> {
    if let Some(ids) = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Quilts,
    ) {
        return (ids == quilt_ids)
            .then(|| {
                affected_ids
                    .iter()
                    .filter(|record| {
                        record.feature_id == feature_id
                            && record.kind == crate::feature::AffectedIdKind::Quilts
                            && record.ids == quilt_ids
                    })
                    .map(|record| record.offset)
                    .min()
            })
            .flatten();
    }
    if has_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::AffectedIdKind::Quilts,
    ) {
        return None;
    }
    let ids = agreed_surface_merge_replay_quilt_ids(replay, feature_id)?;
    (ids == quilt_ids).then(|| {
        replay
            .iter()
            .filter(|record| record.feature_id == feature_id && record.quilt_ids == quilt_ids)
            .map(|record| record.offset)
            .min()
    })?
}

pub(in super::super) fn surface_merge_entity_dependencies(
    affected_ids: &[crate::feature::FeatureAffectedIds],
    replay: &[crate::feature::FeatureSurfaceMergeAffectedIds],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
) -> Vec<u32> {
    let Some(ids) = surface_merge_quilt_ids(affected_ids, replay, feature_id) else {
        return Vec::new();
    };
    let Some(consumer_offset) =
        surface_merge_quilt_state_offset(affected_ids, replay, feature_id, ids)
    else {
        return Vec::new();
    };
    ids.iter()
        .filter_map(|entity_id| {
            let producers = preceding_feature_entity_producers(tables, *entity_id, consumer_offset);
            let [owner] = producers.as_slice() else {
                return None;
            };
            (*owner != feature_id).then_some(*owner)
        })
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(in super::super) fn agreed_feature_affected_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::AffectedIdKind,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id && record.kind == kind);
    let ids = matches.next()?.ids.as_slice();
    matches
        .all(|record| record.ids.as_slice() == ids)
        .then_some(ids)
}

pub(in super::super) fn has_feature_affected_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::AffectedIdKind,
) -> bool {
    records
        .iter()
        .any(|record| record.feature_id == feature_id && record.kind == kind)
}

pub(in super::super) fn agreed_feature_parent_ids(
    records: &[crate::feature::FeatureAffectedIds],
    feature_id: u32,
) -> Vec<u32> {
    let mut emitted_kinds = Vec::new();
    let mut ids = Vec::new();
    for record in records.iter().filter(|record| {
        record.feature_id == feature_id
            && matches!(
                record.kind,
                crate::feature::AffectedIdKind::StrongParents
                    | crate::feature::AffectedIdKind::Parents
            )
    }) {
        if emitted_kinds.contains(&record.kind) {
            continue;
        }
        emitted_kinds.push(record.kind);
        if let Some(agreed) = agreed_feature_affected_ids(records, feature_id, record.kind) {
            ids.extend_from_slice(agreed);
        }
    }
    ids
}

pub(in super::super) fn surface_prototype_feature_dependencies(
    scan: &ContainerScan,
) -> BTreeMap<u32, Vec<u32>> {
    let mut dependencies = BTreeMap::new();
    for (prototype, row, _) in unique_surface_prototype_associations(scan) {
        let mut fields = prototype
            .parameters
            .iter()
            .filter(|field| field.name == "parent_feats");
        let Some(field) = fields.next() else {
            continue;
        };
        if fields.next().is_some() {
            continue;
        }
        let crate::surface::SurfaceNamedValue::CompactIntArray(consumers) = &field.value else {
            continue;
        };
        add_surface_prototype_feature_dependencies(&mut dependencies, row.feature_id, consumers);
    }
    dependencies
}

pub(in super::super) fn add_surface_prototype_feature_dependencies(
    dependencies: &mut BTreeMap<u32, Vec<u32>>,
    producer: u32,
    consumers: &[u32],
) {
    for &consumer in consumers {
        if consumer == 0 || consumer == producer {
            continue;
        }
        let producers = dependencies.entry(consumer).or_default();
        if !producers.contains(&producer) {
            producers.push(producer);
        }
    }
}

pub(in super::super) fn agreed_feature_replay_geometry_ids(
    records: &[crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.geometry_ids.as_slice();
    matches
        .all(|record| record.geometry_ids.as_slice() == ids)
        .then_some(ids)
}

pub(in super::super) fn agreed_feature_replay_edge_ids(
    records: &[crate::feature::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    let mut matches = records
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let ids = matches.next()?.edge_ids.as_slice();
    matches
        .all(|record| record.edge_ids.as_slice() == ids)
        .then_some(ids)
}

pub(in super::super) fn reconcile_feature_links(
    scan: &ContainerScan,
    ir: &mut CadIr,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) {
    let emitted = ir
        .model
        .features
        .iter()
        .map(|feature| feature.id.clone())
        .collect::<BTreeSet<_>>();
    for feature in &mut ir.model.features {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let native_dependencies = native_feature_dependency_ids(
            &scan.features.affected_ids,
            &scan.features.operations,
            &scan.features.entity_tables,
            &scan.features.surface_merge_replay_affected_ids,
            &scan.surfaces.rows,
            feature_id,
            prototype_dependencies
                .get(&feature_id)
                .map_or(&[], Vec::as_slice),
        )
        .into_iter()
        .map(|dependency| IrFeatureId(format!("creo:model:feature#{dependency}")))
        .filter(|dependency| emitted.contains(dependency))
        .filter(|dependency| *dependency != feature.id);
        let generated_dependencies = feature_generated_dependencies(&feature.definition);
        feature.dependencies = reconciled_dependencies(
            &feature.id,
            &feature.dependencies,
            native_dependencies.chain(generated_dependencies),
            &emitted,
        );
        if feature.parent.is_none() {
            feature.parent = current_feature_recipe_parent(&scan.features.operations, feature_id)
                .map(|parent| IrFeatureId(format!("creo:model:feature#{parent}")))
                .filter(|parent| *parent != feature.id && emitted.contains(parent));
        }
    }
    let mut remaining = (0..ir.model.features.len()).collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(remaining.len());
    let mut preceding = BTreeSet::new();
    while !remaining.is_empty() {
        let Some(position) = remaining.iter().position(|index| {
            let feature = &ir.model.features[*index];
            feature
                .dependencies
                .iter()
                .chain(feature.parent.iter())
                .all(|required| !emitted.contains(required) || preceding.contains(required))
        }) else {
            break;
        };
        let index = remaining.remove(position);
        preceding.insert(ir.model.features[index].id.clone());
        ordered.push(index);
    }
    ordered.extend(remaining);
    for (ordinal, index) in ordered.into_iter().enumerate() {
        ir.model.features[index].ordinal = ordinal as u64;
    }
}

pub(in super::super) fn feature_generated_dependencies(
    definition: &IrFeatureDefinition,
) -> Vec<IrFeatureId> {
    let face_selections = match definition {
        IrFeatureDefinition::Hole {
            face: Some(face), ..
        }
        | IrFeatureDefinition::Thicken { faces: face, .. }
        | IrFeatureDefinition::KnitSurface { faces: face, .. } => vec![face],
        _ => Vec::new(),
    };
    let edge_selections = match definition {
        IrFeatureDefinition::Fillet { groups } => {
            groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
        }
        IrFeatureDefinition::Chamfer { groups, .. } => {
            groups.iter().map(|group| &group.edges).collect::<Vec<_>>()
        }
        _ => Vec::new(),
    };
    face_selections
        .into_iter()
        .flat_map(|selection| match selection {
            FaceSelection::Generated { faces, .. } => faces
                .iter()
                .map(|face| face.feature.clone())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .chain(edge_selections.into_iter().flat_map(|selection| {
            match selection {
                EdgeSelection::Generated { edges, .. } => edges
                    .iter()
                    .map(|edge| edge.feature.clone())
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            }
        }))
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}

pub(in super::super) fn reconciled_dependencies(
    feature_id: &IrFeatureId,
    established: &[IrFeatureId],
    native: impl IntoIterator<Item = IrFeatureId>,
    emitted: &BTreeSet<IrFeatureId>,
) -> Vec<IrFeatureId> {
    established
        .iter()
        .cloned()
        .chain(native)
        .filter(|dependency| emitted.contains(dependency))
        .filter(|dependency| dependency != feature_id)
        .fold(Vec::new(), |mut dependencies, dependency| {
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
            dependencies
        })
}
