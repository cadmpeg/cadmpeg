// SPDX-License-Identifier: Apache-2.0
//! Binding histories and oldest-first state visits.

use crate::history_records::{AsmDeltaState, AsmHistory};

/// History access for a pass that visits states oldest first.
pub(crate) enum BindingHistories<'a> {
    /// Keep only the visited state materialized between visits, advancing the
    /// previous visit's materialization to it. A visit reads the visited
    /// state or newer ones.
    Release(&'a mut [AsmHistory]),
    /// Leave materializations in place for a caller that holds the histories
    /// shared.
    Shared(&'a [AsmHistory]),
}

impl BindingHistories<'_> {
    pub(super) fn get(&self) -> &[AsmHistory] {
        match self {
            Self::Release(histories) => histories,
            Self::Shared(histories) => histories,
        }
    }

    pub(super) fn reborrow(&mut self) -> BindingHistories<'_> {
        match self {
            Self::Release(histories) => BindingHistories::Release(histories),
            Self::Shared(histories) => BindingHistories::Shared(histories),
        }
    }

    pub(super) fn advance(&mut self, history_index: usize, state_index: usize) {
        if let Self::Release(histories) = self {
            if let Some(history) = histories.get_mut(history_index) {
                history.advance_topology(state_index);
            }
        }
    }

    pub(super) fn retain(&mut self, history_index: usize, state_index: usize) {
        if let Self::Release(histories) = self {
            if let Some(history) = histories.get_mut(history_index) {
                history.retain_topology(state_index);
            }
        }
    }
}

/// Visit the jobs of each `(history index, state index)` group, oldest state
/// first within each history. Jobs keep their relative order within a group.
pub(super) fn visit_state_groups_oldest_first<J>(
    mut histories: BindingHistories<'_>,
    mut jobs: Vec<(usize, usize, J)>,
    mut visit: impl FnMut(&[AsmHistory], usize, usize, Vec<J>),
) {
    let orders = histories
        .get()
        .iter()
        .map(AsmHistory::states_oldest_first)
        .collect::<Vec<_>>();
    jobs.sort_by_cached_key(|&(history_index, state_index, _)| {
        let position = orders
            .get(history_index)
            .and_then(Option::as_ref)
            .and_then(|order| order.iter().position(|&index| index == state_index));
        (history_index, position.is_none(), position)
    });
    let mut jobs = jobs.into_iter().peekable();
    while let Some((history_index, state_index, job)) = jobs.next() {
        let mut group = vec![job];
        while let Some((_, _, job)) =
            jobs.next_if(|&(history, state, _)| (history, state) == (history_index, state_index))
        {
            group.push(job);
        }
        histories.advance(history_index, state_index);
        visit(histories.get(), history_index, state_index, group);
        histories.retain(history_index, state_index);
    }
}

/// Visit `(history index, state index, job)` entries oldest first within each
/// history and keep only the visited state materialized after each visit.
pub(super) fn visit_states_oldest_first<J>(
    histories: &mut [AsmHistory],
    jobs: Vec<(usize, usize, J)>,
    mut visit: impl FnMut(&AsmHistory, &AsmDeltaState, J),
) {
    visit_state_groups_oldest_first(
        BindingHistories::Release(histories),
        jobs,
        |histories, history_index, state_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(state) = history.states().get(state_index) else {
                return;
            };
            for job in group {
                visit(history, state, job);
            }
        },
    );
}

pub(super) fn history_state_indexes(
    histories: &[AsmHistory],
    history: &AsmHistory,
    state: &AsmDeltaState,
) -> Option<(usize, usize)> {
    let history_index = histories
        .iter()
        .position(|item| std::ptr::eq(item, history))?;
    let state_index = history
        .states()
        .iter()
        .position(|item| std::ptr::eq(item, state))?;
    Some((history_index, state_index))
}
