// SPDX-License-Identifier: Apache-2.0
//! Feature dependency graphs, affected ids, and link reconciliation.

use super::super::surfaces::prototypes::unique_surface_prototype_associations;
use super::knit::surface_transition_dependencies;
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::recipe::current_feature_recipe_parent;
use crate::feature::rows::agreed_feature_affected_ids;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, FeatureOperation as IrFeatureOperation,
};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn feature_dependencies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) -> Result<Vec<IrFeatureId>, CodecError> {
    let native = native_feature_dependency_ids(
        ctx,
        &scan.features.affected_ids,
        &scan.features.operations,
        &scan.features.entity_tables,
        &scan.features.surface_merge_replay_affected_ids,
        &scan.surfaces.rows,
        feature_id,
        prototype_dependencies
            .get(&feature_id)
            .map_or(&[], Vec::as_slice),
    )?;
    let mut dependencies = Vec::new();
    for dependency in native {
        let text = ctx.format_retained(
            format_args!("creo:model:feature#{dependency}"),
            "creo feature dependency IDs",
        )?;
        let id = IrFeatureId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        if ir.model
            .features
            .iter()
            .any(|feature| feature.id == id)
        {
            ctx.try_reserve_items(&mut dependencies, 1, "creo feature dependencies")?;
            dependencies.push(id);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn native_feature_dependency_ids(
    ctx: &DecodeContext<'_>,
    affected_ids: &[crate::feature::rows::FeatureAffectedIds],
    operations: &[crate::feature::operations::FeatureOperation],
    entity_tables: &[crate::feature::entity::FeatureEntityTable],
    surface_merge_replay_affected_ids: &[crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    prototype_dependencies: &[u32],
) -> Result<Vec<u32>, CodecError> {
    let transition_dependencies = surface_transition_dependencies(
        ctx,
        feature_id,
        entity_tables,
        surface_rows,
    )?;
    let parents = agreed_feature_parent_ids(ctx, affected_ids, feature_id)?;
    let merged = surface_merge_entity_dependencies(
        ctx,
        affected_ids,
        surface_merge_replay_affected_ids,
        entity_tables,
        feature_id,
    )?;
    let entity_dependencies = feature_entity_dependencies(ctx, entity_tables, feature_id)?;
    let surface_dependencies =
        feature_output_surface_dependencies(ctx, entity_tables, surface_rows, feature_id)?;
    let mut dependencies = Vec::new();
    for dependency in parents
        .into_iter()
        .chain(current_feature_recipe_parent(operations, feature_id))
        .chain(prototype_dependencies.iter().copied())
        .chain(merged)
        .chain(entity_dependencies)
        .chain(surface_dependencies)
        .chain(transition_dependencies)
    {
        if !dependencies.contains(&dependency) {
            ctx.try_reserve_items(&mut dependencies, 1, "creo native feature dependencies")?;
            dependencies.push(dependency);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn feature_output_surface_dependencies(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
) -> Result<Vec<u32>, CodecError> {
    let mut owned_entities = BTreeSet::new();
    for entry in tables
        .iter()
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 67)
        .flat_map(|table| &table.entries)
        .filter(|entry| entry.source_entity_id() == Some(feature_id))
    {
        if !owned_entities.contains(&entry.entity_id) {
            ctx.charge_collection_items(1, "creo output surface owned entity nodes")?;
            owned_entities.insert(entry.entity_id);
        }
    }
    let mut dependencies = Vec::new();
    for entry in tables
        .iter()
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 100)
        .flat_map(|table| &table.entries)
        .filter(|entry| owned_entities.contains(&entry.entity_id))
    {
        let Some(row) = crate::surface::unique_surface_row(surface_rows, entry.class_id()) else {
            continue;
        };
        if row.feature_id != feature_id && !dependencies.contains(&row.feature_id) {
            ctx.try_reserve_items(&mut dependencies, 1, "creo output surface dependencies")?;
            dependencies.push(row.feature_id);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn feature_entity_dependencies(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    feature_id: u32,
) -> Result<Vec<u32>, CodecError> {
    let mut dependencies = Vec::new();
    for table in tables {
        if table.feature_id != feature_id || table.table_class_id != 100 {
            continue;
        }
        for entry in &table.entries {
            let Some(producer) = unique_feature_entity_producer(tables, entry.entity_id) else {
                continue;
            };
            if producer == feature_id {
                continue;
            }
            if !dependencies.contains(&producer) {
                ctx.try_reserve_items(&mut dependencies, 1, "creo feature entity dependencies")?;
                dependencies.push(producer);
            }
        }
    }
    Ok(dependencies)
}

fn unique_feature_entity_producer(
    tables: &[crate::feature::entity::FeatureEntityTable],
    entity_id: u32,
) -> Option<u32> {
    let mut producer = None;
    for table in tables {
        if table
            .entries
            .iter()
            .any(|entry| entry.class_id() == 200 && entry.entity_id == entity_id)
        {
            match producer {
                Some(owner) if owner != table.feature_id => return None,
                None => producer = Some(table.feature_id),
                _ => {}
            }
        }
    }
    producer
}

fn unique_preceding_feature_entity_producer(
    tables: &[crate::feature::entity::FeatureEntityTable],
    entity_id: u32,
    consumer_offset: usize,
) -> Option<u32> {
    let mut producer = None;
    for table in tables {
        for entry in &table.entries {
            if entry.class_id() == 200
                && entry.entity_id == entity_id
                && entry.offset < consumer_offset
            {
                if producer.is_some() {
                    return None;
                }
                producer = Some(table.feature_id);
            }
        }
    }
    producer
}

fn agreed_surface_merge_replay_quilt_ids(
    records: &[crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    agreed_ids(
        records
            .iter()
            .filter(|record| record.feature_id == feature_id)
            .map(|record| record.quilt_ids.as_slice()),
    )
}

pub(in super::super) fn surface_merge_quilt_ids<'a>(
    affected_ids: &'a [crate::feature::rows::FeatureAffectedIds],
    replay: &'a [crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Option<&'a [u32]> {
    if let Some(ids) = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Quilts,
    ) {
        return (!ids.is_empty()).then_some(ids);
    }
    if has_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Quilts,
    ) {
        return None;
    }
    agreed_surface_merge_replay_quilt_ids(replay, feature_id).filter(|ids| !ids.is_empty())
}

pub(super) fn surface_merge_quilt_state_offset(
    affected_ids: &[crate::feature::rows::FeatureAffectedIds],
    replay: &[crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
    quilt_ids: &[u32],
) -> Option<usize> {
    if let Some(ids) = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Quilts,
    ) {
        return (ids == quilt_ids)
            .then(|| {
                affected_ids
                    .iter()
                    .filter(|record| {
                        record.feature_id == feature_id
                            && record.kind == crate::feature::rows::AffectedIdKind::Quilts
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
        crate::feature::rows::AffectedIdKind::Quilts,
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
    ctx: &DecodeContext<'_>,
    affected_ids: &[crate::feature::rows::FeatureAffectedIds],
    replay: &[crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    tables: &[crate::feature::entity::FeatureEntityTable],
    feature_id: u32,
) -> Result<Vec<u32>, CodecError> {
    let Some(ids) = surface_merge_quilt_ids(affected_ids, replay, feature_id) else {
        return Ok(Vec::new());
    };
    let Some(consumer_offset) =
        surface_merge_quilt_state_offset(affected_ids, replay, feature_id, ids)
    else {
        return Ok(Vec::new());
    };
    let mut dependencies = Vec::new();
    for &entity_id in ids {
        let Some(owner) =
            unique_preceding_feature_entity_producer(tables, entity_id, consumer_offset)
        else {
            continue;
        };
        if owner != feature_id && !dependencies.contains(&owner) {
            ctx.try_reserve_items(&mut dependencies, 1, "creo surface merge dependencies")?;
            dependencies.push(owner);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn has_feature_affected_ids(
    records: &[crate::feature::rows::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::rows::AffectedIdKind,
) -> bool {
    records
        .iter()
        .any(|record| record.feature_id == feature_id && record.kind == kind)
}

fn agreed_feature_parent_ids(
    ctx: &DecodeContext<'_>,
    records: &[crate::feature::rows::FeatureAffectedIds],
    feature_id: u32,
) -> Result<Vec<u32>, CodecError> {
    let mut strong_emitted = false;
    let mut parent_emitted = false;
    let mut ids = Vec::new();
    for record in records.iter().filter(|record| {
        record.feature_id == feature_id
            && matches!(
                record.kind,
                crate::feature::rows::AffectedIdKind::StrongParents
                    | crate::feature::rows::AffectedIdKind::Parents
            )
    }) {
        let emitted = match record.kind {
            crate::feature::rows::AffectedIdKind::StrongParents => &mut strong_emitted,
            crate::feature::rows::AffectedIdKind::Parents => &mut parent_emitted,
            _ => continue,
        };
        if *emitted {
            continue;
        }
        *emitted = true;
        if let Some(agreed) = agreed_feature_affected_ids(records, feature_id, record.kind) {
            ctx.try_reserve_items(&mut ids, agreed.len(), "creo agreed feature parent IDs")?;
            ids.extend_from_slice(agreed);
        }
    }
    Ok(ids)
}

pub(in super::super) fn surface_prototype_feature_dependencies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<BTreeMap<u32, Vec<u32>>, cadmpeg_core::CodecError> {
    let mut dependencies = BTreeMap::new();
    for (prototype, row, _) in unique_surface_prototype_associations(ctx, scan)? {
        let prototype = prototype.record();
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
        add_surface_prototype_feature_dependencies(
            ctx,
            &mut dependencies,
            row.feature_id,
            consumers,
        )?;
    }
    Ok(dependencies)
}

pub(in super::super) fn add_surface_prototype_feature_dependencies(
    ctx: &DecodeContext<'_>,
    dependencies: &mut BTreeMap<u32, Vec<u32>>,
    producer: u32,
    consumers: &[u32],
) -> Result<(), CodecError> {
    for &consumer in consumers {
        if consumer == 0 || consumer == producer {
            continue;
        }
        let producers = match dependencies.entry(consumer) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                ctx.charge_collection_items(1, "creo prototype dependency consumers")?;
                entry.insert(Vec::new())
            }
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        };
        if !producers.contains(&producer) {
            ctx.try_reserve_items(producers, 1, "creo prototype dependency producers")?;
            producers.push(producer);
        }
    }
    Ok(())
}

pub(in super::super) fn agreed_feature_replay_geometry_ids(
    records: &[crate::feature::rows::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    agreed_ids(
        records
            .iter()
            .filter(|record| record.feature_id == feature_id)
            .map(|record| record.geometry_ids.as_slice()),
    )
}

pub(in super::super) fn agreed_feature_replay_edge_ids(
    records: &[crate::feature::rows::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Option<&[u32]> {
    agreed_ids(
        records
            .iter()
            .filter(|record| record.feature_id == feature_id)
            .map(|record| record.edge_ids.as_slice()),
    )
}

pub(in super::super) fn reconcile_feature_links(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
    prototype_dependencies: &BTreeMap<u32, Vec<u32>>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut output_updates = Vec::new();
    for (index, feature) in ir.model.features.iter().enumerate() {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let outputs = cadmpeg_ir::features::DistinctMembers::try_from_reserved_vec(
            super::outputs::feature_output_bodies(ctx, scan, ir, feature_id)?,
        )
        .map_err(cadmpeg_core::CodecError::malformed)?;
        ctx.try_reserve_items(&mut output_updates, 1, "creo reconciled output update rows")?;
        output_updates.push((index, outputs));
    }
    let mut emitted = BTreeSet::new();
    for feature in &ir.model.features {
        if emitted.contains(&feature.id) {
            continue;
        }
        let id = IrFeatureId::mint(ctx.copy_retained_text(
            feature.id.as_str(),
            "creo emitted feature identity text",
        )?)
        .map_err(cadmpeg_core::CodecError::malformed)?;
        ctx.charge_collection_items(1, "creo emitted feature identity nodes")?;
        emitted.insert(id);
    }
    let mut regeneration_edges = Vec::new();
    let mut updates = output_updates.into_iter();
    let mut pending = updates.next();
    for (index, feature) in ir.model.features.iter_mut().enumerate() {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if pending.as_ref().is_some_and(|(update_index, _)| *update_index == index) {
            if let Some((_, outputs)) = pending.take() {
                feature.evaluation.set_outputs(outputs);
            }
            pending = updates.next();
        }
        let mut native_dependencies = Vec::new();
        for dependency in native_feature_dependency_ids(
            ctx,
            &scan.features.affected_ids,
            &scan.features.operations,
            &scan.features.entity_tables,
            &scan.features.surface_merge_replay_affected_ids,
            &scan.surfaces.rows,
            feature_id,
            prototype_dependencies
                .get(&feature_id)
                .map_or(&[], Vec::as_slice),
        )? {
            let text = ctx.format_retained(
                format_args!("creo:model:feature#{dependency}"),
                "creo reconciled native dependency IDs",
            )?;
            let id = IrFeatureId::mint(text)
                .map_err(cadmpeg_core::CodecError::malformed)?;
            if emitted.contains(&id) && id != feature.id {
                ctx.try_reserve_items(
                    &mut native_dependencies,
                    1,
                    "creo reconciled native dependencies",
                )?;
                native_dependencies.push(id);
            }
        }
        let generated_dependencies =
            feature_generated_dependencies(ctx, feature.evaluation.definition())?;
        let mut generated_ids = Vec::new();
        for dependency in generated_dependencies {
            let id = IrFeatureId::mint(ctx.copy_retained_text(
                dependency.as_str(),
                "creo reconciled generated dependency IDs",
            )?)
            .map_err(cadmpeg_core::CodecError::malformed)?;
            ctx.try_reserve_items(
                &mut generated_ids,
                1,
                "creo reconciled generated dependencies",
            )?;
            generated_ids.push(id);
        }
        feature.dependencies = cadmpeg_ir::features::DistinctMembers::try_from_reserved_vec(
            reconciled_dependencies(
            ctx,
            &feature.id,
            &feature.dependencies,
            native_dependencies.into_iter().chain(generated_ids),
            &emitted,
        )?)
        .map_err(cadmpeg_core::CodecError::malformed)?;
        if let Some(parent_id) = current_feature_recipe_parent(&scan.features.operations, feature_id) {
            let text = ctx.format_retained(
                format_args!("creo:model:feature#{parent_id}"),
                "creo regeneration parent IDs",
            )?;
            let parent = IrFeatureId::mint(text)
                .map_err(cadmpeg_core::CodecError::malformed)?;
            if parent != feature.id && emitted.contains(&parent) {
                let child = IrFeatureId::mint(ctx.copy_retained_text(
                    feature.id.as_str(),
                    "creo regeneration child IDs",
                )?)
                .map_err(cadmpeg_core::CodecError::malformed)?;
                ctx.try_reserve_items(&mut regeneration_edges, 1, "creo regeneration edges")?;
                regeneration_edges.push((child, parent));
            }
        }
    }
    for (child, parent) in regeneration_edges {
        if ir.model.feature_regeneration_parent(&child).is_none() {
            ctx.charge_collection_items(1, "creo regeneration parent nodes")?;
        }
        ir.model
            .set_feature_regeneration_parent(child, parent)
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    let mut remaining = Vec::new();
    ctx.try_reserve_items(&mut remaining, ir.model.features.len(), "creo remaining feature order")?;
    remaining.extend(0..ir.model.features.len());
    let mut ordered = Vec::new();
    ctx.try_reserve_items(&mut ordered, remaining.len(), "creo ordered feature indices")?;
    let mut preceding = BTreeSet::new();
    while !remaining.is_empty() {
        let Some(position) = remaining.iter().position(|index| {
            let feature = &ir.model.features[*index];
            feature
                .dependencies
                .iter()
                .chain(ir.model.feature_parent(&feature.id))
                .all(|required| !emitted.contains(required) || preceding.contains(required))
        }) else {
            break;
        };
        let index = remaining.remove(position);
        if !preceding.contains(&ir.model.features[index].id) {
            ctx.charge_collection_items(1, "creo preceding feature identity nodes")?;
            preceding.insert(&ir.model.features[index].id);
        }
        ordered.push(index);
    }
    ordered.extend(remaining);
    for (ordinal, index) in ordered.into_iter().enumerate() {
        ir.model.features[index].ordinal = ordinal as u64;
    }
    Ok(())
}

pub(in super::super) fn feature_generated_dependencies<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    definition: &'a IrFeatureDefinition,
) -> Result<Vec<&'a IrFeatureId>, cadmpeg_core::CodecError> {
    let mut dependencies = Vec::new();
    let mut push_unique = |dependency: &'a IrFeatureId| -> Result<(), cadmpeg_core::CodecError> {
        if !dependencies.contains(&dependency) {
            ctx.try_reserve_items(&mut dependencies, 1, "creo generated dependencies")?;
            dependencies.push(dependency);
        }
        Ok(())
    };
    let face_selection = match definition {
        IrFeatureDefinition::Operation(
            IrFeatureOperation::Hole {
                face: Some(face), ..
            }
            | IrFeatureOperation::Thicken { faces: face, .. }
            | IrFeatureOperation::KnitSurface { faces: face, .. },
        ) => Some(face),
        _ => None,
    };
    if let Some(FaceSelection::Generated { faces, .. }) = face_selection {
        for face in faces {
            push_unique(&face.feature)?;
        }
    }
    let mut visit_selection = |selection: &'a EdgeSelection| -> Result<(), cadmpeg_core::CodecError> {
        if let EdgeSelection::Generated { edges, .. } = selection {
            for edge in edges {
                push_unique(&edge.feature)?;
            }
        }
        Ok(())
    };
    match definition {
        IrFeatureDefinition::Operation(IrFeatureOperation::Fillet { groups }) => {
            for group in groups {
                visit_selection(&group.edges)?;
            }
        }
        IrFeatureDefinition::Operation(IrFeatureOperation::Chamfer { groups, .. }) => {
            for group in groups {
                visit_selection(&group.edges)?;
            }
        }
        _ => {}
    }
    Ok(dependencies)
}

pub(in super::super) fn reconciled_dependencies(
    ctx: &DecodeContext<'_>,
    feature_id: &IrFeatureId,
    established: &[IrFeatureId],
    native: impl IntoIterator<Item = IrFeatureId>,
    emitted: &BTreeSet<IrFeatureId>,
) -> Result<Vec<IrFeatureId>, CodecError> {
    let mut dependencies = Vec::new();
    for dependency in established {
        if !emitted.contains(dependency)
            || dependency == feature_id
            || dependencies.contains(dependency)
        {
            continue;
        }
        let id = IrFeatureId::mint(ctx.copy_retained_text(
            dependency.as_str(),
            "creo established dependency IDs",
        )?)
        .map_err(CodecError::malformed)?;
        ctx.try_reserve_items(&mut dependencies, 1, "creo reconciled dependencies")?;
        dependencies.push(id);
    }
    for dependency in native {
        if emitted.contains(&dependency)
            && dependency != *feature_id
            && !dependencies.contains(&dependency)
        {
            ctx.try_reserve_items(&mut dependencies, 1, "creo reconciled dependencies")?;
            dependencies.push(dependency);
        }
    }
    Ok(dependencies)
}

fn agreed_ids<'a>(mut values: impl Iterator<Item = &'a [u32]>) -> Option<&'a [u32]> {
    let first = values.next()?;
    values.all(|value| value == first).then_some(first)
}

#[cfg(test)]
mod tests;
