// SPDX-License-Identifier: Apache-2.0
//! Selected faces of a healed `DeleteFace`, from its complete transition interval.

use super::output_chain::OutputChain;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory,
};
use cadmpeg_ir::features::FaceSelection;
use std::collections::HashSet;

/// Faces of the preceding state that a healed `DeleteFace` with `members` selects,
/// sorted.
///
/// The operation removes every selected face, and healing can remove further
/// faces. So the deleted faces are the selection only when their count equals
/// the count of distinct members.
fn delete_face_selection(
    members: &[u32],
    previous_state_id: i64,
    transitions: &[&AsmHistoricalTransition],
    preceding: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    if transitions.last()?.previous_state_id != Some(previous_state_id) {
        return None;
    }
    let deleted: HashSet<_> = transitions
        .iter()
        .flat_map(|transition| transition.topology.faces.deleted.iter().copied())
        .collect();
    if transitions.iter().any(|transition| {
        transition
            .topology
            .faces
            .inserted
            .iter()
            .any(|face| deleted.contains(face))
    }) {
        return None;
    }
    let distinct_members = members.iter().collect::<HashSet<_>>().len();
    if deleted.is_empty() || deleted.len() != distinct_members {
        return None;
    }
    let preceding_faces: HashSet<i64> = preceding.faces.iter().copied().collect();
    if !deleted.iter().all(|face| preceding_faces.contains(face)) {
        return None;
    }
    let mut faces: Vec<_> = deleted.into_iter().collect();
    faces.sort_unstable();
    Some(faces)
}

pub(super) struct DeleteFaceStates<'a> {
    pub(super) histories: &'a [AsmHistory],
    pub(super) history: &'a AsmHistory,
    pub(super) state: &'a AsmDeltaState,
    pub(super) previous: &'a AsmDeltaState,
}

/// Bind a native healed `DeleteFace` group through its transition interval.
pub(super) fn bind_delete_face_selection(
    selection: &mut FaceSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    states: &DeleteFaceStates<'_>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    let FaceSelection::Native(native) = selection else {
        return;
    };
    let mut matching = groups.iter().filter(|group| group.id == *native);
    let (Some(group), None) = (matching.next(), matching.next()) else {
        return;
    };
    if group.scope_record_index != scope.record_index {
        return;
    }
    let previous_state_id = states.previous.state_id;
    let Some(chain) = OutputChain::resolve(
        states.histories,
        states.state.state_id,
        previous_state_id,
        Some(&states.history.id),
    ) else {
        return;
    };
    let transitions = chain
        .states
        .iter()
        .map(|state_id| {
            states
                .history
                .states()
                .iter()
                .find(|state| state.state_id == *state_id)?
                .transition
                .as_ref()
        })
        .collect::<Option<Vec<_>>>();
    let members: Vec<u32> = group.members().iter().map(|member| member.value).collect();
    let Some(faces) = states
        .history
        .topology(states.previous)
        .and_then(|preceding| {
            delete_face_selection(&members, previous_state_id, &transitions?, preceding)
        })
    else {
        return;
    };
    let order_assumed = faces.len() >= 2;
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let selected = faces
        .into_iter()
        .map(|slot| {
            crate::ids::history_input_face_id(&prefix, cadmpeg_ir::ids::IdentityKey::from(slot))
        })
        .collect();
    if let Ok(historical) = FaceSelection::historical(
        crate::ids::history_input_state_id(&prefix),
        selected,
        native.clone(),
    ) {
        if order_assumed {
            losses.push(
                crate::loss::F3dLossCode::HistoryDeleteFaceMemberOrderAssumed.note(format!(
                    "DeleteFace group {} assigns selected faces to members in stable-slot order",
                    group.id
                )),
            );
        }
        *selection = historical;
    }
}

#[cfg(test)]
mod tests;
