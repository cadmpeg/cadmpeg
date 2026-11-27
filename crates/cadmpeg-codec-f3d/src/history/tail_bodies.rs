// SPDX-License-Identifier: Apache-2.0
//! Body selections whose group members are whole-body reference tails.

use super::body_selections::FeatureBodySelectionInputs;
use super::face_resolution::historical_brep_source;
use super::history_states::{history_state_pair, transition_or_header_previous_state_id};
use crate::history_records::AsmHistory;
use crate::records::feature::combine::DesignCombineBodyNode;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::selections::{BodySelection, DistinctMembers};
use cadmpeg_ir::features::FeatureId;

/// The body browser nodes that the members of the native body group
/// `group_id` of `scope` name through their reference tails. Every member is
/// one tail record that names nodes.
fn group_nodes(
    group_id: &str,
    scope: &DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Option<(String, Vec<DesignCombineBodyNode>)> {
    let stream = crate::ids::native_stream(&scope.id)?;
    let mut groups = inputs.groups.iter().filter(|group| {
        group.id == group_id
            && group.scope_record_index == scope.record_index
            && matches!(
                group.role(),
                DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B
            )
            && crate::ids::native_stream(&group.id) == Some(stream)
    });
    let group = groups.next()?;
    if groups.next().is_some() || group.members().is_empty() {
        return None;
    }
    let mut nodes = Vec::new();
    for member in group.members() {
        let mut tails = inputs.body_tail_nodes.iter().filter(|tail| {
            tail.tail_record_index == u64::from(member.value)
                && crate::ids::native_stream(&tail.tail_id) == Some(stream)
        });
        let tail = tails.next()?;
        if tails.next().is_some() || tail.nodes.is_empty() {
            return None;
        }
        nodes.extend(tail.nodes.iter().copied());
    }
    Some((group.id.clone(), nodes))
}

/// The history that `scope` is bound to.
fn bound_history<'a>(
    scope: &DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'a>,
) -> Option<&'a AsmHistory> {
    let bound = inputs.scope_histories.get(&scope.id)?;
    let mut histories = inputs
        .histories
        .iter()
        .filter(|history| history.id == *bound);
    let history = histories.next()?;
    histories.next().is_none().then_some(history)
}

/// Whether `ancestor` is `state_id` or a state that the predecessor links of
/// `history` reach from it.
fn state_reaches(history: &AsmHistory, state_id: i64, ancestor: i64) -> bool {
    let mut current = state_id;
    for _ in 0..=history.states().len() {
        if current == ancestor {
            return true;
        }
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == current);
        let (Some(state), None) = (states.next(), states.next()) else {
            return false;
        };
        match transition_or_header_previous_state_id(history, state) {
            Some(previous) => current = previous,
            None => return false,
        }
    }
    false
}

/// The ordinal of the solved body that the body-map pair of `node` names, as
/// the number of its `brep:entity#` identity. `Ok(None)` when the node's body
/// has no solved body. `Err` when the pair names another BREP blob than
/// `history` or its identity carries no number.
fn node_body_ordinal(
    node: DesignCombineBodyNode,
    stream: &str,
    history: &AsmHistory,
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<Option<i64>, ()> {
    let mut pairs = inputs.body_bindings.iter().filter(|pair| {
        pair.entity_suffix == u64::from(node.body_entity_suffix)
            && crate::ids::native_stream(pair.id()) == Some(stream)
    });
    let (Some(pair), None) = (pairs.next(), pairs.next()) else {
        return Ok(None);
    };
    let Some(body) = pair.body.as_ref() else {
        return Ok(None);
    };
    if historical_brep_source(&history.id) != pair.blob_name().strip_prefix("BREP.") {
        return Err(());
    }
    body.as_str()
        .rsplit_once("brep:entity#")
        .and_then(|(_, number)| number.parse().ok())
        .map(Some)
        .ok_or(())
}

/// The body slots of the preceding history state that the tail nodes of the
/// native body group `group_id` name, with the native group identity, or the
/// reason that they name none.
fn tail_history_slots(
    group_id: &str,
    previous_state_id: i64,
    scope: &DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Result<(String, Vec<i64>), &'static str> {
    let (native, nodes) = group_nodes(group_id, scope, inputs)
        .ok_or("its group members are not single tail records that name nodes")?;
    let stream = crate::ids::native_stream(&scope.id).ok_or("its scope has no stream")?;
    let state_id = scope
        .history_state_id()
        .ok_or("its scope stores no history state")?;
    let history = bound_history(scope, inputs).ok_or("its scope is bound to no history")?;
    let (_, _, previous) = history_state_pair(history, state_id, previous_state_id, false)
        .ok_or("its history state pair is not unique")?;
    let input = history
        .topology(previous)
        .ok_or("its preceding history state has no topology")?;
    let mut slots = Vec::new();
    for node in nodes {
        let mut creators = inputs.scopes.iter().filter(|candidate| {
            candidate.record_index == node.creating_scope_record_index
                && crate::ids::native_stream(&candidate.id) == Some(stream)
        });
        let (Some(creator), None) = (creators.next(), creators.next()) else {
            return Err("a node's creating scope is not a single decoded scope");
        };
        if inputs.scope_histories.get(&creator.id) != Some(&history.id) {
            return Err("a node's creating scope reads another history");
        }
        let creator_state = creator
            .history_state_id()
            .ok_or("a node's creating scope has no history state")?;
        if !state_reaches(history, previous_state_id, creator_state) {
            return Err("a node's creating state does not precede the consuming scope");
        }
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == creator_state);
        let (Some(state), None) = (states.next(), states.next()) else {
            return Err("a node's creating history state is not unique");
        };
        let inserted = state.transition.as_ref().map_or(&[][..], |transition| {
            &transition.topology.bodies.inserted[..]
        });
        let Ok(ordinal) = node_body_ordinal(node, stream, history, inputs) else {
            return Err("a node's body-map pair names another history");
        };
        let slot = match (inserted, ordinal) {
            ([slot], None) => slot,
            ([slot], Some(ordinal)) if *slot == ordinal => slot,
            ([_, _, ..], Some(ordinal)) => inserted
                .iter()
                .find(|slot| **slot == ordinal)
                .ok_or("a node's solved body is not inserted by its creating state")?,
            _ => return Err("a node's creating state does not supply one body"),
        };
        if !input.bodies.contains(slot) {
            return Err("a node's body is absent from the preceding history state");
        }
        if slots.contains(slot) {
            return Err("two nodes name one body");
        }
        slots.push(*slot);
    }
    Ok((native, slots))
}

/// Bind a native body group to the history bodies that its tail nodes name in
/// the preceding history state.
///
/// A node names the body that the transition into the history state of its
/// creating scope inserts. The consuming scope and the creating scope are
/// bound to one history, and the creating state is the preceding state of the
/// consuming scope or one of its ancestors. A transition that inserts one body
/// supplies it. A transition that inserts several bodies supplies the body
/// whose slot is the ordinal of the node's solved body. The slot must be one
/// that the preceding state holds, and a solved body of the node must agree
/// with the slot. The selection binds when every node names a distinct slot.
pub(super) fn bind_tail_history_bodies(
    selection: &mut BodySelection,
    feature_id: &FeatureId,
    previous_state_id: i64,
    scope: &DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
) {
    let BodySelection::Native(group_id) = selection else {
        return;
    };
    let Ok((native, slots)) = tail_history_slots(group_id, previous_state_id, scope, inputs) else {
        return;
    };
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let bodies = slots
        .into_iter()
        .map(|slot| crate::ids::history_input_body_id(&prefix, slot))
        .collect();
    if let Ok(historical) =
        BodySelection::historical(crate::ids::history_input_state_id(&prefix), bodies, native)
    {
        *selection = historical;
    }
}

/// Bind a native body group to the solved bodies of the Design body-map pairs
/// that its tail nodes name. Every node names one pair of its stream whose
/// body is solved, and no two nodes name one body.
pub(super) fn bind_tail_model_bodies(
    selection: &mut BodySelection,
    scope: &DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'_>,
) {
    let BodySelection::Native(group_id) = selection else {
        return;
    };
    let Some((native, nodes)) = group_nodes(group_id, scope, inputs) else {
        return;
    };
    let Some(stream) = crate::ids::native_stream(&scope.id) else {
        return;
    };
    let mut bodies = Vec::new();
    for node in nodes {
        let mut pairs = inputs.body_bindings.iter().filter(|pair| {
            pair.entity_suffix == u64::from(node.body_entity_suffix)
                && crate::ids::native_stream(pair.id()) == Some(stream)
        });
        let (Some(pair), None) = (pairs.next(), pairs.next()) else {
            return;
        };
        let Some(body) = pair.body.clone() else {
            return;
        };
        bodies.push(body);
    }
    if let Ok(bodies) = DistinctMembers::try_from(bodies) {
        *selection = BodySelection::Resolved { bodies, native };
    }
}

/// The loss notes of the Pattern and Remove Body selections in `features`
/// whose tail-named bodies stay native after binding.
pub(super) fn unbound_tail_notes(
    features: &[cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::features::patterns::PatternSeed;
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let mut notes = Vec::new();
    for feature in features {
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut scopes = inputs.scopes.iter().filter(|scope| scope.id == native_ref);
        let (Some(scope), None) = (scopes.next(), scopes.next()) else {
            continue;
        };
        let mut native = Vec::new();
        match feature.evaluation.definition() {
            FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) => {
                for seed in seeds {
                    if let PatternSeed::Bodies(BodySelection::Native(group)) = seed {
                        native.push(group.as_str());
                    }
                }
            }
            FeatureDefinition::Operation(FeatureOperation::DeleteBody {
                bodies: BodySelection::Native(group),
                ..
            }) => native.push(group.as_str()),
            _ => {}
        }
        for group in native {
            if group_nodes(group, scope, inputs).is_some() {
                let reason = match super::history_states::effective_scope_previous_history_state_id(
                    scope,
                    inputs.histories,
                ) {
                    Some(previous) => match tail_history_slots(group, previous, scope, inputs) {
                        Err(reason) => reason,
                        Ok(_) => "its bodies are not selected in the preceding history state",
                    },
                    None => "its scope has no preceding history state",
                };
                notes.push(crate::loss::F3dLossCode::FeatureBodyTailUnbound.note(format!(
                    "feature {} names its bodies through browser nodes of {group} that bind no body: {reason}",
                    feature.id
                )));
            }
        }
    }
    notes
}
