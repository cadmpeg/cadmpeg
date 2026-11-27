// SPDX-License-Identifier: Apache-2.0
//! History state lookup, state pairs, and state reachability.

use crate::history_records::{AsmDeltaState, AsmHistory};
use std::collections::HashMap;

pub(super) fn unique_history_state(
    histories: &[AsmHistory],
    state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState)> {
    let mut matches = histories.iter().filter_map(|history| {
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == state_id);
        let state = states.next()?;
        states.next().is_none().then_some((history, state))
    });
    let state = matches.next()?;
    matches.next().is_none().then_some(state)
}

pub(super) fn unique_history_state_in(history: &AsmHistory, state_id: i64) -> bool {
    let mut states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    states.next().is_some() && states.next().is_none()
}

/// Read the scope's predecessor, else the bound transition predecessor, else
/// the raw `next` link. Reject disagreement between the transition and link.
pub(crate) fn effective_scope_previous_history_state_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Option<i64> {
    scope.previous_history_state_id().or_else(|| {
        let state_id = scope.history_state_id()?;
        let (history, state) = unique_history_state(histories, state_id)?;
        transition_or_header_previous_state_id(history, state)
    })
}

/// The history state that a scope's state follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopePrevious {
    /// The scope states no history state.
    Stateless,
    /// No history the scope reads holds its state.
    Unheld,
    /// The stored previous state, else the one state to which every history
    /// the scope reads links its state.
    Known(i64),
    /// The histories the scope reads hold its state and link it to no single
    /// previous state.
    Unlinked(i64),
}

/// Read the scope's previous state: the stored one, else the link of its
/// state in the scope's bound history, else the predecessor on which every
/// history that holds its state agrees.
pub(crate) fn scope_previous_history_state(
    scope: &crate::records::feature::scope::DesignParameterScope,
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
) -> ScopePrevious {
    let Some(state_id) = scope.history_state_id() else {
        return ScopePrevious::Stateless;
    };
    if let Some(previous_state_id) = scope.previous_history_state_id() {
        return ScopePrevious::Known(previous_state_id);
    }
    let read = bound_scope_history(&scope.id, scope_histories, histories)
        .map_or(histories, std::slice::from_ref);
    let mut links = read.iter().filter_map(|history| {
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == state_id);
        let state = states.next()?;
        Some(
            states
                .next()
                .is_none()
                .then(|| transition_or_header_previous_state_id(history, state))
                .flatten(),
        )
    });
    match links.next() {
        None => ScopePrevious::Unheld,
        Some(Some(previous_state_id)) if links.all(|link| link == Some(previous_state_id)) => {
            ScopePrevious::Known(previous_state_id)
        }
        Some(_) => ScopePrevious::Unlinked(state_id),
    }
}

/// Read the scope's predecessor, else its history header predecessor.
/// History reads use only `state_id`, `node_index`, and `next_ref`, so the
/// result does not depend on history binding.
pub(crate) fn header_scope_previous_history_state_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Option<i64> {
    scope.previous_history_state_id().or_else(|| {
        let (history, state) = unique_history_state(histories, scope.history_state_id()?)?;
        header_previous_state_id(history, state)
    })
}

pub(crate) fn unique_history_state_pair(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    unique_state_pair_by(
        histories,
        state_id,
        previous_state_id,
        transition_or_header_previous_state_id,
    )
}

/// Resolve one state pair through history header links.
/// Read only `state_id`, `node_index`, and `next_ref`, so the result does not
/// depend on history binding.
pub(crate) fn unique_header_history_state_pair(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    unique_state_pair_by(
        histories,
        state_id,
        previous_state_id,
        header_previous_state_id,
    )
}

pub(super) fn unique_state_pair_by(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    predecessor: fn(&AsmHistory, &AsmDeltaState) -> Option<i64>,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut direct = histories.iter().filter_map(|history| {
        state_pair_by(history, state_id, previous_state_id, true, predecessor)
    });
    if let Some(pair) = direct.next() {
        return direct.next().is_none().then_some(pair);
    }
    let mut matches = histories.iter().filter_map(|history| {
        state_pair_by(history, state_id, previous_state_id, false, predecessor)
    });
    let pair = matches.next()?;
    matches.next().is_none().then_some(pair)
}

pub(super) fn history_state_pair(
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    state_pair_by(
        history,
        state_id,
        previous_state_id,
        require_direct,
        transition_or_header_previous_state_id,
    )
}

pub(super) fn state_pair_by(
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
    predecessor: fn(&AsmHistory, &AsmDeltaState) -> Option<i64>,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some()
        || (require_direct && predecessor(history, state) != Some(previous_state_id))
    {
        return None;
    }
    let mut previous_states = history
        .states()
        .iter()
        .filter(|state| state.state_id == previous_state_id);
    let previous = previous_states.next()?;
    if previous_states.next().is_some()
        || (!require_direct && !state_reaches_by(history, state, previous_state_id, predecessor))
    {
        return None;
    }
    Some((history, state, previous))
}

/// Return the one ASM history bound to a Design scope.
pub(crate) fn bound_scope_history<'a>(
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<&'a AsmHistory> {
    let history_id = scope_histories.get(scope_id)?;
    let mut matches = histories.iter().filter(|history| &history.id == history_id);
    let history = matches.next()?;
    matches.next().is_none().then_some(history)
}

pub(super) fn bound_history_state_pair<'a>(
    scope_id: &str,
    state_id: i64,
    previous_state_id: i64,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    history_state_pair(
        bound_scope_history(scope_id, scope_histories, histories)?,
        state_id,
        previous_state_id,
        false,
    )
}

pub(super) fn insert_scope_history_binding(
    resolved: &mut HashMap<String, String>,
    scope_id: &str,
    history_id: &str,
) {
    let scope = scope_id.to_owned();
    let history = history_id.to_owned();
    resolved.insert(scope, history);
}

pub(super) fn history_state_reaches(
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> bool {
    state_reaches_by(
        history,
        state,
        previous_state_id,
        transition_or_header_previous_state_id,
    )
}

pub(super) fn state_reaches_by(
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
    predecessor: fn(&AsmHistory, &AsmDeltaState) -> Option<i64>,
) -> bool {
    let mut current = state;
    for _ in 0..=history.states().len() {
        if current.state_id == previous_state_id {
            return true;
        }
        let Some(previous_state_id) = predecessor(history, current) else {
            return false;
        };
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == previous_state_id);
        let Some(previous) = states.next() else {
            return false;
        };
        if states.next().is_some() {
            return false;
        }
        current = previous;
    }
    false
}

/// Read the unique predecessor through the history header's `next` link.
/// Read only `state_id`, `node_index`, and `next_ref`, so the result does not
/// depend on history binding.
pub(super) fn header_previous_state_id(history: &AsmHistory, state: &AsmDeltaState) -> Option<i64> {
    state.next_ref.and_then(|node_index| {
        let mut states = history
            .states()
            .iter()
            .filter(|candidate| candidate.node_index == node_index);
        let previous = states.next()?;
        states.next().is_none().then_some(previous.state_id)
    })
}

/// Read the bound transition predecessor, else the raw `next` link.
/// Reject disagreement between the transition predecessor and the header link.
pub(super) fn transition_or_header_previous_state_id(
    history: &AsmHistory,
    state: &AsmDeltaState,
) -> Option<i64> {
    let linked = header_previous_state_id(history, state);
    let derived = state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id);
    match (derived, linked) {
        (Some(derived), Some(linked)) if derived != linked => None,
        (Some(derived), _) => Some(derived),
        (None, Some(linked)) => Some(linked),
        (None, None) => None,
    }
}

pub(super) fn history_state_index(history: &AsmHistory) -> HashMap<i64, Option<&AsmDeltaState>> {
    let mut states = HashMap::new();
    for state in history.states() {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    states
}
