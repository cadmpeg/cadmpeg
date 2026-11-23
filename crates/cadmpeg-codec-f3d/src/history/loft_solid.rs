// SPDX-License-Identifier: Apache-2.0
//! Solid setting of projected Lofts from the kinds of their output bodies.

use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureId, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind};
use std::collections::HashMap;

/// Sets `solid` of each projected Loft and records the reading.
///
/// `result_kinds` holds the output-body kinds that the Loft's own history state
/// decodes. A Loft without an entry reads the bodies of the final model.
pub(crate) fn bind(
    features: &mut [Feature],
    result_kinds: &HashMap<FeatureId, Vec<BodyKind>>,
    bodies: &[Body],
) -> Vec<LossNote> {
    let body_kinds = bodies
        .iter()
        .map(|body| (&body.id, body.kind))
        .collect::<HashMap<_, _>>();
    let mut losses = Vec::new();
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Loft { op, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let (source, kinds) = match result_kinds.get(&feature.id) {
            Some(kinds) => (
                "at its own history state",
                kinds.iter().copied().map(Some).collect::<Vec<_>>(),
            ),
            None => (
                "in the final model",
                feature
                    .evaluation
                    .outputs()
                    .iter()
                    .map(|output| body_kinds.get(output).copied())
                    .collect(),
            ),
        };
        let sheets = kinds
            .iter()
            .filter(|kind| **kind == Some(BodyKind::Sheet))
            .count();
        let new_body = *op == BooleanOp::NewBody;
        let surface = new_body && !kinds.is_empty() && sheets == kinds.len();
        let reason = if surface {
            format!("its {sheets} output bodies are sheet bodies {source}")
        } else if !new_body {
            "it combines with an existing body".to_owned()
        } else if kinds.is_empty() {
            "it has no output body".to_owned()
        } else {
            format!(
                "{} of its {} output bodies are not sheet bodies {source}",
                kinds.len() - sheets,
                kinds.len()
            )
        };
        losses.push(
            crate::loss::F3dLossCode::FeatureLoftSolidInferred.note(format!(
                "Loft feature {} projects solid {} because {reason} (DR-331).",
                feature.id, !surface
            )),
        );
        if surface {
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Loft { solid, .. }) =
                    definition
                {
                    *solid = false;
                }
            });
        }
    }
    losses
}

#[cfg(test)]
mod tests;
