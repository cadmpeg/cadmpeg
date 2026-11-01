// SPDX-License-Identifier: Apache-2.0
//! Bind component-local construction history to neutral product definitions.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    DistinctMembers, FeatureDefinition, FeatureOperation, SketchFeatureBinding,
};
use cadmpeg_ir::{document::Model, ids::ProductDefinitionId, report::loss::LossNote};

use super::feature_project::{ComponentHistoryNamespace, ScopeHistoryGraph};
use crate::{ids::native_stream, loss::F3dLossCode, native::F3dNative};

pub(crate) fn attach(
    native: &F3dNative,
    model: &mut Model,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut owners = BTreeMap::new();
    for scope in &native.design_parameter_scopes {
        if !model
            .features
            .iter()
            .any(|row| row.native_ref.as_deref() == Some(scope.id.as_str()))
        {
            continue;
        }
        let owner = resolve(native, model, losses, &scope.id, scope.record_index);
        owners.insert(scope.id.as_str(), owner);
    }
    for feature in &mut model.features {
        feature.product_definition = feature
            .native_ref
            .as_deref()
            .and_then(|id| owners.get(id))
            .cloned()
            .flatten();
    }
    let member_root = crate::ids::archive_product_definition_id();
    let unowned = model
        .bodies
        .iter()
        .map(|body| &body.id)
        .filter(|body| {
            !model
                .product_definitions
                .iter()
                .any(|row| row.bodies.contains(body))
        })
        .cloned()
        .collect::<BTreeSet<_>>();
    for feature in &mut model.features {
        let owned_bodies = feature.product_definition.as_ref().and_then(|owner| {
            model
                .product_definitions
                .iter()
                .find(|row| &row.id == owner)
                .map(|row| row.bodies.iter().collect::<BTreeSet<_>>())
                .or_else(|| (owner == &member_root).then(|| unowned.iter().collect()))
        });
        if let Some(owned_bodies) = owned_bodies {
            let outputs = feature.evaluation.outputs();
            let retained = outputs
                .iter()
                .filter(|body| owned_bodies.contains(body))
                .cloned()
                .collect::<Vec<_>>();
            // A feature can place its result bodies in another component than
            // the one whose naming space holds its scope. When one product
            // definition owns every output body, that definition owns the
            // feature.
            let output_owner = if retained.len() == outputs.len() || outputs.is_empty() {
                None
            } else {
                let mut holders = model
                    .product_definitions
                    .iter()
                    .filter(|row| outputs.iter().all(|body| row.bodies.contains(body)));
                match (holders.next(), holders.next()) {
                    (Some(holder), None) => Some(holder.id.clone()),
                    _ => None,
                }
            };
            if let Some(holder) = output_owner {
                feature.product_definition = Some(holder);
            } else if retained.len() != outputs.len() {
                losses.push(F3dLossCode::ScopeOutputOwnerMismatch.note(format!(
                    "Feature {} has {} output body link(s) outside its product definition; those links remain native.",
                    feature.id, outputs.len() - retained.len()
                )));
                feature
                    .evaluation
                    .set_outputs(DistinctMembers::try_from(retained).map_err(CodecError::from)?);
            }
        }
        if let (
            Some(owner),
            FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch)),
            }),
        ) = (&feature.product_definition, feature.evaluation.definition())
        {
            if model
                .product_definitions
                .iter()
                .any(|row| &row.id != owner && row.sketches.contains(sketch))
            {
                losses.push(F3dLossCode::ScopeSketchOwnerMismatch.note(format!(
                    "Feature {} and its sketch collection name different product definitions; the sketch binding remains native.", feature.id
                )));
                feature
                    .evaluation
                    .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: SketchFeatureBinding::Planar(None),
                    }));
                continue;
            }
            if let Some(definition) = model
                .product_definitions
                .iter_mut()
                .find(|row| &row.id == owner)
            {
                if !definition.sketches.contains(sketch) {
                    definition.sketches.push(sketch.clone());
                }
            }
        }
    }
    for index in 0..model.parameters.len() {
        let parameter = &model.parameters[index];
        let inherited = parameter
            .owner
            .as_ref()
            .and_then(|owner| model.features.iter().find(|row| &row.id == owner))
            .map(|row| row.product_definition.clone());
        let source = parameter
            .native_ref
            .as_deref()
            .and_then(|id| native.design_parameters.iter().find(|row| row.id == id));
        let owner = if let Some(inherited) = inherited {
            inherited
        } else if let Some(source) = source {
            let record_index = source
                .owner_record_index()
                .and_then(|record| {
                    native.design_parameter_owners.iter().find(|row| {
                        native_stream(row.id()) == native_stream(&source.id)
                            && row.record_index() == record
                    })
                })
                .map_or(
                    source.record_index,
                    |row| row.scope_record_index().unwrap_or(source.record_index),
                );
            resolve(native, model, losses, &source.id, record_index)
        } else {
            None
        };
        model.parameters[index].product_definition = owner;
    }
    Ok(())
}

fn resolve(
    native: &F3dNative,
    model: &Model,
    losses: &mut Vec<LossNote>,
    id: &str,
    record_index: u32,
) -> Option<ProductDefinitionId> {
    let namespace = ScopeHistoryGraph::component_namespace_at(
        id,
        record_index,
        &native.design_component_naming_spaces,
    );
    let mut candidates = BTreeSet::new();
    match namespace {
        Some(ComponentHistoryNamespace::Component(record)) => {
            for space in &native.design_component_naming_spaces {
                if space.component_record_index == record
                    && native_stream(&space.id) == native_stream(id)
                {
                    let id = crate::ids::neutral_component_id(&space.context_uuid);
                    for definition in &model.product_definitions {
                        if definition.id == id {
                            candidates.insert(id.clone());
                        }
                    }
                }
            }
        }
        Some(ComponentHistoryNamespace::Aggregate) => {
            candidates.insert(crate::ids::archive_product_definition_id());
        }
        None => return None,
    }
    let owner: Option<ProductDefinitionId> = if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        let cause = if candidates.is_empty() {
            "missing-definition"
        } else {
            "multiple-definitions"
        };
        losses.push(
            F3dLossCode::ScopeOwnerUnresolved
                .note(format!("Scope {id} product ownership unresolved: {cause}.")),
        );
        None
    };
    owner
}

/// Clears product owners that name a definition absent from the document.
///
/// Root-level design history names the archive member definition. F3Z merge
/// creates that definition for each external member at every decode depth; a
/// standalone document and the archive root have none, so their root-level
/// history has no product owner.
pub(crate) fn release_unbound(model: &mut Model) {
    let defined = model
        .product_definitions
        .iter()
        .map(|row| row.id.clone())
        .collect::<BTreeSet<_>>();
    for feature in &mut model.features {
        if feature
            .product_definition
            .as_ref()
            .is_some_and(|owner| !defined.contains(owner))
        {
            feature.product_definition = None;
        }
    }
    for parameter in &mut model.parameters {
        if parameter
            .product_definition
            .as_ref()
            .is_some_and(|owner| !defined.contains(owner))
        {
            parameter.product_definition = None;
        }
    }
}

#[cfg(test)]
mod tests;
