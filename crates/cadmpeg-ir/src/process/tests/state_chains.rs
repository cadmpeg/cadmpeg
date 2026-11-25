// SPDX-License-Identifier: Apache-2.0
//! Reading a document walks each chain of process state bases, through the
//! evaluations it rests on, under `ResourceLimits::max_state_chain_depth`, and
//! refuses a cycle (§2.5, §7.9).

use cadmpeg_core::decode::{ResourceDimension, ResourceFailure, ResourceLimits};
use cadmpeg_core::CodecError;

use super::fixtures::{id, setup_id};
use super::root_fixtures::state_id;
use crate::design::evaluation::{
    Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::read;
use crate::design::refs::{Evaluated, Member, Position};
use crate::document::CadIr;
use crate::process::state::{ProcessBase, ProcessContent, ProcessState, ProcessStateOf};

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

/// Process state `at` with content on `base`.
fn state(at: u64, base: ProcessBase) -> ProcessState {
    ProcessState {
        id: state_id(key(at)),
        of: ProcessStateOf::Stock(setup_id(1)),
        configuration: None,
        status: EvaluationStatus::Current,
        content: Some(ProcessContent {
            base,
            delta: StateDelta::default(),
        }),
        feature: None,
    }
}

/// Process state `at` without content.
fn unstored(at: u64) -> ProcessState {
    ProcessState {
        content: None,
        ..state(at, ProcessBase::Design(StateBase::Empty))
    }
}

fn on(at: u64) -> ProcessBase {
    ProcessBase::State(state_id(key(at)))
}

fn on_evaluation(at: u64) -> ProcessBase {
    ProcessBase::Design(StateBase::Evaluation(id("evaluation", key(at))))
}

/// A chain of `depth` process states numbered from `first`: the first on
/// `base`, each next on the one before.
fn chain_from(first: u64, depth: u64, base: ProcessBase) -> Vec<ProcessState> {
    let mut states = vec![state(first, base)];
    states.extend((first + 1..first + depth).map(|at| state(at, on(at - 1))));
    states
}

fn chain(depth: u64) -> Vec<ProcessState> {
    chain_from(1, depth, ProcessBase::Design(StateBase::Empty))
}

/// A chain of `depth` evaluations: evaluation 1 on `Empty`, each next on the
/// one before.
fn evaluations(depth: u64) -> Vec<Evaluation> {
    (1..=depth)
        .map(|at| Evaluation {
            id: id("evaluation", key(at)),
            scope: id("product-definition", 1),
            configuration: None,
            of: Evaluated::State(Position::After(Member::Feature(id("feature", key(at))))),
            status: EvaluationStatus::Current,
            content: Some(StateContent {
                base: if at == 1 {
                    StateBase::Empty
                } else {
                    StateBase::Evaluation(id("evaluation", key(at - 1)))
                },
                delta: StateDelta::default(),
            }),
            native_ref: None,
        })
        .collect()
}

fn document(evaluations: Vec<Evaluation>, states: Vec<ProcessState>) -> String {
    let mut ir = CadIr::empty();
    ir.design.evaluations = evaluations;
    ir.process.states = states;
    ir.finalize();
    serde_json::to_string(&ir).unwrap()
}

fn admitted(text: &str, limits: &ResourceLimits) {
    let back = read::from_str::<CadIr>(text, limits)
        .unwrap_or_else(|error| panic!("a chain within the ceiling is refused: {error}"));
    assert_eq!(serde_json::to_string(&back).unwrap(), text);
}

fn refused_past(text: &str, limits: &ResourceLimits) {
    let limit = limits.max_state_chain_depth;
    match read::from_str::<CadIr>(text, limits) {
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

fn refused_cycle(states: Vec<ProcessState>, limits: &ResourceLimits) {
    match read::from_str::<CadIr>(&document(Vec::new(), states), limits) {
        Err(CodecError::Malformed(error)) => {
            assert!(error.contains("process state"), "{error}");
            assert!(error.contains("its own ancestor"), "{error}");
        }
        other => panic!("a cycle of process state bases gave {other:?}"),
    }
}

#[test]
fn a_process_chain_at_the_ceiling_reads_and_one_past_is_refused() {
    for limits in policies() {
        let limit = limits.max_state_chain_depth;
        admitted(&document(Vec::new(), chain(limit)), &limits);
        refused_past(&document(Vec::new(), chain(limit + 1)), &limits);
        let mut tip_first = chain(limit + 1);
        tip_first.reverse();
        refused_past(&document(Vec::new(), tip_first), &limits);
    }
}

/// A chain continues through the evaluation a design base names, and the
/// evaluations below it count towards the same ceiling.
#[test]
fn a_process_chain_counts_the_evaluations_below_it() {
    for limits in policies() {
        let limit = limits.max_state_chain_depth;
        let below = limit / 2;
        let above = |depth| chain_from(1, depth, on_evaluation(below));
        admitted(&document(evaluations(below), above(limit - below)), &limits);
        refused_past(
            &document(evaluations(below), above(limit - below + 1)),
            &limits,
        );
        let mut tip_first = above(limit - below + 1);
        tip_first.reverse();
        refused_past(&document(evaluations(below), tip_first), &limits);
    }
}

/// A base that names an evaluation without content, an evaluation the
/// document does not hold, a process state without content, or a process
/// state the document does not hold ends the chain.
#[test]
fn a_base_without_stored_content_ends_the_chain() {
    for limits in policies() {
        let limit = limits.max_state_chain_depth;
        let mut unstored_evaluation = evaluations(1);
        unstored_evaluation[0].content = None;
        admitted(
            &document(unstored_evaluation, chain_from(1, limit, on_evaluation(1))),
            &limits,
        );
        admitted(
            &document(Vec::new(), chain_from(1, limit, on_evaluation(7))),
            &limits,
        );
        let mut ends = chain_from(1, limit, on(limit + 1));
        ends.push(unstored(limit + 1));
        admitted(&document(Vec::new(), ends), &limits);
        admitted(
            &document(Vec::new(), chain_from(1, limit, on(limit + 7))),
            &limits,
        );
    }
}

#[test]
fn a_cycle_of_process_state_bases_is_refused() {
    for limits in policies() {
        refused_cycle(vec![state(1, on(1))], &limits);
        refused_cycle(vec![state(1, on(2)), state(2, on(1))], &limits);
        let mut tail_into_cycle = chain(3);
        tail_into_cycle[0] = state(1, on(3));
        tail_into_cycle.insert(0, state(4, on(3)));
        refused_cycle(tail_into_cycle, &limits);
    }
}

/// `CadIr::from_json` walks the chains under the default ceilings, as the
/// serde reader does.
#[test]
fn from_json_walks_the_chains() {
    let limit = ResourceLimits::default().max_state_chain_depth;
    assert!(CadIr::from_json(&document(Vec::new(), chain(limit))).is_ok());
    let error = CadIr::from_json(&document(Vec::new(), chain(limit + 1))).unwrap_err();
    assert!(
        error.to_string().contains("state base chain depth"),
        "{error}"
    );
    let error = CadIr::from_json(&document(Vec::new(), vec![state(1, on(1))])).unwrap_err();
    assert!(error.to_string().contains("its own ancestor"), "{error}");
    let error = CadIr::from_json(&document(
        Vec::new(),
        vec![state(1, on(2)), state(2, on(1))],
    ))
    .unwrap_err();
    assert!(error.to_string().contains("its own ancestor"), "{error}");
    let mut evaluation = evaluations(1);
    if let Some(content) = &mut evaluation[0].content {
        content.base = StateBase::Evaluation(id("evaluation", 1));
    }
    let error = CadIr::from_json(&document(evaluation, Vec::new())).unwrap_err();
    assert!(error.to_string().contains("its own ancestor"), "{error}");
}
