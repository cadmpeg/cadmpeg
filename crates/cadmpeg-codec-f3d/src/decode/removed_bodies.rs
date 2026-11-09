// SPDX-License-Identifier: Apache-2.0
//! Bodies that an active `RemoveBody` deletes from the final model.

use cadmpeg_ir::features::{
    BodyRetentionMode, BodySelection, Feature, FeatureDefinition, FeatureId, FeatureOperation,
};
use cadmpeg_ir::ids::BodyId;
use std::collections::HashMap;

/// The model bodies that `feature` removes: the bodies an active,
/// delete-selected `RemoveBody` resolves.
fn removed_bodies(feature: &Feature) -> &[BodyId] {
    if feature.suppressed == Some(true) {
        return &[];
    }
    let operation = match feature.evaluation.definition() {
        FeatureDefinition::Operation(operation)
        | FeatureDefinition::PostProcess { operation, .. } => operation,
    };
    let FeatureOperation::DeleteBody {
        bodies,
        mode: BodyRetentionMode::DeleteSelected,
    } = operation
    else {
        return &[];
    };
    match bodies {
        BodySelection::Bodies(bodies) => bodies.as_slice(),
        BodySelection::Resolved { bodies, .. } => bodies.as_slice(),
        _ => &[],
    }
}

/// Record on each model body that one active `RemoveBody` deletes the
/// feature that removes it. A body that several features name keeps no
/// mark.
pub(super) fn mark(model: &mut cadmpeg_ir::document::Model) {
    let mut removers = HashMap::<&BodyId, Option<&FeatureId>>::new();
    for feature in &model.features {
        for body in removed_bodies(feature) {
            removers
                .entry(body)
                .and_modify(|remover| *remover = None)
                .or_insert(Some(&feature.id));
        }
    }
    let marks = removers
        .into_iter()
        .filter_map(|(body, remover)| Some((body.clone(), remover?.clone())))
        .collect::<HashMap<_, _>>();
    for body in &mut model.bodies {
        if let Some(remover) = marks.get(&body.id) {
            body.removed_by = Some(remover.clone());
        }
    }
}

#[cfg(test)]
mod tests;
