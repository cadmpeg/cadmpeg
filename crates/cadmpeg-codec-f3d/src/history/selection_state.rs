// SPDX-License-Identifier: Apache-2.0
//! The history state pair that a feature's face selections read.

use std::collections::HashMap;

use super::{
    bound_history_state_pair, effective_scope_previous_history_state_id, history_state_indexes,
    unique_history_state_pair, AsmHistory, FaceSelectionState,
};

pub(super) fn face_selection_state<'a>(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
) -> Option<FaceSelectionState<'a>> {
    face_selection_state_with(feature, scopes, scope_histories, histories, false)
}

/// The state pair of a feature whose result state may follow its preceding
/// state through several transitions.
pub(super) fn face_selection_state_spanning<'a>(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
) -> Option<FaceSelectionState<'a>> {
    face_selection_state_with(feature, scopes, scope_histories, histories, true)
}

fn face_selection_state_with<'a>(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
    spanning: bool,
) -> Option<FaceSelectionState<'a>> {
    let native_ref = feature.native_ref.as_deref()?;
    let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
    let scope = matching_scopes.next()?;
    if matching_scopes.next().is_some() {
        return None;
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    // A scope bound to a history reads that history; other scopes need a
    // state pair that only one history holds.
    let (history, state, previous) = if scope_histories.contains_key(&scope.id) {
        bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        )
    } else {
        unique_history_state_pair(histories, state_id, previous_state_id)
    }?;
    let spans_transitions = spanning
        || matches!(
            feature.evaluation.definition(),
            cadmpeg_ir::features::FeatureDefinition::Operation(
                cadmpeg_ir::features::FeatureOperation::MoveFace { .. }
                    | cadmpeg_ir::features::FeatureOperation::DeleteFace { heal: true, .. }
            )
        );
    if !spans_transitions && state.transition.as_ref()?.previous_state_id != Some(previous_state_id)
    {
        return None;
    }
    let (history_index, state_index) = history_state_indexes(histories, history, state)?;
    let (_, previous_index) = history_state_indexes(histories, history, previous)?;
    Some(FaceSelectionState {
        scope,
        previous_state_id,
        history: history_index,
        state: state_index,
        previous: previous_index,
    })
}
