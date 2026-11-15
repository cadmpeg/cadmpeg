// SPDX-License-Identifier: Apache-2.0
//! Which faces a closed-edge recipe may select for an Extrude start or profile.

use std::collections::{HashMap, HashSet};

use super::AsmDeltaState;

use crate::records::topology::face::DesignFaceOperand;

/// Whether any selected face slot is absent from the faces the transition
/// changes.
pub(super) fn selection_has_unchanged_face(slots: &[i64], changed: &HashSet<i64>) -> bool {
    slots.iter().any(|slot| !changed.contains(slot))
}

/// Whether a recipe face replaces the operand's current selection. A recipe
/// without a closed-edge node always does. A closed-edge recipe face fills an
/// empty selection only, and not as an Extrude start or profile face the
/// transition left unchanged: the Extrude joins to or cuts from that face, so
/// the transition changes it.
pub(super) fn recipe_face_replaces_selection(
    closed_edge_node: bool,
    selection_empty: bool,
    unchanged_extrude_face: bool,
) -> bool {
    !closed_edge_node || (selection_empty && !unchanged_extrude_face)
}


/// The state chain a face selection reads.
pub(super) struct Transition<'a> {
    pub(super) state: &'a AsmDeltaState,
    pub(super) previous_state_id: i64,
    pub(super) states: &'a HashMap<i64, Option<&'a AsmDeltaState>>,
}

/// Drop the selection of a closed-edge Extrude start or profile operand that
/// holds a face the transition left unchanged. The Extrude joins to or cuts
/// from its start face, so the transition changes it, and it consumes its
/// profile face, so the transition deletes that face. An Extrude that inserts
/// a body leaves its profile face in place.
pub(super) fn drop_unchanged_closed_edge_selection(
    operand: &mut DesignFaceOperand,
    transition: &Transition<'_>,
    changed_faces: &HashSet<i64>,
    profile: bool,
) {
    if !operand.has_closed_edge_node()
        || transition
            .state
            .transition
            .as_ref()
            .is_some_and(|step| !step.topology.bodies.inserted.is_empty())
    {
        return;
    }
    let required = if profile {
        let Some(deleted) = deleted_faces(transition) else {
            return;
        };
        deleted
    } else {
        changed_faces.clone()
    };
    if selection_has_unchanged_face(&operand.resolved_face_slots, &required) {
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
    }
}

fn deleted_faces(transition: &Transition<'_>) -> Option<HashSet<i64>> {
    let mut current = transition.state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    while current.state_id != transition.previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let step = current.transition.as_ref()?;
        deleted.extend(step.topology.faces.deleted.iter().copied());
        current = step
            .previous_state_id
            .and_then(|id| transition.states.get(&id).copied().flatten())?;
    }
    Some(deleted)
}

#[cfg(test)]
mod tests;
