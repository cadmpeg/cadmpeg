// SPDX-License-Identifier: Apache-2.0
//! History lookup for the linked scopes of one timeline item.

use crate::history_records::{AsmDeltaState, AsmHistory};

/// The state pair of the one history whose BREP holds every candidate face of
/// `operand`, when several histories store the pair. The linked scopes of one
/// timeline item store the same state identities, each in the history of its
/// own component.
pub(super) fn operand_brep_history_state_pair<'a>(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    histories: &'a [AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    let faces = operand
        .references()
        .iter()
        .flat_map(|reference| reference.candidate_faces.iter())
        .collect::<Vec<_>>();
    if faces.is_empty() {
        return None;
    }
    for require_direct in [true, false] {
        let mut pairs = histories.iter().filter_map(|history| {
            let pair = super::state_pair_by(
                history,
                state_id,
                previous_state_id,
                require_direct,
                super::transition_or_header_previous_state_id,
            )?;
            let source = super::historical_brep_source(&pair.2.id)?;
            faces
                .iter()
                .all(|face| super::entity_in_brep_source(face.as_str(), source))
                .then_some(pair)
        });
        if let Some(pair) = pairs.next() {
            return pairs.next().is_none().then_some(pair);
        }
    }
    None
}
