// SPDX-License-Identifier: Apache-2.0
//! Feature recipe, schema class, and revolution-extent helpers.

use super::super::uniqueness::unique_feature_definition_for_transform;
use crate::container::ContainerScan;
use cadmpeg_ir::features::{Angle, AngularTermination, RevolveExtent};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn feature_recipe(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeKind> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::FeatureRecipe::kind)
}

pub(in super::super) fn feature_recipe_effect(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeEffect> {
    current_feature_recipe(&scan.features.operations, feature_id)
        .map(crate::feature::FeatureRecipe::effect)
}

pub(in super::super) fn feature_section_sweep_semantics_conflict(
    scan: &ContainerScan,
    feature_id: u32,
) -> bool {
    current_feature_operation(&scan.features.operations, feature_id).is_some_and(|operation| {
        operation.recipe_conflict
            || (operation.display_state_conflict
                && operation.recipe.is_none()
                && operation.kind == crate::feature::OperationKind::Native)
    })
}

pub(in super::super) fn current_additive_feature_recipe(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipeKind> {
    let recipe = current_feature_recipe(operations, feature_id)?;
    (recipe.effect() == crate::feature::FeatureRecipeEffect::Protrude).then(|| recipe.kind())
}

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
    scan: &ContainerScan,
    feature_id: u32,
) -> bool {
    let candidate_feature_ids = scan
        .features
        .operations
        .iter()
        .map(|operation| operation.feature_id)
        .collect::<BTreeSet<_>>()
        .into_iter();
    let mut material_definition_offsets = Vec::new();
    for candidate in candidate_feature_ids {
        let Some(operation) = current_feature_operation(&scan.features.operations, candidate)
        else {
            continue;
        };
        let recipe_is_material = operation.recipe.is_some_and(|recipe| {
            matches!(
                recipe.effect(),
                crate::feature::FeatureRecipeEffect::Protrude
                    | crate::feature::FeatureRecipeEffect::Cut
            )
        });
        if !recipe_is_material && !matches!(feature_schema_class(scan, candidate), Some(916 | 917))
        {
            continue;
        }
        let transforms = scan
            .features
            .section_transforms
            .iter()
            .filter(|transform| transform.feature_id == Some(candidate))
            .collect::<Vec<_>>();
        let [transform] = transforms.as_slice() else {
            continue;
        };
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        material_definition_offsets.push((candidate, definition.offset));
    }
    first_material_feature_by_definition_order(feature_id, &material_definition_offsets)
}

pub(in super::super) fn current_feature_recipe(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<crate::feature::FeatureRecipe> {
    current_feature_operation(operations, feature_id)?.recipe
}

pub(in super::super) fn current_feature_recipe_parent(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<u32> {
    let operation = current_feature_operation(operations, feature_id)?;
    operation.recipe?;
    operation.parent_feature_id()
}

pub(in super::super) fn current_feature_operation(
    operations: &[crate::feature::FeatureOperation],
    feature_id: u32,
) -> Option<&crate::feature::FeatureOperation> {
    let mut matches = operations
        .iter()
        .filter(|operation| operation.feature_id == feature_id);
    let operation = matches.next()?;
    matches.next().is_none().then_some(operation)
}

pub(in super::super) fn feature_schema_class(scan: &ContainerScan, feature_id: u32) -> Option<u32> {
    resolved_feature_schema_class_from_classes(
        &scan.features.operations,
        feature_row_schema_classes(scan, feature_id),
        feature_id,
    )
    .or_else(|| {
        scan.features
            .legacy_rounds
            .iter()
            .any(|round| round.feature_id == feature_id)
            .then_some(913)
    })
}

pub(in super::super) fn resolved_feature_schema_class_from_classes(
    operations: &[crate::feature::FeatureOperation],
    classes: BTreeSet<u32>,
    feature_id: u32,
) -> Option<u32> {
    if let Some(schema_class) = current_feature_operation(operations, feature_id)
        .and_then(|operation| operation.root_schema_class())
    {
        return Some(schema_class);
    }
    if !classes.is_empty() {
        let mut classes = classes.into_iter();
        let schema_class = classes.next()?;
        return classes.next().is_none().then_some(schema_class);
    }
    None
}

pub(in super::super) fn feature_row_schema_classes(
    scan: &ContainerScan,
    feature_id: u32,
) -> BTreeSet<u32> {
    row_feature_schema_classes(&scan.features.rows, feature_id)
        .into_iter()
        .chain(row_feature_schema_classes(
            &scan.features.depdb_recipe_rows,
            feature_id,
        ))
        .collect()
}

pub(in super::super) fn row_feature_schema_classes(
    rows: &[crate::feature::FeatureRow],
    feature_id: u32,
) -> BTreeSet<u32> {
    rows.iter()
        .filter(|row| row.feature_id == feature_id)
        .filter_map(|row| row.root_schema_class)
        .collect()
}

pub(in super::super) fn feature_revolution_extent(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<RevolveExtent> {
    unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id).map(
        |kind| match kind {
            crate::feature::FeatureRevolutionExtentKind::FullTurn => RevolveExtent::OneSided {
                termination: AngularTermination::Angle {
                    angle: Angle(std::f64::consts::TAU),
                },
            },
        },
    )
}

pub(in super::super) fn unique_feature_revolution_extent_kind(
    records: &[crate::feature::FeatureRevolutionExtent],
    feature_id: u32,
) -> Option<crate::feature::FeatureRevolutionExtentKind> {
    let mut kinds = records
        .iter()
        .filter(|record| record.feature_id == feature_id)
        .map(|record| record.kind);
    let kind = kinds.next()?;
    kinds.all(|candidate| candidate == kind).then_some(kind)
}
