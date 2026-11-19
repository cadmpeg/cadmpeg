// SPDX-License-Identifier: Apache-2.0
//! Identity and reference checks for source manufacturing definitions.
use crate::index::identities::IdentitySet;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::CadIr;
use std::collections::BTreeSet;

pub(super) fn check_manufacturing(ir: &CadIr, ids: &dyn IdentitySet, findings: &mut Vec<Finding>) {
    let manufacturing = &ir.manufacturing;
    let mut seen = BTreeSet::new();
    for id in manufacturing
        .setups
        .iter()
        .map(|record| record.id.as_str())
        .chain(
            manufacturing
                .operations
                .iter()
                .map(|record| record.id.as_str()),
        )
        .chain(manufacturing.tools.iter().map(|record| record.id.as_str()))
    {
        if !seen.insert(id) || ids.contains(id) {
            super::record_finding(
                findings,
                Check::Identity,
                Severity::Error,
                Some(id),
                format_args!("duplicate manufacturing identity {id}"),
            );
        }
    }
    let setups: BTreeSet<_> = manufacturing
        .setups
        .iter()
        .map(|record| &record.id)
        .collect();
    let tools: BTreeSet<_> = manufacturing
        .tools
        .iter()
        .map(|record| &record.id)
        .collect();
    let mut orders = BTreeSet::new();
    for operation in &manufacturing.operations {
        for reference in
            std::iter::once((setups.contains(&operation.setup), operation.setup.as_str())).chain(
                operation
                    .tool
                    .as_ref()
                    .map(|tool| (tools.contains(tool), tool.as_str())),
            )
        {
            if !reference.0 {
                super::record_finding(
                    findings,
                    Check::ReferentialIntegrity,
                    Severity::Error,
                    Some(operation.id.as_str()),
                    format_args!("unresolved manufacturing reference {}", reference.1),
                );
            }
        }
        if !orders.insert((&operation.setup, operation.order)) {
            super::record_finding(
                findings,
                Check::Identity,
                Severity::Error,
                Some(operation.id.as_str()),
                format_args!(
                    "duplicate manufacturing operation order {}",
                    operation.order
                ),
            );
        }
    }
}
