// SPDX-License-Identifier: Apache-2.0
//! Which faces a closed-edge recipe may select for an Extrude start or profile.

use std::collections::{HashMap, HashSet};

use crate::history_records::AsmDeltaState;

use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;
use crate::records::topology::face::DesignFaceOperand;

/// Whether the selection is not empty and none of its face slots is among the
/// faces the transition deletes.
pub(super) fn selection_has_no_deleted_face(slots: &[i64], deleted: &HashSet<i64>) -> bool {
    !slots.is_empty() && !slots.iter().any(|slot| deleted.contains(slot))
}

/// Whether a recipe face replaces the operand's current selection. A recipe
/// without a closed-edge node always does. A closed-edge recipe face fills an
/// empty selection only, and only as an Extrude start or profile face that
/// the transition changes: the Extrude joins to or cuts from that face, so
/// the transition changes it. Any other face is `unproven`.
pub(super) fn recipe_face_replaces_selection(
    closed_edge_node: bool,
    selection_empty: bool,
    unproven: bool,
) -> bool {
    !closed_edge_node || (selection_empty && !unproven)
}

/// The state chain a face selection reads.
pub(super) struct Transition<'a> {
    pub(super) state: &'a AsmDeltaState,
    pub(super) previous_state_id: i64,
    pub(super) states: &'a HashMap<i64, Option<&'a AsmDeltaState>>,
}

/// Drop the selection of a closed-edge Extrude profile operand that holds a
/// no face the transition deletes. The sweep consumes its profile face, so the
/// transition deletes at least one of the selected faces. An Extrude that makes a new body leaves
/// its profile face in place.
pub(super) fn drop_unconsumed_closed_edge_profile(
    operand: &mut DesignFaceOperand,
    transition: &Transition<'_>,
    new_body: bool,
) {
    if !operand.has_closed_edge_node() || new_body {
        return;
    }
    let Some(deleted) = deleted_faces(transition) else {
        return;
    };
    if selection_has_no_deleted_face(&operand.resolved_face_slots, &deleted) {
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
    }
}

/// Whether the operand belongs to an Extrude profile group of its scope.
pub(super) fn is_extrude_profile_member(
    operand: &DesignFaceOperand,
    scope: &DesignParameterScope,
    groups: &[DesignConstructionOperandGroup],
    extrude: bool,
) -> bool {
    let Some(record) = operand.group_record_index().filter(|_| extrude) else {
        return false;
    };
    let stream = crate::ids::native_stream(&operand.id);
    groups.iter().any(|group| {
        crate::ids::native_stream(&group.id) == stream
            && group.scope_record_index == scope.record_index
            && group.record_index == record
            && group.extrude_role() == Some(DesignExtrudeOperandRole::Profile)
    })
}

/// Whether the scope is an Extrude that makes a new body.
pub(super) fn is_new_body_extrude(scope: &DesignParameterScope) -> bool {
    matches!(
        scope.payload(),
        DesignScopePayload::Extrude(Some(extrude))
            if extrude.extrude_prologue.as_ref().is_some_and(|prologue| {
                prologue.operation() == DesignExtrudeOperation::NewBody
            })
    )
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
