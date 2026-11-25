// SPDX-License-Identifier: Apache-2.0
//! Part 10 validation of the CADIR v11 process layer (part 7).
//!
//! The rules here are decided by reference lookup, counting, and in-IR
//! arithmetic over the process records and the design records they refer
//! to. A process value bound to a parameter is checked through the
//! parameter's cached value only where a rule asks for its value.

mod bindings;
mod caches;
mod evaluations;
#[cfg(test)]
pub(crate) mod fixture;
mod operations;
mod order;
mod records;
mod references;
mod scope;
mod walk;

use crate::report::check::Ruled;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use super::design::records::DesignRecords;
use crate::document::CadIr;
use crate::index::ModelIndex;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use records::ProcessRecords;

/// Check the process layer of `ir` against part 10's process rules.
pub(super) fn check_process(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    if is_empty(ir) {
        return Ok(());
    }
    let design = DesignRecords::new(ir);
    let records = ProcessRecords::new(&design);
    let walked = walk::walk_records(&records);
    order::check_order(&records, findings);
    references::check_references(&records, findings);
    references::check_native_refs(&records, ids, findings);
    bindings::check_bindings(&records, &walked, findings);
    scope::check_scope(&records, &walked, findings);
    evaluations::check_evaluations(
        &records,
        ctx.policy().limits.max_state_chain_depth,
        findings,
    )?;
    evaluations::check_resolved_states(ctx, &records, ids, findings)?;
    operations::check_operations(&records, findings);
    caches::check_caches(&records, findings);
    Ok(())
}

fn is_empty(ir: &CadIr) -> bool {
    let process = &ir.process;
    process.setups.is_empty()
        && process.operations.is_empty()
        && process.features.is_empty()
        && process.groups.is_empty()
        && process.tools.is_empty()
        && process.presets.is_empty()
        && process.machines.is_empty()
        && process.programs.is_empty()
        && process.parameters.is_empty()
        && process.toolpaths.is_empty()
        && process.states.is_empty()
}

/// Runs a check of the design layer on a process record and reports its
/// findings under [`Check::Process`].
fn as_process<T>(findings: &mut Vec<Finding>, check: impl FnOnce(&mut Vec<Finding>) -> T) -> T {
    let mut design = Vec::new();
    let result = check(&mut design);
    findings.extend(design.into_iter().map(|finding| Finding {
        check: Check::Process,
        ..finding
    }));
    result
}

fn error(findings: &mut Vec<Finding>, entity: &str, message: Ruled<'_>) {
    super::record_finding(
        findings,
        Check::Process,
        Severity::Error,
        Some(entity),
        message,
    );
}

fn warning(findings: &mut Vec<Finding>, entity: &str, message: Ruled<'_>) {
    super::record_finding(
        findings,
        Check::Process,
        Severity::Warning,
        Some(entity),
        message,
    );
}
