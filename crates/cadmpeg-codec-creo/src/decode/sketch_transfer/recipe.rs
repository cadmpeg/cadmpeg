// SPDX-License-Identifier: Apache-2.0
//! Feature recipe, schema class, and revolution-extent helpers.

use super::super::uniqueness::unique_feature_definition_for_transform;
use crate::container::ContainerScan;
use crate::feature::schema::SchemaClass;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{AngularTermination, RevolveExtent};
#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::{BTreeSet, HashMap};

pub(in super::super) fn feature_recipe(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<crate::feature::operations::FeatureRecipeKind>, CodecError> {
    Ok(current_feature_recipe(ctx, &scan.features.operations, feature_id)?
        .map(crate::feature::operations::FeatureRecipe::kind))
}

pub(in super::super) fn feature_recipe_effect(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<crate::feature::operations::FeatureRecipeEffect>, CodecError> {
    Ok(current_feature_recipe(ctx, &scan.features.operations, feature_id)?
        .map(crate::feature::operations::FeatureRecipe::effect))
}

pub(in super::super) fn feature_section_sweep_semantics_conflict(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<bool, CodecError> {
    let Some(operation) = current_feature_operation(ctx, &scan.features.operations, feature_id)? else {
        return Ok(false);
    };
    if operation.recipe.is_conflicting() {
        return Ok(true);
    }
    if !operation.display_state_conflict
        || !matches!(
            operation.recipe,
            crate::feature::operations::RecipeResolution::None
        )
    {
        return Ok(false);
    }
    ctx.equal(
        &operation.kind,
        &crate::feature::operations::OperationKind::Native,
        "creo feature operation kind comparison",
    )
}

pub(in super::super) fn current_additive_feature_recipe(
    ctx: &DecodeContext<'_>,
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Result<Option<crate::feature::operations::FeatureRecipeKind>, CodecError> {
    Ok(current_feature_recipe(ctx, operations, feature_id)?.and_then(|recipe| {
        (recipe.effect() == crate::feature::operations::FeatureRecipeEffect::Protrude)
            .then(|| recipe.kind())
    }))
}

#[cfg(test)]
pub(in super::super) fn first_material_feature_by_definition_order(
    target_feature_id: u32,
    material_definition_offsets: &[(u32, usize)],
) -> bool {
    let mut offsets = BTreeMap::new();
    for &(feature_id, offset) in material_definition_offsets {
        if offsets.insert(feature_id, offset).is_some() {
            return false;
        }
    }
    let Some(target_offset) = offsets.get(&target_feature_id).copied() else {
        return false;
    };
    offsets
        .into_iter()
        .filter(|(feature_id, _)| *feature_id != target_feature_id)
        .all(|(_, offset)| offset > target_offset)
}

pub(in super::super) fn feature_is_first_material_operation(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "creo first material indexes")?;
    let mut unique_operations = HashMap::new();
    for operation in ctx.admit_iter(&scan.features.operations, "creo first material identity rows")? {
        storage.with_storage(|| {
            ctx.entry_hash_map(&mut unique_operations, operation.feature_id, "creo first material operation identities")?
                .and_modify(|unique| *unique = false).or_insert(true);
            Ok::<_, CodecError>(())
        })?;
    }
    let mut transforms_by_feature = None;
    let mut target_offset = None;
    let mut earliest_other_offset: Option<usize> = None;
    for operation in ctx.admit_iter(
        &scan.features.operations,
        "creo first material operation rows",
    )? {
        let candidate = operation.feature_id;
        if unique_operations.get(&candidate) != Some(&true) { continue; }
        let recipe_is_material = operation.recipe.resolved().is_some_and(|recipe| {
            matches!(
                recipe.effect(),
                crate::feature::operations::FeatureRecipeEffect::Protrude
                    | crate::feature::operations::FeatureRecipeEffect::Cut
            )
        });
        if !recipe_is_material
            && !matches!(
                feature_schema_class_with_operation(ctx, scan, candidate, Some(operation))?,
                Some(SchemaClass::Cut | SchemaClass::Protrusion)
            )
        {
            continue;
        }
        if transforms_by_feature.is_none() {
            let mut transforms = HashMap::new();
            for transform in ctx.admit_iter(&scan.features.section_transforms, "creo first material section transforms")? {
                let Some(id) = transform.feature_id else { continue; };
                storage.with_storage(|| {
                    ctx.entry_hash_map(&mut transforms, id, "creo first material transform identities")?
                        .and_modify(|unique| *unique = None).or_insert(Some(transform));
                    Ok::<_, CodecError>(())
                })?;
            }
            transforms_by_feature = Some(transforms);
        }
        let Some(transform) = transforms_by_feature.as_ref().and_then(|transforms| transforms.get(&candidate)).copied().flatten() else { continue; };
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        if candidate == feature_id {
            target_offset = Some(definition.offset);
        } else {
            earliest_other_offset =
                Some(earliest_other_offset.map_or(definition.offset, |previous| {
                    previous.min(definition.offset)
                }));
        }
    }
    Ok(
        target_offset
            .is_some_and(|target| earliest_other_offset.is_none_or(|other| other > target)),
    )
}

pub(in super::super) fn current_feature_recipe(
    ctx: &DecodeContext<'_>,
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Result<Option<crate::feature::operations::FeatureRecipe>, CodecError> {
    Ok(current_feature_operation(ctx, operations, feature_id)?.and_then(|operation| operation.recipe.resolved()))
}

pub(in super::super) fn current_feature_recipe_parent(
    ctx: &DecodeContext<'_>,
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Result<Option<u32>, CodecError> {
    Ok(current_feature_operation(ctx, operations, feature_id)?.and_then(|operation| {
        operation.recipe.resolved()?;
        operation.parent_feature_id()
    }))
}

pub(in super::super) fn current_feature_operation<'operations>(
    ctx: &DecodeContext<'_>,
    operations: &'operations [crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Result<Option<&'operations crate::feature::operations::FeatureOperation>, CodecError> {
    if operations.is_empty() { return Ok(None); }
    crate::decode::uniqueness::exactly_one_by(ctx, operations,
        |operation| Ok(operation.feature_id == feature_id), "creo current feature operation rows")
}

pub(in super::super) fn feature_schema_class(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<SchemaClass>, cadmpeg_core::CodecError> {
    let operation = current_feature_operation(ctx, &scan.features.operations, feature_id)?;
    feature_schema_class_with_operation(ctx, scan, feature_id, operation)
}

fn feature_schema_class_with_operation(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
    operation: Option<&crate::feature::operations::FeatureOperation>,
) -> Result<Option<SchemaClass>, CodecError> {
    resolved_feature_schema_class_from_classes(
        operation,
        |visit_class| {
            for (rows, operation) in [
                (&scan.features.rows, "creo feature schema rows"),
                (&scan.features.depdb_recipe_rows, "creo feature depdb schema rows"),
            ] {
                if rows.is_empty() { continue; }
                let mut rows = rows.iter();
                while let Some(row) = ctx.next_charged(&mut rows, operation)? {
                    if row.feature_id != feature_id { continue; }
                    let Some(schema_class) = row.root_schema_class else { continue; };
                    if matches!(visit_class(schema_class)?, std::ops::ControlFlow::Break(())) {
                        return Ok(());
                    }
                }
            }
            Ok(())
        },
        || {
            ctx.any_by(
                &scan.features.legacy_rounds,
                |round| Ok(round.feature_id == feature_id),
                "creo legacy round schema rows",
            )
        },
    )
}

pub(in super::super) fn resolved_feature_schema_class_from_classes(
    operation: Option<&crate::feature::operations::FeatureOperation>,
    visit_classes: impl FnOnce(
        &mut dyn FnMut(SchemaClass) -> Result<std::ops::ControlFlow<()>, cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError>,
    has_legacy_round: impl FnOnce() -> Result<bool, cadmpeg_core::CodecError>,
) -> Result<Option<SchemaClass>, cadmpeg_core::CodecError> {
    if let Some(schema_class) = operation.and_then(crate::feature::operations::FeatureOperation::root_schema_class)
    {
        return Ok(Some(schema_class));
    }
    let mut selected = None;
    let mut conflict = false;
    let mut visit_class = |schema_class| {
        if selected.is_some_and(|previous| previous != schema_class) {
            conflict = true;
            return Ok(std::ops::ControlFlow::Break(()));
        }
        selected = Some(schema_class);
        Ok(std::ops::ControlFlow::Continue(()))
    };
    visit_classes(&mut visit_class)?;
    if !conflict && selected.is_some() {
        return Ok(selected);
    }
    Ok(has_legacy_round()?.then_some(SchemaClass::Round))
}

pub(in super::super) fn feature_row_schema_classes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<BTreeSet<SchemaClass>, cadmpeg_core::CodecError> {
    let mut classes = BTreeSet::new();
    for row in ctx
        .admit_iter(&scan.features.rows, "creo feature schema class rows")?
        .chain(ctx.admit_iter(
            &scan.features.depdb_recipe_rows,
            "creo depdb feature schema class rows",
        )?)
    {
        if row.feature_id == feature_id {
            if let Some(schema_class) = row.root_schema_class {
                ctx.insert_btree_set(
                    &mut classes,
                    schema_class,
                    "creo feature schema class nodes",
                )?;
            }
        }
    }
    Ok(classes)
}

pub(in super::super) fn row_feature_schema_classes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &[crate::feature::rows::FeatureRow],
    feature_id: u32,
) -> Result<BTreeSet<SchemaClass>, cadmpeg_core::CodecError> {
    let mut classes = BTreeSet::new();
    for row in ctx
        .admit_iter(rows, "creo row feature schema classes")?
        .filter(|row| row.feature_id == feature_id)
    {
        if let Some(schema_class) = row.root_schema_class {
            ctx.insert_btree_set(&mut classes, schema_class, "creo row schema class nodes")?;
        }
    }
    Ok(classes)
}

pub(in super::super) fn feature_revolution_extent(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<RevolveExtent>, cadmpeg_core::CodecError> {
    let extent =
        unique_feature_revolution_extent(ctx, &scan.features.revolution_extents, feature_id)?;
    Ok(extent.map(|_| RevolveExtent::OneSided {
        termination: AngularTermination::Angle {
            angle: cadmpeg_ir::scalar::PositiveAngle::FULL_TURN,
        },
    }))
}

pub(in super::super) fn unique_feature_revolution_extent<'records>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    records: &'records [crate::feature::rows::FeatureRevolutionExtent],
    feature_id: u32,
) -> Result<Option<&'records crate::feature::rows::FeatureRevolutionExtent>, cadmpeg_core::CodecError>
{
    ctx.find_by(
        records,
        |record| Ok(record.feature_id == feature_id),
        "creo feature revolution extent rows",
    )
}

#[cfg(test)]
mod tests;
