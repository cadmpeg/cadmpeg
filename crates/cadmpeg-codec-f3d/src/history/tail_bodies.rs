// SPDX-License-Identifier: Apache-2.0
//! Body selections whose group members are whole-body reference tails.

use super::body_selections::FeatureBodySelectionInputs;
use super::history_states::unique_history_state_pair;
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
        let mut tails = inputs.body_reference_tails.iter().filter(|tail| {
            tail.record_index == u64::from(member.value)
                && crate::ids::native_stream(&tail.id) == Some(stream)
        });
        let tail = tails.next()?;
        if tails.next().is_some() || tail.second_body_nodes.is_empty() {
            return None;
        }
        nodes.extend(tail.second_body_nodes.iter().copied());
    }
    Some((group.id.clone(), nodes))
}

/// Bind a native body group to the history bodies that its tail nodes name in
/// the preceding history state.
///
/// A node names the body that the transition into the history state of its
/// creating scope inserts. The slot is the body of the preceding state when
/// the creating scope reads the history of `scope`, its state is unique, and
/// that state inserts exactly one body that the preceding state holds. The
/// selection binds when every node names a distinct slot.
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
    let Some((native, nodes)) = group_nodes(group_id, scope, inputs) else {
        return;
    };
    let Some(stream) = crate::ids::native_stream(&scope.id) else {
        return;
    };
    let Some(state_id) = scope.history_state_id() else {
        return;
    };
    let Some((history, _, previous)) =
        unique_history_state_pair(inputs.histories, state_id, previous_state_id)
    else {
        return;
    };
    let Some(input) = history.topology(previous) else {
        return;
    };
    let mut slots = Vec::new();
    for node in nodes {
        let mut creators = inputs.scopes.iter().filter(|candidate| {
            candidate.record_index == node.creating_scope_record_index
                && crate::ids::native_stream(&candidate.id) == Some(stream)
        });
        let (Some(creator), None) = (creators.next(), creators.next()) else {
            return;
        };
        if inputs
            .scope_histories
            .get(&creator.id)
            .is_some_and(|bound| *bound != history.id)
        {
            return;
        }
        let Some(creator_state) = creator.history_state_id() else {
            return;
        };
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == creator_state);
        let (Some(state), None) = (states.next(), states.next()) else {
            return;
        };
        let inserted = state.transition.as_ref().map_or(&[][..], |transition| {
            &transition.topology.bodies.inserted[..]
        });
        let slot = match inserted {
            [slot] => slot,
            [_, _, ..] => {
                let Some(ordinal) = node_body_ordinal(node, stream, &history.id, inputs) else {
                    return;
                };
                let Some(slot) = inserted.iter().find(|slot| **slot == ordinal) else {
                    return;
                };
                slot
            }
            [] => return,
        };
        if !input.bodies.contains(slot) || slots.contains(slot) {
            return;
        }
        slots.push(*slot);
    }
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

/// The body slot that a browser node names when its creating transition
/// inserts several bodies: the ordinal of the node's solved body in the BREP
/// blob of the history. The ordinal is the number of the solved body's
/// `brep:entity#` identity. The node's body-map pair must name the blob of
/// the history.
fn node_body_ordinal(
    node: DesignCombineBodyNode,
    stream: &str,
    history_id: &str,
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Option<i64> {
    let mut pairs = inputs.body_bindings.iter().filter(|pair| {
        pair.entity_suffix == u64::from(node.body_entity_suffix)
            && crate::ids::native_stream(pair.id()) == Some(stream)
    });
    let (Some(pair), None) = (pairs.next(), pairs.next()) else {
        return None;
    };
    if !history_id.contains(pair.blob_name()) {
        return None;
    }
    pair.body
        .as_ref()?
        .as_str()
        .rsplit_once("brep:entity#")?
        .1
        .parse()
        .ok()
}

/// Bind a native body group to the solved bodies of the Design body-map pairs
/// that its tail nodes name. Every node names one pair of its stream whose
/// body is solved and hidden in the model, and no two nodes name one body.
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
        // A removed body stays in its BREP blob with its model body hidden.
        // A visible body is not shown to be the removed body.
        if !inputs
            .bodies
            .iter()
            .any(|candidate| candidate.id == body && candidate.visible == Some(false))
        {
            return;
        }
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
                notes.push(crate::loss::F3dLossCode::FeatureBodyTailUnbound.note(format!(
                    "feature {} names its bodies through browser nodes of {group} that bind no body",
                    feature.id
                )));
            }
        }
    }
    notes
}
