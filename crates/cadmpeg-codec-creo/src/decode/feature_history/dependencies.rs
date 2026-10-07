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
    let mut native_storage = ctx.reserve_scoped(0, "Creo native dependency lookup storage")?;
    let native = native_storage.with_storage(|| {
        native_feature_dependency_ids(
            ctx,
            &scan.features.affected_ids,
            &scan.features.operations,
            &scan.features.entity_tables,
            &scan.features.surface_merge_replay_affected_ids,
            &scan.surfaces.rows,
            (
                feature_id,
                prototype_dependencies
                    .get(&feature_id)
                    .map_or(&[], Vec::as_slice),
            ),
        )
    })?;
    let mut dependencies = Vec::new();
    for dependency in ctx.admit_iter(&native, "creo external feature dependency IDs")? {
        let text = ctx.format_retained(
            format_args!("creo:model:feature#{dependency}"),
            "creo feature dependency IDs",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(text.len()),
            "creo feature dependency identity validation",
        )?;
        let id = IrFeatureId::mint(text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        let mut feature_exists = false;
        for feature in ctx.admit_iter(&ir.model.features, "creo dependency feature lookup")? {
            if ctx.equal(
                &feature.id,
                &id,
                "creo dependency feature identity comparison",
            )? {
                feature_exists = true;
                break;
            }
        }
        if feature_exists {
            ctx.reserve_vec(&mut dependencies, 1, "creo feature dependencies")?;
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
    surface_rows: &crate::surface::SurfaceRows,
    feature: (u32, &[u32]),
) -> Result<Vec<u32>, CodecError> {
    let (feature_id, prototype_dependencies) = feature;
    let mut input_storage = ctx.reserve_scoped(0, "Creo dependency input vectors")?;

    let transition_dependencies = input_storage.with_storage(|| {
        surface_transition_dependencies(ctx, feature_id, entity_tables, surface_rows)
    })?;
    let parents =
        input_storage.with_storage(|| agreed_feature_parent_ids(ctx, affected_ids, feature_id))?;
    let merged = input_storage.with_storage(|| {
        surface_merge_entity_dependencies(
            ctx,
            affected_ids,
            surface_merge_replay_affected_ids,
            entity_tables,
            feature_id,
        )
    })?;
    let entity_dependencies = input_storage
        .with_storage(|| feature_entity_dependencies(ctx, entity_tables, feature_id))?;
    let surface_dependencies = input_storage.with_storage(|| {
        feature_output_surface_dependencies(ctx, entity_tables, surface_rows, feature_id)
    })?;
    let mut dependencies = Vec::new();
    let recipe_parent = current_feature_recipe_parent(operations, feature_id);
    for dependency in ctx
        .admit_iter(&parents, "creo native parent dependency IDs")?
        .copied()
        .chain(recipe_parent)
        .chain(
            ctx.admit_iter(prototype_dependencies, "creo prototype dependency IDs")?
                .copied(),
        )
        .chain(
            ctx.admit_iter(&merged, "creo surface merge dependency IDs")?
                .copied(),
        )
        .chain(
            ctx.admit_iter(&entity_dependencies, "creo entity dependency IDs")?
                .copied(),
        )
        .chain(
            ctx.admit_iter(&surface_dependencies, "creo output surface dependency IDs")?
                .copied(),
        )
        .chain(
            ctx.admit_iter(
                &transition_dependencies,
                "creo surface transition dependency IDs",
            )?
            .copied(),
        )
    {
        if !dependencies.contains(&dependency) {
            ctx.reserve_vec(&mut dependencies, 1, "creo native feature dependencies")?;
            dependencies.push(dependency);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn feature_output_surface_dependencies(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &crate::surface::SurfaceRows,
    feature_id: u32,
) -> Result<Vec<u32>, CodecError> {
    let mut owned_storage = ctx.reserve_scoped(0, "Creo owned dependency entities")?;
    let mut owned_entities = BTreeSet::new();
    for table in ctx.admit_iter(tables, "creo output surface ownership tables")? {
        if table.feature_id != feature_id || table.table_class_id != 67 {
            continue;
        }
        for entry in ctx.admit_iter(&table.entries, "creo output surface ownership entries")? {
            if entry.source_entity_id() != Some(feature_id) {
                continue;
            }
            owned_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut owned_entities,
                    entry.entity_id,
                    "creo output surface owned entity nodes",
                )
            })?;
        }
    }
    let mut dependencies = Vec::new();
    for table in ctx.admit_iter(tables, "creo output surface dependency tables")? {
        if table.feature_id != feature_id || table.table_class_id != 100 {
            continue;
        }
        for entry in ctx.admit_iter(&table.entries, "creo output surface dependency entries")? {
            if !owned_entities.contains(&entry.entity_id) {
                continue;
            }
            let Some(row) = crate::surface::unique_surface_row(surface_rows, entry.class_id())
            else {
                continue;
            };
            if row.feature_id != feature_id && !dependencies.contains(&row.feature_id) {
                ctx.reserve_vec(&mut dependencies, 1, "creo output surface dependencies")?;
                dependencies.push(row.feature_id);
            }
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
    for table in ctx.admit_iter(tables, "creo feature entity dependency tables")? {
        if table.feature_id != feature_id || table.table_class_id != 100 {
            continue;
        }
        for entry in ctx.admit_iter(&table.entries, "creo feature entity dependency entries")? {
            let Some(producer) = unique_feature_entity_producer(ctx, tables, entry.entity_id)?
            else {
                continue;
            };
            if producer == feature_id {
                continue;
            }
            if !dependencies.contains(&producer) {
                ctx.reserve_vec(&mut dependencies, 1, "creo feature entity dependencies")?;
                dependencies.push(producer);
            }
        }
    }
    Ok(dependencies)
}

fn unique_feature_entity_producer(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    entity_id: u32,
) -> Result<Option<u32>, CodecError> {
    let mut producer = None;
    for table in ctx.admit_iter(tables, "creo unique feature producer tables")? {
        if ctx.any_by(
            &table.entries,
            |entry| Ok(entry.class_id() == 200 && entry.entity_id == entity_id),
            "creo unique feature producer entries",
        )? {
            match producer {
                Some(owner) if owner != table.feature_id => return Ok(None),
                None => producer = Some(table.feature_id),
                _ => {}
            }
        }
    }
    Ok(producer)
}

fn unique_preceding_feature_entity_producer(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    entity_id: u32,
    consumer_offset: usize,
) -> Result<Option<u32>, CodecError> {
    let mut producer = None;
    for table in ctx.admit_iter(tables, "creo preceding feature producer tables")? {
        for entry in ctx.admit_iter(&table.entries, "creo preceding feature producer entries")? {
            if entry.class_id() == 200
                && entry.entity_id == entity_id
                && entry.offset < consumer_offset
            {
                if producer.is_some() {
                    return Ok(None);
                }
                producer = Some(table.feature_id);
            }
        }
    }
    Ok(producer)
}

fn agreed_surface_merge_replay_quilt_ids<'a>(
    ctx: &DecodeContext<'_>,
    records: &'a [crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Result<Option<&'a [u32]>, CodecError> {
    agreed_ids(
        ctx,
        ctx.admit_iter(records, "creo surface merge replay ID records")?
            .filter(|record| record.feature_id == feature_id)
            .map(|record| record.quilt_ids.as_slice()),
    )
}

pub(in super::super) fn surface_merge_quilt_ids<'a>(
    ctx: &DecodeContext<'_>,
    affected_ids: &'a [crate::feature::rows::FeatureAffectedIds],
    replay: &'a [crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
) -> Result<Option<&'a [u32]>, CodecError> {
    if let Some(ids) = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Quilts,
    ) {
        return Ok((!ids.is_empty()).then_some(ids));
    }
    if has_feature_affected_ids(
        ctx,
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Quilts,
    )? {
        return Ok(None);
    }
    Ok(
        agreed_surface_merge_replay_quilt_ids(ctx, replay, feature_id)?
            .filter(|ids| !ids.is_empty()),
    )
}

pub(super) fn surface_merge_quilt_state_offset(
    ctx: &DecodeContext<'_>,
    affected_ids: &[crate::feature::rows::FeatureAffectedIds],
    replay: &[crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    feature_id: u32,
    quilt_ids: &[u32],
) -> Result<Option<usize>, CodecError> {
    if let Some(ids) = agreed_feature_affected_ids(
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Quilts,
    ) {
        if !ctx.equal(ids, quilt_ids, "creo surface merge quilt ID equality")? {
            return Ok(None);
        }
        let mut offset = None;
        for record in ctx.admit_iter(affected_ids, "creo surface merge affected ID offsets")? {
            if record.feature_id != feature_id
                || record.kind != crate::feature::rows::AffectedIdKind::Quilts
            {
                continue;
            }
            if !ctx.equal(
                record.ids.as_slice(),
                quilt_ids,
                "creo surface merge affected quilt ID equality",
            )? {
                continue;
            }
            offset =
                Some(offset.map_or(record.offset, |current: usize| current.min(record.offset)));
        }
        return Ok(offset);
    }
    if has_feature_affected_ids(
        ctx,
        affected_ids,
        feature_id,
        crate::feature::rows::AffectedIdKind::Quilts,
    )? {
        return Ok(None);
    }
    let Some(ids) = agreed_surface_merge_replay_quilt_ids(ctx, replay, feature_id)? else {
        return Ok(None);
    };
    if !ctx.equal(ids, quilt_ids, "creo surface merge quilt ID equality")? {
        return Ok(None);
    }
    let mut offset = None;
    for record in ctx.admit_iter(replay, "creo surface merge replay ID offsets")? {
        if record.feature_id != feature_id {
            continue;
        }
        if !ctx.equal(
            record.quilt_ids.as_slice(),
            quilt_ids,
            "creo surface merge replay quilt ID equality",
        )? {
            continue;
        }
        offset = Some(offset.map_or(record.offset, |current: usize| current.min(record.offset)));
    }
    Ok(offset)
}

pub(in super::super) fn surface_merge_entity_dependencies(
    ctx: &DecodeContext<'_>,
    affected_ids: &[crate::feature::rows::FeatureAffectedIds],
    replay: &[crate::feature::rows::FeatureSurfaceMergeAffectedIds],
    tables: &[crate::feature::entity::FeatureEntityTable],
    feature_id: u32,
) -> Result<Vec<u32>, CodecError> {
    let Some(ids) = surface_merge_quilt_ids(ctx, affected_ids, replay, feature_id)? else {
        return Ok(Vec::new());
    };
    let Some(consumer_offset) =
        surface_merge_quilt_state_offset(ctx, affected_ids, replay, feature_id, ids)?
    else {
        return Ok(Vec::new());
    };
    let mut dependencies = Vec::new();
    for entity_id in ctx
        .admit_iter(ids, "creo surface merge quilt IDs")?
        .copied()
    {
        let Some(owner) =
            unique_preceding_feature_entity_producer(ctx, tables, entity_id, consumer_offset)?
        else {
            continue;
        };
        if owner != feature_id && !dependencies.contains(&owner) {
            ctx.reserve_vec(&mut dependencies, 1, "creo surface merge dependencies")?;
            dependencies.push(owner);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn has_feature_affected_ids(
    ctx: &DecodeContext<'_>,
    records: &[crate::feature::rows::FeatureAffectedIds],
    feature_id: u32,
    kind: crate::feature::rows::AffectedIdKind,
) -> Result<bool, CodecError> {
    ctx.any_by(
        records,
        |record| Ok(record.feature_id == feature_id && record.kind == kind),
        "creo feature affected ID records",
    )
}

fn agreed_feature_parent_ids(
    ctx: &DecodeContext<'_>,
    records: &[crate::feature::rows::FeatureAffectedIds],
    feature_id: u32,
) -> Result<Vec<u32>, CodecError> {
    let mut strong_emitted = false;
    let mut parent_emitted = false;
    let mut ids = Vec::new();
    for record in ctx
        .admit_iter(records, "creo agreed feature parent records")?
        .filter(|record| {
            record.feature_id == feature_id
                && matches!(
                    record.kind,
                    crate::feature::rows::AffectedIdKind::StrongParents
                        | crate::feature::rows::AffectedIdKind::Parents
                )
        })
    {
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
            ctx.reserve_vec(&mut ids, agreed.len(), "creo agreed feature parent IDs")?;
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
    let associations = unique_surface_prototype_associations(ctx, scan)?;
    for (prototype, row, _) in
        ctx.admit_iter(&associations, "creo surface prototype associations")?
    {
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
    for consumer in ctx
        .admit_iter(consumers, "creo prototype dependency consumers")?
        .copied()
    {
        if consumer == 0 || consumer == producer {
            continue;
        }
        let producers = match ctx.entry_btree_map(
            dependencies,
            consumer,
            "creo prototype dependency consumers",
        )? {
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        };
        if !ctx.contains(
            producers.as_slice(),
            &producer,
            "creo prototype dependency producer lookup",
        )? {
            ctx.reserve_vec(producers, 1, "creo prototype dependency producers")?;
            producers.push(producer);
        }
    }
    Ok(())
}

pub(in super::super) fn agreed_feature_replay_geometry_ids<'a>(
    ctx: &DecodeContext<'_>,
    records: &'a [crate::feature::rows::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Result<Option<&'a [u32]>, CodecError> {
    agreed_ids(
        ctx,
        ctx.admit_iter(records, "creo replay geometry ID records")?
            .filter(|record| record.feature_id == feature_id)
            .map(|record| record.geometry_ids.as_slice()),
    )
}

pub(in super::super) fn agreed_feature_replay_edge_ids<'a>(
    ctx: &DecodeContext<'_>,
    records: &'a [crate::feature::rows::FeatureReplayAffectedIds],
    feature_id: u32,
) -> Result<Option<&'a [u32]>, CodecError> {
    agreed_ids(
        ctx,
        ctx.admit_iter(records, "creo replay edge ID records")?
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
    let mut lookup_storage = ctx.reserve_scoped(0, "Creo feature reconciliation storage")?;
    let mut output_updates = Vec::new();
    for (index, feature) in ctx
        .admit_iter(
            &ir.model.features,
            "creo feature reconciliation output features",
        )?
        .enumerate()
    {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let outputs = cadmpeg_ir::features::DistinctMembers::try_from(
            super::outputs::feature_output_bodies(ctx, scan, ir, feature_id)?,
            ctx,
        )
        .map_err(cadmpeg_core::CodecError::from)?;
        lookup_storage.with_storage(|| {
            ctx.reserve_vec(&mut output_updates, 1, "creo reconciled output update rows")
        })?;
        output_updates.push((index, outputs));
    }
    let mut emitted = BTreeSet::new();
    for feature in ctx.admit_iter(&ir.model.features, "creo emitted feature identities")? {
        if ctx.contains_btree_set(
            &emitted,
            &feature.id,
            "creo emitted feature identity lookup",
        )? {
            continue;
        }
        let id = lookup_storage.with_storage(|| {
            feature
                .id
                .try_clone_for_decode(ctx, "creo emitted feature identity text")
        })?;
        lookup_storage.with_storage(|| {
            ctx.insert_btree_set(&mut emitted, id, "creo emitted feature identity nodes")
        })?;
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
        if pending
            .as_ref()
            .is_some_and(|(update_index, _)| *update_index == index)
        {
            if let Some((_, outputs)) = pending.take() {
                feature.evaluation.set_outputs(outputs);
            }
            pending = updates.next();
        }
        let mut native_dependencies = Vec::new();
        let native_dependency_ids = lookup_storage.with_storage(|| {
            native_feature_dependency_ids(
                ctx,
                &scan.features.affected_ids,
                &scan.features.operations,
                &scan.features.entity_tables,
                &scan.features.surface_merge_replay_affected_ids,
                &scan.surfaces.rows,
                (
                    feature_id,
                    prototype_dependencies
                        .get(&feature_id)
                        .map_or(&[], Vec::as_slice),
                ),
            )
        })?;
        for dependency in ctx.admit_iter(
            &native_dependency_ids,
            "creo reconciled native dependency IDs",
        )? {
            let text = ctx.format_retained(
                format_args!("creo:model:feature#{dependency}"),
                "creo reconciled native dependency IDs",
            )?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(text.len()),
                "creo reconciled native dependency identity validation",
            )?;
            let id = IrFeatureId::mint(text).map_err(cadmpeg_core::CodecError::malformed)?;
            if ctx.contains_btree_set(&emitted, &id, "creo reconciled feature emission lookup")?
                && !ctx.equal(
                    &id,
                    &feature.id,
                    "creo reconciled feature identity comparison",
                )?
            {
                ctx.reserve_vec(
                    &mut native_dependencies,
                    1,
                    "creo reconciled native dependencies",
                )?;
                native_dependencies.push(id);
            }
        }
        let generated_dependencies = lookup_storage.with_storage(|| {
            feature_generated_dependencies(ctx, feature.evaluation.definition())
        })?;
        let mut generated_ids = Vec::new();
        for dependency in ctx.admit_iter(
            &generated_dependencies,
            "creo reconciled generated dependency references",
        )? {
            let id =
                dependency.try_clone_for_decode(ctx, "creo reconciled generated dependency IDs")?;
            ctx.reserve_vec(
                &mut generated_ids,
                1,
                "creo reconciled generated dependencies",
            )?;
            generated_ids.push(id);
        }
        feature.dependencies = cadmpeg_ir::features::DistinctMembers::try_from(
            reconciled_dependencies(
                ctx,
                &feature.id,
                &feature.dependencies,
                native_dependencies.into_iter().chain(generated_ids),
                &emitted,
            )?,
            ctx,
        )
        .map_err(cadmpeg_core::CodecError::from)?;
        if let Some(parent_id) =
            current_feature_recipe_parent(&scan.features.operations, feature_id)
        {
            let text = ctx.format_retained(
                format_args!("creo:model:feature#{parent_id}"),
                "creo regeneration parent IDs",
            )?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(text.len()),
                "creo regeneration parent identity validation",
            )?;
            let parent = IrFeatureId::mint(text).map_err(cadmpeg_core::CodecError::malformed)?;
            if !ctx.equal(
                &parent,
                &feature.id,
                "creo regeneration feature identity comparison",
            )? && ctx.contains_btree_set(
                &emitted,
                &parent,
                "creo regeneration parent identity lookup",
            )? {
                let child = feature
                    .id
                    .try_clone_for_decode(ctx, "creo regeneration child IDs")?;
                lookup_storage.with_storage(|| {
                    ctx.reserve_vec(&mut regeneration_edges, 1, "creo regeneration edges")
                })?;
                regeneration_edges.push((child, parent));
            }
        }
    }
    for (child, parent) in ctx.admit_iter(&regeneration_edges, "creo feature regeneration edges")? {
        ir.model
            .set_feature_regeneration_parent(ctx, child, parent)?;
    }
    let mut remaining = Vec::new();
    lookup_storage.with_storage(|| {
        ctx.reserve_vec(
            &mut remaining,
            ir.model.features.len(),
            "creo remaining feature order",
        )
    })?;
    remaining.extend(0..ir.model.features.len());
    let mut ordered = Vec::new();
    lookup_storage.with_storage(|| {
        ctx.reserve_vec(
            &mut ordered,
            remaining.len(),
            "creo ordered feature indices",
        )
    })?;
    let mut preceding = BTreeSet::new();
    while !remaining.is_empty() {
        ctx.charge_work(1, "creo remaining feature ordering step")?;
        let mut position = None;
        for (candidate_position, index) in ctx
            .admit_iter(&remaining, "creo remaining feature order search")?
            .enumerate()
        {
            let feature = &ir.model.features[*index];
            let mut ready = true;
            for required in ctx
                .admit_iter(
                    feature.dependencies.as_slice(),
                    "creo feature ordering dependencies",
                )?
                .chain(ir.model.feature_parent(&feature.id))
            {
                if ctx.contains_btree_set(
                    &emitted,
                    required,
                    "creo emitted dependency identity lookup",
                )? && !ctx.contains_btree_set(
                    &preceding,
                    required,
                    "creo preceding dependency identity lookup",
                )? {
                    ready = false;
                    break;
                }
            }
            if ready {
                position = Some(candidate_position);
                break;
            }
        }
        let Some(position) = position else {
            break;
        };
        let shifted_len = remaining
            .len()
            .checked_sub(position)
            .and_then(|after_removed| after_removed.checked_sub(1))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "creo remaining feature order removal shifts",
                    u64::MAX,
                    u64::MAX,
                )
            })?;
        let shift_bytes = cadmpeg_core::decode::u64_from_index(shifted_len)
            .checked_mul(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
                usize,
            >()))
            .ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "creo remaining feature order removal shifts",
                    u64::MAX,
                    u64::MAX,
                )
            })?;
        ctx.charge_work(shift_bytes, "creo remaining feature order removal shifts")?;
        let index = remaining.remove(position);
        lookup_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut preceding,
                &ir.model.features[index].id,
                "creo preceding feature identity nodes",
            )
        })?;
        ordered.push(index);
    }
    ordered.extend(remaining);
    for (ordinal, index) in ctx
        .admit_iter(&ordered, "creo ordered feature ordinal assignment")?
        .copied()
        .enumerate()
    {
        ir.model.features[index].ordinal = cadmpeg_core::decode::u64_from_index(ordinal);
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
            ctx.reserve_vec(&mut dependencies, 1, "creo generated dependencies")?;
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
        for face in ctx.admit_iter(&**faces, "creo generated feature face references")? {
            push_unique(&face.feature)?;
        }
    }
    let mut visit_selection =
        |selection: &'a EdgeSelection| -> Result<(), cadmpeg_core::CodecError> {
            if let EdgeSelection::Generated { edges, .. } = selection {
                for edge in ctx.admit_iter(&**edges, "creo generated feature edge references")? {
                    push_unique(&edge.feature)?;
                }
            }
            Ok(())
        };
    match definition {
        IrFeatureDefinition::Operation(IrFeatureOperation::Fillet { groups }) => {
            for group in ctx.admit_iter(groups.as_slice(), "creo generated fillet edge groups")? {
                visit_selection(&group.edges)?;
            }
        }
        IrFeatureDefinition::Operation(IrFeatureOperation::Chamfer { groups, .. }) => {
            for group in ctx.admit_iter(groups.as_slice(), "creo generated chamfer edge groups")? {
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
    for dependency in ctx.admit_iter(established, "creo established feature dependencies")? {
        if !ctx.contains_btree_set(
            emitted,
            dependency,
            "creo established dependency emission lookup",
        )? || ctx.equal(
            dependency,
            feature_id,
            "creo established dependency identity comparison",
        )? || dependencies.contains(dependency)
        {
            continue;
        }
        let id = dependency.try_clone_for_decode(ctx, "creo established dependency IDs")?;
        ctx.reserve_vec(&mut dependencies, 1, "creo reconciled dependencies")?;
        dependencies.push(id);
    }
    for dependency in native {
        if ctx.contains_btree_set(
            emitted,
            &dependency,
            "creo native dependency emission lookup",
        )? && !ctx.equal(
            &dependency,
            feature_id,
            "creo native dependency identity comparison",
        )? && !ctx.contains(
            &dependencies,
            &dependency,
            "creo native dependency duplicate lookup",
        )? {
            ctx.reserve_vec(&mut dependencies, 1, "creo reconciled dependencies")?;
            dependencies.push(dependency);
        }
    }
    Ok(dependencies)
}

fn agreed_ids<'a>(
    ctx: &DecodeContext<'_>,
    mut values: impl Iterator<Item = &'a [u32]>,
) -> Result<Option<&'a [u32]>, CodecError> {
    let Some(first) = values.next() else {
        return Ok(None);
    };
    for value in values {
        if !ctx.equal(value, first, "creo feature ID list agreement")? {
            return Ok(None);
        }
    }
    Ok(Some(first))
}

#[cfg(test)]
mod tests;
