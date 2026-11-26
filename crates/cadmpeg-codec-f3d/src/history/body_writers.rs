// SPDX-License-Identifier: Apache-2.0
//! Name each body of a feature's input state by the feature that last writes it.

use std::collections::HashMap;

use cadmpeg_ir::features::evaluation::{FeatureInputTopology, HistoricalBodyWriter};
use cadmpeg_ir::features::Feature;

use super::generated_bodies::{
    historical_state_number, inserts_body, scope_history, scope_state_history, updates_body,
};
use crate::history_records::AsmHistory;
use crate::records::feature::scope::DesignParameterScope;

/// Fill `body_writers` of each input state. The writer of a body slot is the
/// feature of the one scope of the history whose transition last inserts or
/// updates the slot up to the input state, when that feature precedes the
/// consuming feature.
pub(crate) fn attach(
    features: &[Feature],
    (scopes, histories): (&[DesignParameterScope], &[AsmHistory]),
    scope_histories: &HashMap<String, String>,
    topologies: &mut [FeatureInputTopology],
) {
    let by_id = features
        .iter()
        .map(|feature| (feature.id.as_str(), feature))
        .collect::<HashMap<_, _>>();
    let by_scope = features
        .iter()
        .filter_map(|feature| Some((feature.native_ref.as_deref()?, feature)))
        .collect::<HashMap<_, _>>();
    for topology in topologies {
        let Some(feature) = by_id.get(topology.input_of.as_str()) else {
            continue;
        };
        let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|native| scopes.iter().find(|scope| scope.id == native))
        else {
            continue;
        };
        let Some(history) = scope_state_history(scope, scope_histories, histories) else {
            continue;
        };
        let Some(input_state) = historical_state_number(topology.id.as_str()) else {
            continue;
        };
        let writer_of_state = |state_id: i64| {
            let mut candidates = scopes.iter().filter(|candidate| {
                candidate.history_state_id() == Some(state_id)
                    && scope_history(candidate, scope_histories, histories)
                        .is_some_and(|candidate_history| std::ptr::eq(candidate_history, history))
            });
            let candidate = candidates.next()?;
            if candidates.next().is_some() {
                return None;
            }
            let writer = by_scope.get(candidate.id.as_str())?;
            (writer.ordinal < feature.ordinal).then(|| writer.id.clone())
        };
        let writers = topology
            .bodies
            .iter()
            .filter_map(|body| {
                let slot = super::body_selections::historical_body_slot(body.as_str())?;
                let state = last_write(history, input_state, slot)?;
                Some(HistoricalBodyWriter {
                    body: body.clone(),
                    writer: writer_of_state(state)?,
                })
            })
            .collect();
        topology.body_writers = writers;
    }
}

/// The id of the latest state up to and including `input_state` whose
/// transition inserts or updates `slot`.
fn last_write(history: &AsmHistory, input_state: i64, slot: i64) -> Option<i64> {
    let order = history.states_oldest_first()?;
    let states = history.states();
    let mut positions = order.iter().enumerate().filter(|(_, index)| {
        states
            .get(**index)
            .is_some_and(|state| state.state_id == input_state)
    });
    let (position, _) = positions.next()?;
    if positions.next().is_some() {
        return None;
    }
    order[..=position]
        .iter()
        .rev()
        .filter_map(|index| states.get(*index))
        .find(|state| inserts_body(state, slot) || updates_body(state, slot))
        .map(|state| state.state_id)
}

#[cfg(test)]
mod tests;
