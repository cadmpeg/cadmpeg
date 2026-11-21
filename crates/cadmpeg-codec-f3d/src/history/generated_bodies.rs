// SPDX-License-Identifier: Apache-2.0
//! Body selections that name the bodies of earlier features.

use super::stored_bodies::ActiveBodies;
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
/// A Reverse Normal reads its slots in its bound history: see
/// [`reverse_normal_binding`]. A body pattern seed whose every slot is a model
/// body selects those model bodies: see [`live_seed_binding`]. The generating
/// feature becomes a dependency of
/// the selecting feature. Returns the result states of the producers of
/// generated bodies, each listing every body slot that the producer's
/// transition inserts.
pub(crate) fn bind_generated_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
    scope_histories: &HashMap<String, String>,
    primary_brep: Option<&str>,
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
    let active = ActiveBodies::new(inputs.bodies, primary_brep);
    let feature_by_scope = features
        .iter()
        .filter_map(|feature| {
            Some((
                feature.native_ref.clone()?,
                (feature.id.clone(), feature.ordinal),
            ))
        })
        .collect::<HashMap<_, _>>();
    let producer = |slot: i64| -> Option<Producer> {
        let mut inserting = inputs
            .histories
            .iter()
            .flat_map(AsmHistory::states)
            .filter(|state| inserts_body(state, slot));
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
            feature_by_scope.get(&scope.id)?.0.clone(),
            state.state_id,
            inserted,
        ))
    };
    let reverse_context = ReverseContext {
        inputs,
        scope_histories,
        active: &active,
        feature_by_scope: &feature_by_scope,
    };

    let mut results: Vec<Producer> = Vec::new();
    for feature in features.iter_mut() {
        let scope = feature
            .native_ref
            .as_deref()
            .and_then(|native| inputs.scopes.iter().find(|scope| scope.id == native));
        let mut seed_state = None;
        let selected = match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                let [PatternSeed::Bodies(BodySelection::Historical {
                    state,
                    bodies,
                    native,
                })] = seeds.as_slice()
                else {
                    continue;
                };
                seed_state = historical_state_number(state.as_str());
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
        let pattern = matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
        );
        if pattern && slots.iter().all(|slot| live.contains(slot)) {
            let Some(ModelBodies {
                bodies,
                dependencies,
            }) = scope.zip(seed_state).and_then(|(scope, seed_state)| {
                live_seed_binding(&reverse_context, feature, scope, seed_state, &slots)
            })
            else {
                continue;
            };
            let resolved = BodySelection::Resolved { bodies, native };
            feature.evaluation.edit(|definition, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
                    definition
                {
                    if let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() {
                        *selection = resolved;
                    }
                }
            });
            for dependency in dependencies {
                feature.dependencies.insert(dependency);
            }
            continue;
        }
        let producers = if reverse {
            match reverse_normal_binding(&reverse_context, feature, scope, &slots) {
                Ok(ReverseBinding::Model(ModelBodies {
                    bodies,
                    dependencies,
                })) => {
                    feature.evaluation.edit(|definition, _| {
                        if let FeatureDefinition::Operation(FeatureOperation::ReverseShape {
                            source,
                        }) = definition
                        {
                            *source = BodySelection::Resolved { bodies, native };
                        }
                    });
                    for dependency in dependencies {
                        feature.dependencies.insert(dependency);
                    }
                    continue;
                }
                Ok(ReverseBinding::Generated(producers)) => producers,
                Err(reason) => {
                    losses.push(F3dLossCode::FeatureReverseNormalBodyProducerUnresolved.note(
                        format!(
                            "Reverse Normal {} keeps body slots {slots:?} of {native} in its input state: {reason}.",
                            feature.id
                        ),
                    ));
                    continue;
                }
            }
        } else {
            if slots.iter().any(|slot| live.contains(slot)) {
                continue;
            }
            let Some(producers) = slots
                .iter()
                .map(|slot| producer(*slot))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            producers
        };
        let generated = producers
            .iter()
            .zip(&slots)
            .map(|((producer, _, _), slot)| {
                GeneratedBodyRef::new(producer.clone(), slot.to_string()).ok()
            })
            .collect::<Option<Vec<_>>>()
            .and_then(|refs| BodySelection::generated(refs, native.clone()).ok());
        let Some(generated) = generated else {
            if reverse {
                losses.push(F3dLossCode::FeatureReverseNormalBodyProducerUnresolved.note(format!(
                    "Reverse Normal {} keeps body slots {slots:?} of {native} in its input state: the generated bodies form no body set.",
                    feature.id
                )));
            }
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

/// A generating feature, the identity of the history state whose transition
/// inserts the body, and every body slot that transition inserts.
type Producer = (cadmpeg_ir::features::FeatureId, i64, Vec<i64>);

struct ReverseContext<'a> {
    inputs: &'a FeatureBodySelectionInputs<'a>,
    scope_histories: &'a HashMap<String, String>,
    active: &'a ActiveBodies<'a>,
    feature_by_scope: &'a HashMap<String, (cadmpeg_ir::features::FeatureId, u64)>,
}

/// The model bodies of a selection of body slots, with the earlier features
/// whose transitions insert those slots.
struct ModelBodies {
    bodies: cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::ids::BodyId>,
    dependencies: Vec<cadmpeg_ir::features::FeatureId>,
}

enum ReverseBinding {
    /// Every slot is a result body of the bound history's BREP source.
    Model(ModelBodies),
    /// No slot is a result body of that source; each has a generating feature.
    Generated(Vec<Producer>),
}

/// The binding of a Reverse Normal input-state selection of `slots`, read in
/// the feature's bound history and the model bodies of that history's BREP
/// source. A body slot's generating feature precedes the Reverse Normal.
fn reverse_normal_binding(
    context: &ReverseContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    scope: Option<&crate::records::feature::scope::DesignParameterScope>,
    slots: &[i64],
) -> Result<ReverseBinding, String> {
    let inputs = context.inputs;
    let history = scope
        .and_then(|scope| scope_state_history(scope, context.scope_histories, inputs.histories))
        .ok_or("no one bound history holds its input and result states")?;
    let source = super::historical_brep_source(&history.id)
        .ok_or_else(|| format!("its bound history {} names no BREP source", history.id))?;
    let live = slots
        .iter()
        .filter(|slot| context.active.contains_in_source(**slot, Some(source)))
        .count();
    if live == slots.len() {
        return history_model_bodies(context, feature, history, source, slots)
            .map(ReverseBinding::Model)
            .ok_or_else(|| format!("a slot names no unique result body of BREP source {source}"));
    }
    if live != 0 {
        return Err(format!(
            "BREP source {source} keeps some of the slots and omits the others"
        ));
    }
    slots
        .iter()
        .map(|slot| history_producer(context, feature, history, *slot))
        .collect::<Option<Vec<_>>>()
        .map(ReverseBinding::Generated)
        .ok_or_else(|| {
            format!(
                "a slot has no unique inserting state of {} with one earlier scope",
                history.id
            )
        })
}

/// The model bodies of a body-pattern seed of `slots` in state `seed_state`,
/// read in the feature's bound history as a Reverse Normal reads its slots.
/// The walk from that history's terminal state back to the seed state passes
/// the scope's input state, and no transition on it updates or deletes a slot.
fn live_seed_binding(
    context: &ReverseContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    scope: &crate::records::feature::scope::DesignParameterScope,
    seed_state: i64,
    slots: &[i64],
) -> Option<ModelBodies> {
    let histories = context.inputs.histories;
    let history = scope_state_history(scope, context.scope_histories, histories)?;
    let previous = super::effective_scope_previous_history_state_id(scope, histories)?;
    let mut current = super::terminal_state(history)?.state_id;
    let mut passed_previous = false;
    let mut steps = 0;
    while current != seed_state {
        passed_previous |= current == previous;
        steps += 1;
        if steps > history.states().len() {
            return None;
        }
        let state = history
            .states()
            .iter()
            .find(|state| state.state_id == current)?;
        let delta = &state.transition.as_ref()?.topology;
        if slots
            .iter()
            .any(|slot| delta.bodies.updated.contains(slot) || delta.bodies.deleted.contains(slot))
            || [
                &delta.regions,
                &delta.shells,
                &delta.faces,
                &delta.loops,
                &delta.coedges,
                &delta.edges,
                &delta.vertices,
            ]
            .into_iter()
            .any(|family| !family.updated.is_empty() || !family.deleted.is_empty())
        {
            return None;
        }
        current = super::transition_or_header_previous_state_id(history, state)?;
    }
    if !(passed_previous || current == previous) {
        return None;
    }
    let source = super::historical_brep_source(&history.id)?;
    history_model_bodies(context, feature, history, source, slots)
}

/// The result bodies of `slots` in BREP source `source` of `history`, with the
/// generating feature of each slot that has one.
fn history_model_bodies(
    context: &ReverseContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    history: &AsmHistory,
    source: &str,
    slots: &[i64],
) -> Option<ModelBodies> {
    let bodies = context.active.resolve(slots, Some(source))?;
    let dependencies = slots
        .iter()
        .filter_map(|slot| history_producer(context, feature, history, *slot))
        .map(|(producer, _, _)| producer)
        .collect();
    Some(ModelBodies {
        bodies,
        dependencies,
    })
}

/// The generating feature of body slot `slot` in `history`: the feature of
/// the one scope of that history whose state is the one state that inserts
/// the slot. That feature precedes `feature`.
fn history_producer(
    context: &ReverseContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    history: &AsmHistory,
    slot: i64,
) -> Option<Producer> {
    let inputs = context.inputs;
    let mut inserting = history
        .states()
        .iter()
        .filter(|state| inserts_body(state, slot));
    let state = inserting.next()?;
    if inserting.next().is_some() {
        return None;
    }
    let mut scopes = inputs.scopes.iter().filter(|scope| {
        scope.history_state_id() == Some(state.state_id)
            && scope_history(scope, context.scope_histories, inputs.histories)
                .is_some_and(|scope_history| std::ptr::eq(scope_history, history))
    });
    let scope = scopes.next()?;
    if scopes.next().is_some() {
        return None;
    }
    let (producer, ordinal) = context.feature_by_scope.get(&scope.id)?;
    if *ordinal >= feature.ordinal {
        return None;
    }
    let inserted = state.transition.as_ref()?.topology.bodies.inserted.clone();
    Some((producer.clone(), state.state_id, inserted))
}

/// The ASM state number of a history-input state identity.
fn historical_state_number(id: &str) -> Option<i64> {
    id.strip_prefix("f3d:history-input:state#")?
        .rsplit_once(':')?
        .1
        .parse()
        .ok()
}

fn inserts_body(state: &crate::history_records::AsmDeltaState, slot: i64) -> bool {
    state
        .transition
        .as_ref()
        .is_some_and(|transition| transition.topology.bodies.inserted.contains(&slot))
}

/// The one ASM history that holds the result and input states of `scope`: its
/// bound history, else the unique history with that state pair.
fn scope_state_history<'h>(
    scope: &crate::records::feature::scope::DesignParameterScope,
    scope_histories: &HashMap<String, String>,
    histories: &'h [AsmHistory],
) -> Option<&'h AsmHistory> {
    let state_id = scope.history_state_id()?;
    let previous = super::effective_scope_previous_history_state_id(scope, histories)?;
    if scope_histories.contains_key(&scope.id) {
        super::bound_history_state_pair(&scope.id, state_id, previous, scope_histories, histories)
    } else {
        super::unique_history_state_pair(histories, state_id, previous)
    }
    .map(|(history, _, _)| history)
}

/// The ASM history of `scope`: its bound history, else the unique history
/// with its state pair, else the unique history with its result state.
fn scope_history<'h>(
    scope: &crate::records::feature::scope::DesignParameterScope,
    scope_histories: &HashMap<String, String>,
    histories: &'h [AsmHistory],
) -> Option<&'h AsmHistory> {
    if scope_histories.contains_key(&scope.id) {
        return super::bound_scope_history(&scope.id, scope_histories, histories);
    }
    let state_id = scope.history_state_id()?;
    match super::effective_scope_previous_history_state_id(scope, histories) {
        Some(previous) => super::unique_history_state_pair(histories, state_id, previous)
            .map(|(history, _, _)| history),
        None => super::unique_history_state(histories, state_id).map(|(history, _)| history),
    }
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
