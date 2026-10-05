// SPDX-License-Identifier: Apache-2.0
//! Feature recipe, schema class, and revolution-extent helpers.

use super::super::uniqueness::unique_feature_definition_for_transform;
use crate::container::ContainerScan;
use crate::feature::schema::SchemaClass;
use cadmpeg_ir::features::{AngularTermination, RevolveExtent};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::BTreeSet;

pub(in super::super) fn feature_recipe(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::operations::FeatureRecipeKind> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::operations::FeatureRecipe::kind)
}

pub(in super::super) fn feature_recipe_effect(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::operations::FeatureRecipeEffect> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::operations::FeatureRecipe::effect)
}

pub(in super::super) fn feature_section_sweep_semantics_conflict(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<bool, CodecError> {
    let Some(operation) = current_feature_operation(&scan.features.operations, feature_id) else {
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
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::operations::FeatureRecipeKind> {
    let recipe = current_feature_recipe(operations, feature_id)?;
    (recipe.effect() == crate::feature::operations::FeatureRecipeEffect::Protrude)
        .then(|| recipe.kind())
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
    let mut target_offset = None;
    let mut earliest_other_offset: Option<usize> = None;
    for operation in ctx.admit_iter(
        &scan.features.operations,
        "creo first material operation rows",
    )? {
        let candidate = operation.feature_id;
        let Some(operation) = current_feature_operation(&scan.features.operations, candidate)
        else {
            continue;
        };
        let recipe_is_material = operation.recipe.resolved().is_some_and(|recipe| {
            matches!(
                recipe.effect(),
                crate::feature::operations::FeatureRecipeEffect::Protrude
                    | crate::feature::operations::FeatureRecipeEffect::Cut
            )
        });
        if !recipe_is_material
            && !matches!(
                feature_schema_class(ctx, scan, candidate)?,
                Some(SchemaClass::Cut | SchemaClass::Protrusion)
            )
        {
            continue;
        }
        let mut transforms = ctx
            .admit_iter(
                &scan.features.section_transforms,
                "creo first material section transforms",
            )?
            .filter(|transform| transform.feature_id == Some(candidate));
        let Some(transform) = transforms.next() else {
            continue;
        };
        if transforms.next().is_some() {
            continue;
        }
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
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::operations::FeatureRecipe> {
    current_feature_operation(operations, feature_id)?
        .recipe
        .resolved()
}

pub(in super::super) fn current_feature_recipe_parent(
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Option<u32> {
    let operation = current_feature_operation(operations, feature_id)?;
    operation.recipe.resolved()?;
    operation.parent_feature_id()
}

pub(in super::super) fn current_feature_operation(
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
) -> Option<&crate::feature::operations::FeatureOperation> {
    let mut matches = operations
        .iter()
        .filter(|operation| operation.feature_id == feature_id);
    let operation = matches.next()?;
    matches.next().is_none().then_some(operation)
}

pub(in super::super) fn feature_schema_class(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<SchemaClass>, cadmpeg_core::CodecError> {
    resolved_feature_schema_class_from_classes(
        &scan.features.operations,
        feature_id,
        |visit_class| {
            for row in ctx
                .admit_iter(&scan.features.rows, "creo feature schema rows")?
                .chain(ctx.admit_iter(
                    &scan.features.depdb_recipe_rows,
                    "creo feature depdb schema rows",
                )?)
                .filter(|row| row.feature_id == feature_id)
            {
                let Some(schema_class) = row.root_schema_class else {
                    continue;
                };
                if matches!(visit_class(schema_class)?, std::ops::ControlFlow::Break(())) {
                    break;
                }
            }
            Ok(())
        },
        || {
            Ok(ctx
                .admit_iter(
                    &scan.features.legacy_rounds,
                    "creo legacy round schema rows",
                )?
                .any(|round| round.feature_id == feature_id))
        },
    )
}

pub(in super::super) fn resolved_feature_schema_class_from_classes(
    operations: &[crate::feature::operations::FeatureOperation],
    feature_id: u32,
    visit_classes: impl FnOnce(
        &mut dyn FnMut(SchemaClass) -> Result<std::ops::ControlFlow<()>, cadmpeg_core::CodecError>,
    ) -> Result<(), cadmpeg_core::CodecError>,
    has_legacy_round: impl FnOnce() -> Result<bool, cadmpeg_core::CodecError>,
) -> Result<Option<SchemaClass>, cadmpeg_core::CodecError> {
    if let Some(schema_class) = current_feature_operation(operations, feature_id)
        .and_then(crate::feature::operations::FeatureOperation::root_schema_class)
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
) -> Result<
    Option<&'records crate::feature::rows::FeatureRevolutionExtent>,
    cadmpeg_core::CodecError,
> {
    Ok(ctx
        .admit_iter(records, "creo feature revolution extent rows")?
        .find(|record| record.feature_id == feature_id))
}

#[cfg(test)]
mod tests;
