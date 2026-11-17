// SPDX-License-Identifier: Apache-2.0
//! Part 10 validation of the CADIR v11 design layer.
//!
//! The rules here are decided by reference lookup, counting, and in-IR
//! arithmetic over the design records, the current geometry, and the product
//! definitions. A rule about an operation's operands or names does not fire
//! on an `Unresolved` or `Native` operation.

mod configurations;
mod containers;
mod evaluations;
mod index;
mod linkage;
mod states;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::index::ModelIndex;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use index::DesignIndex;

/// Check the design layer of `ir` against part 10's scope-and-linkage,
/// container, position, evaluation, current-geometry, and configuration
/// rules.
pub(super) fn check_design(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    ids: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let index = DesignIndex::new(ir);
    linkage::check_linkage(&index, ids, findings);
    containers::check_containers(&index, findings);
    evaluations::check_evaluations(&index, findings);
    evaluations::check_current_geometry(&index, findings);
    states::check_resolved_states(ctx, &index, findings)?;
    configurations::check_configurations(&index, findings);
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

#[cfg(test)]
mod tests;
