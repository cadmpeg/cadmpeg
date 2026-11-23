// SPDX-License-Identifier: Apache-2.0
//! Reading a document walks each chain of state bases under
//! `ResourceLimits::max_state_chain_depth` and refuses a cycle (§2.5).

use cadmpeg_core::decode::{ResourceDimension, ResourceFailure, ResourceLimits};
use cadmpeg_core::CodecError;

use super::fixtures::id;
use super::root_fixtures as root;
use crate::design::evaluation::{Evaluation, StateBase, StateContent, StateDelta};
use crate::design::read::{self, Ceiling};
use crate::document::CadIr;

/// The desktop and service limits, with distinct state chain ceilings low
/// enough to build chains past them cheaply.
fn policies() -> [ResourceLimits; 2] {
    let mut desktop = ResourceLimits::desktop();
    desktop.max_state_chain_depth = 8;
    let mut service = ResourceLimits::service();
    service.max_state_chain_depth = 5;
    [desktop, service]
}

fn key(at: u64) -> u32 {
    u32::try_from(at).unwrap()
}

/// Evaluation `at` without content.
fn without_content(at: u64) -> Evaluation {
    let mut evaluation = root::evaluations().remove(0);
    evaluation.id = id("evaluation", key(at));
    evaluation.content = None;
    evaluation
}

/// Evaluation `at` with content on `base`.
fn evaluation(at: u64, base: StateBase) -> Evaluation {
    let mut evaluation = without_content(at);
    evaluation.content = Some(StateContent {
        base,
        delta: StateDelta::default(),
    });
    evaluation
}

fn on(at: u64) -> StateBase {
    StateBase::Evaluation(id("evaluation", key(at)))
}

/// A chain of `depth` evaluations: evaluation 1 on `Empty`, each next on the
/// one before.
fn chain(depth: u64) -> Vec<Evaluation> {
    (1..=depth)
        .map(|at| {
            let base = if at == 1 {
                StateBase::Empty
            } else {
                on(at - 1)
            };
            evaluation(at, base)
        })
        .collect()
}

fn document(evaluations: Vec<Evaluation>) -> String {
    let mut ir = CadIr::empty();
    ir.design.evaluations = evaluations;
    ir.finalize();
    serde_json::to_string(&ir).unwrap()
}

fn admitted(evaluations: Vec<Evaluation>, limits: &ResourceLimits) {
    let text = document(evaluations);
    let back = read::from_str::<CadIr>(&text, limits)
        .unwrap_or_else(|error| panic!("a chain within the ceiling is refused: {error}"));
    assert_eq!(serde_json::to_string(&back).unwrap(), text);
}

fn refused_past(evaluations: Vec<Evaluation>, limits: &ResourceLimits) {
    let limit = Ceiling::StateChain.of(limits);
    match read::from_str::<CadIr>(&document(evaluations), limits) {
        Err(CodecError::ResourceLimit(refusal)) => {
            assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
            assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
            assert_eq!(refusal.limit, limit);
            assert_eq!(refusal.used, limit);
            assert_eq!(refusal.additional, 1);
            assert_eq!(refusal.operation, "CADIR state base chain depth");
        }
        other => panic!("a chain one past the ceiling of {limit} gave {other:?}"),
    }
}

fn refused_cycle(evaluations: Vec<Evaluation>, limits: &ResourceLimits) {
    match read::from_str::<CadIr>(&document(evaluations), limits) {
        Err(CodecError::Malformed(error)) => {
            assert!(error.contains("its own ancestor"), "{error}");
        }
        other => panic!("a cycle of state bases gave {other:?}"),
    }
}

#[test]
fn a_chain_at_the_ceiling_reads_and_one_past_is_refused() {
    for limits in policies().into_iter().chain([ResourceLimits::desktop()]) {
        let limit = Ceiling::StateChain.of(&limits);
        admitted(chain(limit), &limits);
        refused_past(chain(limit + 1), &limits);
    }
}

/// The walk does not depend on arena order: from the tip down, with a fork
/// whose arms share the chain below them, and through a base without content
/// or a base the arena does not hold, both of which end a chain.
#[test]
fn the_depth_is_the_longest_chain_in_any_arena_order() {
    for limits in policies() {
        let limit = Ceiling::StateChain.of(&limits);
        let mut tip_first = chain(limit - 1);
        tip_first.push(evaluation(limit, on(limit - 1)));
        tip_first.push(evaluation(limit + 1, on(limit - 1)));
        tip_first.reverse();
        admitted(tip_first.clone(), &limits);
        tip_first.insert(0, evaluation(limit + 2, on(limit)));
        refused_past(tip_first, &limits);

        let mut ends = chain(limit);
        ends[0] = evaluation(1, on(limit + 1));
        ends.push(without_content(limit + 1));
        admitted(ends.clone(), &limits);
        ends[0] = evaluation(1, on(limit + 7));
        admitted(ends, &limits);
    }
}

#[test]
fn a_cycle_of_state_bases_is_refused() {
    for limits in policies() {
        refused_cycle(vec![evaluation(1, on(1))], &limits);
        refused_cycle(vec![evaluation(1, on(2)), evaluation(2, on(1))], &limits);
        let mut tail_into_cycle = chain(3);
        tail_into_cycle[0] = evaluation(1, on(3));
        tail_into_cycle.insert(0, evaluation(4, on(3)));
        refused_cycle(tail_into_cycle, &limits);
    }
}

/// A chain far past the ceiling is refused one past it, from either end.
#[test]
fn a_chain_far_past_the_ceiling_is_refused_one_past_it() {
    let [_, limits] = policies();
    let long = chain(2 * Ceiling::StateChain.of(&limits));
    refused_past(long.clone(), &limits);
    let mut tip_first = long;
    tip_first.reverse();
    refused_past(tip_first, &limits);
}

/// `CadIr::from_json` walks the chains as every other read does.
#[test]
fn from_json_walks_the_chains_as_reading_does() {
    assert!(CadIr::from_json(&document(chain(3))).is_ok());
    assert!(CadIr::from_json(&document(vec![evaluation(1, on(1))])).is_err());
}
