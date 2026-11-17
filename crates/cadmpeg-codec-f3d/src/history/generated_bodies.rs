// SPDX-License-Identifier: Apache-2.0
//! Body selections of bodies that the final model leaves out.

use super::{historical_body_slot, stable_ref, FeatureBodySelectionInputs};
use crate::history_records::AsmHistory;
use std::collections::{HashMap, HashSet};

/// Re-express body selections of bodies that the final model leaves out as
/// bodies that an earlier feature generates.
///
/// A body pattern seed resolved in its input state, and a body deletion whose
/// operands resolved through their recipe identity, name ASM body slots. When
/// every selected slot is absent from the model bodies, the one feature whose
/// history transition inserts that slot generates the body, and the selection
/// names it there.
/// The producer becomes a dependency of the selecting feature. Returns the
/// result states of the producers, each listing every body slot that the
/// producer's transition inserts.
pub(crate) fn bind_generated_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Vec<cadmpeg_ir::features::FeatureResultTopology> {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation, GeneratedBodyRef,
    };

    let live = inputs
        .bodies
        .iter()
        .filter_map(|body| stable_ref(body.id.as_str()))
        .collect::<HashSet<_>>();
    let feature_by_scope = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.clone()?, feature.id.clone())))
        .collect::<HashMap<_, _>>();
    let producer = |slot: i64| -> Option<(cadmpeg_ir::features::FeatureId, i64, Vec<i64>)> {
        let mut inserting = inputs
            .histories
            .iter()
            .flat_map(AsmHistory::states)
            .filter(|state| {
                state
                    .transition
                    .as_ref()
                    .is_some_and(|transition| transition.topology.bodies.inserted.contains(&slot))
            });
        let state = inserting.next()?;
        if inserting.next().is_some() {
            return None;
        }
        let mut scopes = inputs
            .scopes
            .iter()
            .filter(|scope| scope.history_state_id() == Some(state.state_id));
        let scope = scopes.next()?;
        if scopes.next().is_some() {
            return None;
        }
        let inserted = state.transition.as_ref()?.topology.bodies.inserted.clone();
        Some((
            feature_by_scope.get(&scope.id)?.clone(),
            state.state_id,
            inserted,
        ))
    };

    let mut results: Vec<(cadmpeg_ir::features::FeatureId, i64, Vec<i64>)> = Vec::new();
    for feature in features.iter_mut() {
        let scope = feature
            .native_ref
            .as_deref()
            .and_then(|native| inputs.scopes.iter().find(|scope| scope.id == native));
        let selected = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                let [PatternSeed::Bodies(BodySelection::Historical { bodies, native, .. })] =
                    seeds.as_slice()
                else {
                    continue;
                };
                bodies
                    .iter()
                    .map(|body| historical_body_slot(body.as_str()))
                    .collect::<Option<Vec<_>>>()
                    .map(|slots| (slots, native.as_str().to_owned()))
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Native(group_id),
                ..
            }) => scope
                .and_then(|scope| group_operand_body_slots(inputs, group_id, scope))
                .map(|slots| (slots, group_id.clone())),
            _ => continue,
        };
        let Some((slots, native)) = selected else {
            continue;
        };
        if slots.is_empty() || slots.iter().any(|slot| live.contains(slot)) {
            continue;
        }
        let Some(producers) = slots
            .iter()
            .map(|slot| producer(*slot))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Some(refs) = producers
            .iter()
            .zip(&slots)
            .map(|((producer, _, _), slot)| {
                GeneratedBodyRef::new(producer.clone(), slot.to_string()).ok()
            })
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let Ok(generated) = BodySelection::generated(refs, native) else {
            continue;
        };
        feature.evaluation.edit(|definition, _| match definition {
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                if let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() {
                    *selection = generated;
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) => {
                *bodies = generated;
            }
            _ => {}
        });
        for (producer, state_id, inserted) in producers {
            feature.dependencies.insert(producer.clone());
            if !results
                .iter()
                .any(|(known, state, _)| *known == producer && *state == state_id)
            {
                results.push((producer, state_id, inserted));
            }
        }
    }
    results
        .into_iter()
        .filter_map(|(producer, state_id, slots)| {
            let bodies = slots
                .iter()
                .map(|slot| cadmpeg_core::text::NonBlankString::new(slot.to_string()))
                .collect::<Option<Vec<_>>>()?;
            let members = cadmpeg_ir::features::FeatureResultMembers::new(
                bodies,
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .ok()?;
            Some(cadmpeg_ir::features::FeatureResultTopology::new(
                crate::ids::history_result_state_id(&producer, state_id),
                producer,
                members,
                None,
            ))
        })
        .collect()
}

/// The body slots that the body recipe operands of construction-operand group
/// `group_id` in `scope` resolve, in member order without repeats. `None`
/// when the group is absent or ambiguous, or when a member has an unresolved
/// slot.
pub(super) fn group_operand_body_slots(
    inputs: &FeatureBodySelectionInputs<'_>,
    group_id: &str,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Option<Vec<i64>> {
    let stream = crate::ids::native_stream(&scope.id);
    let mut groups = inputs.groups.iter().filter(|group| {
        group.id == group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let group = groups.next()?;
    if groups.next().is_some() || group.members().is_empty() {
        return None;
    }
    let mut slots = Vec::new();
    for (ordinal, member) in group.members().iter().enumerate() {
        let ordinal = u32::try_from(ordinal).ok()?;
        let mut operands = inputs.body_recipe_operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == member.value
                && crate::ids::native_stream(&operand.id) == stream
        });
        let operand = operands.next()?;
        if operands.next().is_some() {
            return None;
        }
        let slot = operand.resolved_body_slot?;
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    Some(slots)
}

/// Return a body-recipe selection to its native group when its face match
/// selects a model body whose entity number is outside the body slots that
/// the group's operands resolve through their recipe identity.
pub(super) fn keep_recipe_body_slots(
    selection: &mut cadmpeg_ir::features::BodySelection,
    native: cadmpeg_ir::features::BodySelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
) {
    use cadmpeg_ir::features::BodySelection;
    let (BodySelection::Native(group_id), BodySelection::Resolved { bodies, .. }) =
        (&native, &*selection)
    else {
        return;
    };
    let Some(slots) = group_operand_body_slots(inputs, group_id, scope) else {
        return;
    };
    if bodies
        .iter()
        .any(|body| stable_ref(body.as_str()).is_none_or(|slot| !slots.contains(&slot)))
    {
        *selection = native;
    }
}
