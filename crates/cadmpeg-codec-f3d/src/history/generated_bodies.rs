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
/// A body pattern seed and a body move resolved in their input states, a body
/// deletion whose operands resolved through their recipe identity, and a
/// Reverse Normal resolved in its input state name ASM body slots. Body slots
/// are local to one ASM history. A pattern seed or body deletion reads its
/// slots in the history bound to the selecting scope (see [`scope_history`]).
/// If that history is unresolved or has no producer, it searches every history
/// and charges a loss. A body move reads its slots in its bound history and
/// keeps its historical selection when that history or a producer is unresolved.
/// When every selected slot is absent from the model bodies, the one projected
/// feature of the same Design stream whose transition inserts the slot
/// generates the body. In a bound history, its scope belongs to that history.
/// A Reverse Normal reads its slots in its bound history: see
/// [`reverse_normal_binding`]. The generating feature becomes a dependency of
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
    let producer =
        |history: Option<&AsmHistory>, stream: Option<&str>, slot: i64| -> Option<Producer> {
            let states: Box<dyn Iterator<Item = &crate::history_records::AsmDeltaState>> =
                match history {
                    Some(history) => Box::new(history.states().iter()),
                    None => Box::new(inputs.histories.iter().flat_map(AsmHistory::states)),
                };
            let mut inserting = states.filter(|state| inserts_body(state, slot));
            let state = inserting.next()?;
            if inserting.next().is_some() {
                return None;
            }
            let mut scopes = inputs.scopes.iter().filter(|scope| {
                crate::ids::native_stream(&scope.id) == stream
                    && scope.history_state_id() == Some(state.state_id)
                    && history.is_none_or(|history| {
                        scope_history(scope, scope_histories, inputs.histories)
                            .is_some_and(|candidate| std::ptr::eq(candidate, history))
                    })
            });
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
        if matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Combine { .. })
        ) {
            bind_combine_producers(&reverse_context, feature, scope, &mut results, losses);
            continue;
        }
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
            FeatureDefinition::Operation(FeatureOperation::MoveBody {
                bodies: BodySelection::Historical { bodies, native, .. },
                ..
            }) => bodies
                .iter()
                .map(|body| historical_body_slot(body.as_str()))
                .collect::<Option<Vec<_>>>()
                .map(|slots| (slots, native.as_str().to_owned())),
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
        let producers = if reverse {
            match reverse_normal_binding(&reverse_context, feature, scope, &slots) {
                Ok(ReverseBinding::Model {
                    bodies,
                    dependencies,
                }) => {
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
            let Some(scope) = scope else {
                continue;
            };
            let history = scope_history(scope, scope_histories, inputs.histories);
            let stream = crate::ids::native_stream(&scope.id);
            let strict = matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::MoveBody { .. })
            );
            if strict && history.is_none() {
                continue;
            }
            let mut fell_back = !strict && history.is_none();
            let mut producers = slots
                .iter()
                .map(|slot| producer(history, stream, *slot))
                .collect::<Option<Vec<_>>>();
            if !strict && producers.is_none() && history.is_some() {
                fell_back = true;
                producers = slots
                    .iter()
                    .map(|slot| producer(None, stream, *slot))
                    .collect::<Option<Vec<_>>>();
            }
            if fell_back {
                losses.push(F3dLossCode::FeatureGeneratedBodyHistoryUnresolved.note(format!(
                    "Feature {} selects body slots {slots:?} of {native} with {}: {}.",
                    feature.id,
                    if history.is_some() {
                        "no producer in its bound ASM history"
                    } else {
                        "no resolved ASM history"
                    },
                    if producers.is_some() {
                        "the one state of any history that inserts each slot is taken as its producer"
                    } else {
                        "the selection keeps its slots"
                    }
                )));
            }
            let Some(producers) = producers else {
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
                | FeatureOperation::MoveBody { bodies, .. }
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

/// The input-state body slots of one Combine operand and the native member
/// of each slot.
struct CombineSide {
    slots: Vec<i64>,
    natives: Vec<String>,
    /// The operand stores one native expression for all its slots.
    single_native: bool,
}

fn combine_side(selection: &cadmpeg_ir::features::BodySelection) -> Option<CombineSide> {
    use cadmpeg_ir::features::BodySelection;
    match selection {
        BodySelection::Historical { bodies, native, .. } => Some(CombineSide {
            slots: bodies
                .iter()
                .map(|body| historical_body_slot(body.as_str()))
                .collect::<Option<_>>()?,
            natives: vec![native.as_str().to_owned()],
            single_native: true,
        }),
        BodySelection::HistoricalSet { members, .. } => {
            let mut slots = Vec::new();
            let mut natives = Vec::new();
            for (body, native) in members.iter().map(|row| row.clone().into_parts()) {
                slots.push(historical_body_slot(body.as_str())?);
                natives.push(native);
            }
            Some(CombineSide {
                slots,
                natives,
                single_native: false,
            })
        }
        _ => None,
    }
}

/// A Combine operand named by model bodies or by the features that generate
/// them, the producers of generated bodies, and the features that become
/// dependencies.
type CombineSideBinding = (
    cadmpeg_ir::features::BodySelection,
    Vec<Producer>,
    Vec<cadmpeg_ir::features::FeatureId>,
);

/// Bind the target and the tools of a Combine resolved in its input state to
/// their producers, each side on its own. A slot that the bound history's
/// BREP source keeps, and that no transition after the Combine's result state
/// updates or deletes, names its model body. A slot that the source omits
/// names the body that its generating feature produces. A side keeps its
/// input-state bodies when its slots mix both readings or name neither.
fn bind_combine_producers(
    context: &ReverseContext<'_>,
    feature: &mut cadmpeg_ir::features::Feature,
    scope: Option<&crate::records::feature::scope::DesignParameterScope>,
    results: &mut Vec<Producer>,
    losses: &mut Vec<LossNote>,
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
    let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
        feature.evaluation.definition()
    else {
        return;
    };
    let sides = [
        ("target", combine_side(operands.target())),
        ("tools", combine_side(operands.tools())),
    ];
    if sides.iter().all(|(_, side)| side.is_none()) {
        return;
    }
    let mut bound: [Option<CombineSideBinding>; 2] = [None, None];
    for ((name, side), slot) in sides.into_iter().zip(&mut bound) {
        let Some(side) = side else { continue };
        match combine_side_binding(context, feature, scope, &side) {
            Ok(binding) => *slot = Some(binding),
            Err(reason) => losses.push(F3dLossCode::FeatureCombineBodyProducerUnresolved.note(
                format!(
                    "Combine {} keeps the input-state bodies of its {name}, slots {:?}: {reason}.",
                    feature.id, side.slots
                ),
            )),
        }
    }
    let [target, tools] = bound;
    if target.is_none() && tools.is_none() {
        return;
    }
    let mut producers = Vec::new();
    let mut dependencies = Vec::new();
    let mut edited = false;
    feature.evaluation.edit(|definition, _| {
        if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) = definition
        {
            edited = operands
                .try_edit(|stored_target, stored_tools| {
                    for (stored, binding) in [(stored_target, target), (stored_tools, tools)] {
                        if let Some((selection, side_producers, side_dependencies)) = binding {
                            *stored = selection;
                            producers.extend(side_producers);
                            dependencies.extend(side_dependencies);
                        }
                    }
                })
                .is_ok();
        }
    });
    if !edited {
        losses.push(F3dLossCode::FeatureCombineBodyProducerUnresolved.note(format!(
            "Combine {} keeps its input-state bodies: the producer bodies of its target and tools overlap.",
            feature.id
        )));
        return;
    }
    for dependency in dependencies {
        feature.dependencies.insert(dependency);
    }
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

fn combine_side_binding(
    context: &ReverseContext<'_>,
    feature: &cadmpeg_ir::features::Feature,
    scope: Option<&crate::records::feature::scope::DesignParameterScope>,
    side: &CombineSide,
) -> Result<CombineSideBinding, String> {
    use cadmpeg_ir::features::{BodyMembers, BodySelection, GeneratedBodyRef};
    let inputs = context.inputs;
    let scope = scope.ok_or("the Combine has no scope")?;
    let history = scope_state_history(scope, context.scope_histories, inputs.histories)
        .ok_or("no one bound history holds its input and result states")?;
    let source = super::historical_brep_source(&history.id)
        .ok_or_else(|| format!("its bound history {} names no BREP source", history.id))?;
    let producer = |slot: i64| -> Option<Producer> {
        let mut inserting = history
            .states()
            .iter()
            .filter(|state| inserts_body(state, slot));
        let state = inserting.next()?;
        if inserting.next().is_some() {
            return None;
        }
        let mut scopes = inputs.scopes.iter().filter(|candidate| {
            candidate.history_state_id() == Some(state.state_id)
                && scope_history(candidate, context.scope_histories, inputs.histories)
                    .is_some_and(|candidate_history| std::ptr::eq(candidate_history, history))
        });
        let candidate = scopes.next()?;
        if scopes.next().is_some() {
            return None;
        }
        let (producer, ordinal) = context.feature_by_scope.get(&candidate.id)?;
        if *ordinal >= feature.ordinal {
            return None;
        }
        let inserted = state.transition.as_ref()?.topology.bodies.inserted.clone();
        Some((producer.clone(), state.state_id, inserted))
    };
    let later = later_body_changes(history, scope)
        .ok_or("its result state has no unique place in the history's state order")?;
    let live = side
        .slots
        .iter()
        .filter(|slot| context.active.contains_in_source(**slot, Some(source)))
        .count();
    let natives = |count: usize| -> Vec<String> {
        if side.single_native {
            std::iter::repeat_n(side.natives[0].clone(), count).collect()
        } else {
            side.natives.clone()
        }
    };
    if live == side.slots.len() {
        if let Some(slot) = side.slots.iter().find(|slot| later.contains(slot)) {
            return Err(format!(
                "a later transition changes slot {slot}, so the model body is not the operand"
            ));
        }
        let bodies = context
            .active
            .resolve(&side.slots, Some(source))
            .ok_or_else(|| format!("a slot names no unique result body of BREP source {source}"))?;
        let dependencies = side
            .slots
            .iter()
            .filter_map(|slot| producer(*slot))
            .map(|(producer, _, _)| producer)
            .collect::<Vec<_>>();
        let selection = if side.single_native {
            BodySelection::Resolved {
                bodies,
                native: side.natives[0].clone(),
            }
        } else {
            let rows = bodies
                .iter()
                .cloned()
                .zip(natives(side.slots.len()))
                .map(|(body, native)| super::body_member(body, native))
                .collect::<Option<Vec<_>>>()
                .ok_or("a native member is blank")?;
            BodySelection::ResolvedSet {
                members: BodyMembers::try_from_rows(rows)
                    .map_err(|error| format!("the model bodies form no body set: {error}"))?,
            }
        };
        return Ok((selection, Vec::new(), dependencies));
    }
    if live != 0 {
        return Err(format!(
            "BREP source {source} keeps some of the slots and omits the others"
        ));
    }
    let producers = side
        .slots
        .iter()
        .map(|slot| producer(*slot))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            format!(
                "a slot has no unique inserting state of {} with one earlier scope",
                history.id
            )
        })?;
    let mut natives = side.natives.iter();
    let native = natives.next().ok_or("the operand has no native member")?;
    if natives.any(|other| other != native) {
        return Err(
            "its native members name several records, and a generated selection keeps one".into(),
        );
    }
    let refs = producers
        .iter()
        .zip(&side.slots)
        .map(|((producer, _, _), slot)| {
            GeneratedBodyRef::new(producer.clone(), slot.to_string()).ok()
        })
        .collect::<Option<Vec<_>>>()
        .ok_or("a generated body identity is malformed")?;
    let selection = BodySelection::generated(refs, native.clone())
        .map_err(|error| format!("the generated bodies form no body set: {error}"))?;
    Ok((selection, producers, Vec::new()))
}

/// The body slots that a transition after the result state of `scope`
/// updates or deletes in `history`.
fn later_body_changes(
    history: &AsmHistory,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Option<HashSet<i64>> {
    let state_id = scope.history_state_id()?;
    let order = history.states_oldest_first()?;
    let states = history.states();
    let mut positions = order.iter().enumerate().filter(|(_, index)| {
        states
            .get(**index)
            .is_some_and(|state| state.state_id == state_id)
    });
    let (position, _) = positions.next()?;
    if positions.next().is_some() {
        return None;
    }
    Some(
        order[position + 1..]
            .iter()
            .filter_map(|index| states.get(*index)?.transition.as_ref())
            .flat_map(|transition| {
                let bodies = &transition.topology.bodies;
                bodies.updated.iter().chain(&bodies.deleted).copied()
            })
            .collect(),
    )
}

struct ReverseContext<'a> {
    inputs: &'a FeatureBodySelectionInputs<'a>,
    scope_histories: &'a HashMap<String, String>,
    active: &'a ActiveBodies<'a>,
    feature_by_scope: &'a HashMap<String, (cadmpeg_ir::features::FeatureId, u64)>,
}

enum ReverseBinding {
    /// Every slot is a result body of the bound history's BREP source.
    Model {
        bodies: cadmpeg_ir::features::DistinctMembers<cadmpeg_ir::ids::BodyId>,
        dependencies: Vec<cadmpeg_ir::features::FeatureId>,
    },
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
    let producer = |slot: i64| -> Option<Producer> {
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
    };
    let live = slots
        .iter()
        .filter(|slot| context.active.contains_in_source(**slot, Some(source)))
        .count();
    if live == slots.len() {
        let bodies = context
            .active
            .resolve(slots, Some(source))
            .ok_or_else(|| format!("a slot names no unique result body of BREP source {source}"))?;
        let dependencies = slots
            .iter()
            .filter_map(|slot| producer(*slot))
            .map(|(producer, _, _)| producer)
            .collect();
        return Ok(ReverseBinding::Model {
            bodies,
            dependencies,
        });
    }
    if live != 0 {
        return Err(format!(
            "BREP source {source} keeps some of the slots and omits the others"
        ));
    }
    slots
        .iter()
        .map(|slot| producer(*slot))
        .collect::<Option<Vec<_>>>()
        .map(ReverseBinding::Generated)
        .ok_or_else(|| {
            format!(
                "a slot has no unique inserting state of {} with one earlier scope",
                history.id
            )
        })
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
