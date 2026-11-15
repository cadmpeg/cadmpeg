// SPDX-License-Identifier: Apache-2.0
//! Which faces a closed-edge recipe may select for an Extrude start or profile.

use std::collections::HashSet;

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


/// Drop the selection of a closed-edge Extrude start or profile operand that
/// holds a face the transition left unchanged. The Extrude joins to or cuts
/// from its start or profile face, so the transition changes that face.
pub(super) fn drop_unchanged_closed_edge_selection(
    operand: &mut DesignFaceOperand,
    changed_faces: &HashSet<i64>,
) {
    if operand.has_closed_edge_node()
        && selection_has_unchanged_face(&operand.resolved_face_slots, changed_faces)
    {
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
    }
}

#[cfg(test)]
mod tests;
