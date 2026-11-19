// SPDX-License-Identifier: Apache-2.0
//! Body selections that name the bodies of earlier features.

use super::{historical_body_slot, stable_ref, FeatureBodySelectionInputs};
use crate::history_records::AsmHistory;
use crate::loss::F3dLossCode;
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{HashMap, HashSet};

/// Re-express body selections of ASM body slots as bodies of the features
/// that generate them.
///
/// A body pattern seed resolved in its input state, a body deletion whose
/// operands resolved through their recipe identity, and a Reverse Normal
/// resolved in its input state name ASM body slots. When every selected slot
/// is absent from the model bodies, the one feature whose history transition
/// inserts that slot generates the body, and the selection names it there.
/// When every slot of a Reverse Normal is the entity number of exactly one
/// model body and has such a generating feature, the selection names those
/// model bodies. The generating feature becomes a dependency of the selecting
/// feature. Returns the result states of the producers of generated bodies,
/// each listing every body slot that the producer's transition inserts.
pub(crate) fn bind_generated_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
    losses: &mut Vec<LossNote>,
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
            FeatureDefinition::Operation(FeatureOperation::ReverseShape {
                source: BodySelection::Historical { bodies, native, .. },
            }) => bodies
                .iter()
                .map(|body| historical_body_slot(body.as_str()))
                .collect::<Option<Vec<_>>>()
                .map(|slots| (slots, native.as_str().to_owned())),
            _ => continue,
        };
        let reverse = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::ReverseShape { .. })
        );
        let Some((slots, native)) = selected else {
            if reverse {
                losses.push(F3dLossCode::FeatureReverseNormalBodyProducerUnresolved.note(format!(
                    "Reverse Normal {} keeps its input-state bodies: a body names no ASM body slot.",
                    feature.id
                )));
            }
            continue;
        };
        if slots.is_empty() {
            continue;
        }
        let live_count = slots.iter().filter(|slot| live.contains(slot)).count();
        let keep_input_state = |losses: &mut Vec<LossNote>, reason: &str| {
            if reverse {
                losses.push(F3dLossCode::FeatureReverseNormalBodyProducerUnresolved.note(format!(
                    "Reverse Normal {} keeps body slots {slots:?} of {native} in its input state: {reason}.",
                    feature.id
                )));
            }
        };
        if live_count != 0 && !(reverse && live_count == slots.len()) {
            keep_input_state(
                losses,
                "the final model keeps some of the slots and omits the others",
            );
            continue;
        }
        let Some(producers) = slots
            .iter()
            .map(|slot| producer(*slot))
            .collect::<Option<Vec<_>>>()
        else {
            keep_input_state(
                losses,
                "a slot has no unique history state and scope that insert it",
            );
            continue;
        };
        if live_count != 0 {
            let Some(bodies) = slots
                .iter()
                .map(|slot| model_body(inputs, *slot))
                .collect::<Option<Vec<_>>>()
            else {
                keep_input_state(
                    losses,
                    "a slot is the entity number of several model bodies",
                );
                continue;
            };
            let Ok(bodies) = cadmpeg_ir::features::DistinctMembers::try_from(bodies) else {
                keep_input_state(losses, "the model bodies form no body set");
                continue;
            };
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::ReverseShape { source }) =
                    definition
                {
                    *source = BodySelection::Resolved { bodies, native };
                }
            });
            for (producer, _, _) in producers {
                feature.dependencies.insert(producer);
            }
            continue;
        }
        let Some(refs) = producers
            .iter()
            .zip(&slots)
            .map(|((producer, _, _), slot)| {
                GeneratedBodyRef::new(producer.clone(), slot.to_string()).ok()
            })
            .collect::<Option<Vec<_>>>()
        else {
            keep_input_state(losses, "a generated body has no feature-local identity");
            continue;
        };
        let Ok(generated) = BodySelection::generated(refs, native.clone()) else {
            keep_input_state(losses, "the generated bodies form no body set");
            continue;
        };
        feature.evaluation.edit(|definition, _| match definition {
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                if let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() {
                    *selection = generated;
                }
            }
            FeatureDefinition::Operation(
                FeatureOperation::DeleteBody { bodies, .. }
                | FeatureOperation::ReverseShape { source: bodies },
            ) => {
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

/// The model body whose entity number is `slot`, when exactly one has it.
fn model_body(
    inputs: &FeatureBodySelectionInputs<'_>,
    slot: i64,
) -> Option<cadmpeg_ir::ids::BodyId> {
    let mut bodies = inputs
        .bodies
        .iter()
        .filter(|body| stable_ref(body.id.as_str()) == Some(slot));
    let body = bodies.next()?;
    bodies.next().is_none().then(|| body.id.clone())
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
