// SPDX-License-Identifier: Apache-2.0
//! Setups, order, and programs (§7.1, §7.2, §7.3): every operation's setup,
//! modifier inputs, program steps, stock taken from a previous setup, and
//! the nesting a reader admits.

use std::collections::{BTreeMap, BTreeSet};

use super::records::{program_targets, ProcessRecords, ProgramTarget};
use super::walk::Found;
use crate::design::ids::OperationId;
use crate::process::operation::OperationKind;
use crate::process::stock::StockRule;
use crate::report::check::Finding;
use crate::validate::cycles::on_cycles;

/// The JSON nesting `serde_json` reads, which bounds every recursive process
/// type the specification gives no ceiling: step folders, program groups,
/// and motions nested through a cycle's expansion.
pub(super) const READER_JSON_DEPTH: usize = 127;

/// The levels above a record of a process arena in a document: the root
/// object, the `process` object, and the arena's array.
pub(super) const RECORD_PREFIX_DEPTH: usize = 3;

pub(super) fn check_order(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    check_membership(records, findings);
    check_inputs(records, findings);
    check_programs(records, findings);
    check_previous_setups(records, findings);
}

/// Every operation is in exactly one setup: one listing in that setup's
/// steps, or an input of a modifier in that setup (§7.1).
fn check_membership(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    for (&operation, listings) in &records.listings {
        if !records.operations.contains_key(operation) {
            for setup in listings {
                super::error(
                    findings,
                    setup,
                    crate::rule!(
                        "process.setup-step-names-no-operation",
                        "setup step names {operation}, which is no operation"
                    ),
                );
            }
        }
        if listings.len() > 1 {
            super::error(
                findings,
                operation,
                crate::rule!(
                    "process.operation-listed-times-setup-steps-one",
                    "operation is listed {} times in setup steps; it is one step of one setup",
                    listings.len()
                ),
            );
        }
    }
    for operation in &records.ir.process.operations {
        let id = operation.id.as_str();
        match records.memberships.get(id).map(BTreeSet::len) {
            None | Some(0) => super::error(
                findings,
                id,
                crate::rule!("process.operation-no-setup-no-setup-steps", 
                    "operation is in no setup: no setup's steps list it and no modifier in a setup takes it"
                ),
            ),
            Some(1) => {}
            Some(_) => {
                let setups: Vec<&str> = records.memberships[id].iter().copied().collect();
                super::error(
                    findings,
                    id,
                    crate::rule!("process.operation-setups-exactly-one", 
                        "operation is in setups {}; it is in exactly one",
                        setups.join(", ")
                    ),
                );
            }
        }
    }
}

/// Modifier inputs are nonempty, name operations, and form no cycle (§7.3).
fn check_inputs(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for operation in &records.ir.process.operations {
        let OperationKind::Modifier { inputs, .. } = &operation.kind else {
            continue;
        };
        let id = operation.id.as_str();
        if inputs.is_empty() {
            super::error(
                findings,
                id,
                crate::rule!("process.modifier-takes-no-input", "modifier takes no input"),
            );
        }
        for input in inputs {
            if !records.operations.contains_key(input.as_str()) {
                super::error(
                    findings,
                    id,
                    crate::rule!(
                        "process.modifier-input-no-operation",
                        "modifier input {} is no operation",
                        input.as_str()
                    ),
                );
            }
        }
        graph.insert(id, inputs.iter().map(OperationId::as_str).collect());
    }
    for id in on_cycles(&graph) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.operation-own-input-through-modifier-inputs",
                "operation is its own input through modifier inputs"
            ),
        );
    }
}

/// Every program step names an existing setup or operation (§7.1).
fn check_programs(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    for program in &records.ir.process.programs {
        for target in program_targets(&program.steps) {
            let (exists, what, id) = match target {
                ProgramTarget::Setup(setup) => (
                    records.setups.contains_key(setup.as_str()),
                    "setup",
                    setup.as_str(),
                ),
                ProgramTarget::Operation(operation) => (
                    records.operations.contains_key(operation),
                    "operation",
                    operation,
                ),
            };
            if !exists {
                super::error(
                    findings,
                    program.id.as_str(),
                    crate::rule!(
                        "process.program-step-names-no",
                        "program step names {id}, which is no {what}"
                    ),
                );
            }
        }
    }
}

/// `PreviousSetup` names an existing setup other than its own, and those
/// references form no cycle (§7.2).
fn check_previous_setups(records: &ProcessRecords<'_>, findings: &mut Vec<Finding>) {
    let mut graph: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for setup in &records.ir.process.setups {
        let id = setup.id.as_str();
        let Some(StockRule::PreviousSetup(previous)) =
            setup.stock.as_ref().and_then(|stock| stock.value.as_ref())
        else {
            continue;
        };
        let previous = previous.as_str();
        if previous == id {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.setup-takes-stock-itself",
                    "setup takes its stock from itself"
                ),
            );
            continue;
        }
        if !records.setups.contains_key(previous) {
            super::error(
                findings,
                id,
                crate::rule!(
                    "process.setup-takes-stock-no-setup",
                    "setup takes its stock from {previous}, which is no setup"
                ),
            );
        }
        graph.insert(id, BTreeSet::from([previous]));
    }
    for id in on_cycles(&graph) {
        super::error(
            findings,
            id,
            crate::rule!(
                "process.setup-takes-stock-itself-through-previous",
                "setup takes its stock from itself through previous setups"
            ),
        );
    }
}

/// A record nests no deeper than the reader admits, so a validated document
/// reads back.
pub(super) fn check_nesting(arena: &str, id: &str, found: &Found, findings: &mut Vec<Finding>) {
    let depth = RECORD_PREFIX_DEPTH + found.depth;
    if depth > READER_JSON_DEPTH {
        super::error(
            findings,
            id,
            crate::rule!("process.record-nests-json-levels-deep-document", 
                "{arena} record nests {depth} JSON levels deep in the document, past the {READER_JSON_DEPTH} a reader admits"
            ),
        );
    }
}

#[cfg(test)]
mod tests;
