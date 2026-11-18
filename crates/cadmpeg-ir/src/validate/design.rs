// SPDX-License-Identifier: Apache-2.0
//! Part 10 validation of the CADIR v11 design layer.
//!
//! The rules here are decided by reference lookup, counting, and in-IR
//! arithmetic over the design records, the current geometry, and the product
//! definitions. A rule about an operation's operands or names does not fire
//! on an `Unresolved` or `Native` operation.

mod caches;
mod configurations;
mod containers;
mod dimensions;
mod evaluations;
mod expressions;
#[cfg(test)]
mod fixture;
mod linkage;
mod parameter_records;
#[cfg(test)]
mod parameter_tests;
mod parameters;
mod programs;
mod records;
mod resolved;
mod tolerances;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::index::ModelIndex;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use records::DesignRecords;

/// Check the design layer of `ir` against part 10's scope-and-linkage,
/// container, position, evaluation, current-geometry, configuration,
/// parameter, expression, program, dimension, and cache rules.
pub(super) fn check_design(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let records = DesignRecords::new(ir);
    linkage::check_linkage(&records, ids, findings);
    containers::check_containers(&records, findings);
    evaluations::check_evaluations(
        &records,
        ctx.policy().limits.max_state_chain_depth,
        findings,
    );
    evaluations::check_current_geometry(&records, findings);
    resolved::check_resolved_states(ctx, &records, ids, findings)?;
    configurations::check_configurations(&records, findings);
    parameter_records::check_parameter_records(&records, findings);
    programs::check_programs(&records, findings);
    expressions::check_expressions(&records, findings);
    dimensions::check_dimensions(&records, findings);
    tolerances::check_tolerances(&records, findings);
    caches::check_caches(&records, findings);
    Ok(())
}

fn error(findings: &mut Vec<Finding>, entity: &str, message: std::fmt::Arguments<'_>) {
    super::record_finding(
        findings,
        Check::Design,
        Severity::Error,
        Some(entity),
        message,
    );
}

fn warning(findings: &mut Vec<Finding>, entity: &str, message: std::fmt::Arguments<'_>) {
    super::record_finding(
        findings,
        Check::Design,
        Severity::Warning,
        Some(entity),
        message,
    );
}
