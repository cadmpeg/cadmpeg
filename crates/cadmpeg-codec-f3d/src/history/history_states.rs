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

/// The history state a scope produces and the state it follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScopeStates {
    pub(crate) state_id: i64,
    pub(crate) previous: PreviousState,
}

/// The state that a scope's history state follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviousState {
    /// The stored previous state, else the one state to which the histories
    /// link the history state.
    Linked(i64),
    /// No history holds the history state.
    Unheld,
    /// The histories that hold the history state link it to no state.
    Unnamed,
    /// The histories link the history state to more than one state.
    Conflicting(StateConflict),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StateConflict {
    /// Two histories link the state to these two states.
    Histories(i64, i64),
    /// One history links the state to the first state by its transition and
    /// to the second by its header.
    TransitionAndHeader(i64, i64),
    /// One history holds the state twice.
    HeldTwice,
}

/// What one history that holds a state links it to.
enum HistoryLink {
    Names(i64),
    Abstains,
    Contradicts(StateConflict),
}

fn history_link(history: &AsmHistory, state_id: i64) -> Option<HistoryLink> {
    let mut states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some() {
        return Some(HistoryLink::Contradicts(StateConflict::HeldTwice));
    }
    let transition = state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id);
    Some(
        match (transition, header_previous_state_id(history, state)) {
            (Some(transition), Some(header)) if transition != header => {
                HistoryLink::Contradicts(StateConflict::TransitionAndHeader(transition, header))
            }
            (Some(previous_state_id), _) | (None, Some(previous_state_id)) => {
                HistoryLink::Names(previous_state_id)
            }
            (None, None) => HistoryLink::Abstains,
        },
    )
}

/// The state to which `histories` link `state_id`. A history that links it
/// to no state abstains.
fn linked_previous_state<'h>(
    histories: impl Iterator<Item = &'h AsmHistory>,
    state_id: i64,
) -> PreviousState {
    histories
        .filter_map(|history| history_link(history, state_id))
        .fold(PreviousState::Unheld, |previous, link| {
            match (previous, link) {
                (PreviousState::Conflicting(_), _) => previous,
                (_, HistoryLink::Contradicts(conflict)) => PreviousState::Conflicting(conflict),
                (PreviousState::Linked(first), HistoryLink::Names(second)) if first != second => {
                    PreviousState::Conflicting(StateConflict::Histories(first, second))
                }
                (PreviousState::Linked(_), HistoryLink::Names(_) | HistoryLink::Abstains) => {
                    previous
                }
                (PreviousState::Unheld | PreviousState::Unnamed, HistoryLink::Names(state)) => {
                    PreviousState::Linked(state)
                }
                (PreviousState::Unheld | PreviousState::Unnamed, HistoryLink::Abstains) => {
                    PreviousState::Unnamed
                }
            }
        })
}

/// Read the scope's states. Its previous state is the stored one, else the
/// state to which the histories with the scope's bound history id link its
/// state, else the state to which every history that holds its state links
/// it.
pub(crate) fn scope_states(
    scope: &crate::records::feature::scope::DesignParameterScope,
    scope_histories: &HashMap<String, String>,
    histories: &[AsmHistory],
) -> Option<ScopeStates> {
    let state_id = scope.history_state_id()?;
    let previous = match scope.previous_history_state_id() {
        Some(previous_state_id) => PreviousState::Linked(previous_state_id),
        None => {
            let bound = scope_histories.get(&scope.id);
            match linked_previous_state(
                histories
                    .iter()
                    .filter(|history| Some(&history.id) == bound),
                state_id,
            ) {
                linked @ (PreviousState::Linked(_) | PreviousState::Conflicting(_)) => linked,
                PreviousState::Unheld | PreviousState::Unnamed => {
                    linked_previous_state(histories.iter(), state_id)
                }
            }
        }
    };
    Some(ScopeStates { state_id, previous })
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
