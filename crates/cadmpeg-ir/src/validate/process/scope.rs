// SPDX-License-Identifier: Apache-2.0
//! Scope (part 7): every setup machines a design scope; a record no setup
//! holds resolves from the one scope of the setups that use it; every
//! workpiece index, selection anchor, and occurrence path resolves from its
//! setup's scope.

use std::collections::BTreeSet;

use super::records::{OwnerKey, ProcessRecords};
use super::walk::Walked;
use crate::design::dimension::{ParamRef, ParameterRef};
use crate::design::selection::{Anchor, OccurrencePath, PickContext};
use crate::report::check::Finding;
use crate::validate::design::evaluations::{check_position, check_resulting_position};
use crate::validate::design::records::{MemberKey, PositionKey};
use crate::validate::design::selections;

pub(super) fn check_scope(
    records: &ProcessRecords<'_>,
    walked: &[Walked<'_>],
    findings: &mut Vec<Finding>,
) {
    for setup in &records.ir.process.setups {
        let scope = setup.scope.as_str();
        if !records.design.scopes.contains_key(scope) {
            super::error(
                findings,
                setup.id.as_str(),
                format_args!("setup's scope {scope} names no definition with a design scope"),
            );
        }
    }
    let moved = selections::moved_occurrences(records.ir);
    for record in walked {
        check_record(records, record, &moved, findings);
    }
}

fn check_record(
    records: &ProcessRecords<'_>,
    record: &Walked<'_>,
    moved: &BTreeSet<&str>,
    findings: &mut Vec<Finding>,
) {
    let id = record.id;
    let setups = records.setups_of(record.owner);
    let shared = !matches!(record.owner, OwnerKey::Setup(_) | OwnerKey::Operation(_));
    if shared && record.arena != "parameters" {
        let scopes: BTreeSet<&str> = setups
            .iter()
            .filter_map(|setup| records.scope_of_setup(setup))
            .collect();
        if scopes.len() > 1 {
            super::error(
                findings,
                id,
                format_args!(
                    "the setups that use the record machine several scopes: {}",
                    scopes.into_iter().collect::<Vec<_>>().join(", ")
                ),
            );
        }
    }
    let through: Vec<&OccurrencePath> = record
        .found
        .references
        .iter()
        .filter_map(|(_, reference)| match reference {
            ParamRef::Design(ParameterRef {
                through: Some(path),
                ..
            }) => Some(path),
            _ => None,
        })
        .collect();
    if setups.is_empty() {
        if shared && (record.found.holds_selection() || !through.is_empty()) {
            super::error(
                findings,
                id,
                format_args!(
                    "no setup uses the record, so it holds no selection and no `through` path"
                ),
            );
        }
        return;
    }
    for (at, workpiece) in &record.found.workpieces {
        let Some(index) = workpiece else { continue };
        for &setup in &setups {
            let count = records
                .setups
                .get(setup)
                .map_or(0, |setup| setup.workpieces.len());
            if usize::try_from(*index).map_or(true, |index| index >= count) {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "workpiece {index} at {at} indexes none of the {count} workpieces of setup {setup}"
                    ),
                );
            }
        }
    }
    let Some(scope) = records.scope_of(record.owner) else {
        return;
    };
    for (at, anchor) in &record.found.anchors {
        match anchor {
            Anchor::Input => super::error(
                findings,
                id,
                format_args!(
                    "selection at {at} is anchored `input`; a process selection is anchored `current` or `pinned`"
                ),
            ),
            Anchor::Pinned(position) => {
                let position = PositionKey::of(position);
                let exists = super::as_process(findings, |findings| {
                    check_position(records.design, position, id, findings)
                        && check_resulting_position(records.design, position, id, findings)
                });
                if exists {
                    let member = match position {
                        PositionKey::Start(container) => MemberKey::Container(container),
                        PositionKey::After(member) => member,
                    };
                    if records.design.scope_of(member) != Some(scope) {
                        super::error(
                            findings,
                            id,
                            format_args!(
                                "selection at {at} is pinned at {position}, which is not in the setup's scope {scope}"
                            ),
                        );
                    }
                }
            }
            Anchor::Current | Anchor::Native(_) => {}
        }
    }
    for path in through {
        check_path(records, scope, path, moved, id, findings);
    }
    for (at, context) in &record.found.contexts {
        let what = format!("pick context at {at}");
        match context {
            PickContext::Occurrence(path) => {
                check_path(records, scope, path, moved, id, findings);
            }
            PickContext::Definition(definition) => {
                check_definition(records, definition.definition.as_str(), id, &what, findings);
            }
            PickContext::InContext {
                assembly,
                own,
                target,
            } => {
                let assembly = assembly.definition.as_str();
                if check_definition(records, assembly, id, &what, findings) {
                    for path in own.iter().chain(target) {
                        check_path(records, assembly, path, moved, id, findings);
                    }
                }
            }
        }
    }
}

fn check_definition(
    records: &ProcessRecords<'_>,
    definition: &str,
    id: &str,
    what: &str,
    findings: &mut Vec<Finding>,
) -> bool {
    let exists = records.design.definitions.contains_key(definition);
    if !exists {
        super::error(
            findings,
            id,
            format_args!("{what} names {definition}, which is no product definition"),
        );
    }
    exists
}

/// An occurrence path resolves from `from` (§8.1), under the design rule.
fn check_path(
    records: &ProcessRecords<'_>,
    from: &str,
    path: &OccurrencePath,
    moved: &BTreeSet<&str>,
    id: &str,
    findings: &mut Vec<Finding>,
) {
    super::as_process(findings, |findings| {
        selections::check_path(records.design, Some(from), path, moved, id, findings);
    });
}

#[cfg(test)]
mod tests;
