// SPDX-License-Identifier: Apache-2.0
//! Parameter programs (§5.14): owners, stages, and the parameters each
//! program assigns.

use std::collections::{BTreeMap, BTreeSet};

use super::records::{DesignRecords, MemberKey};
use crate::design::dimension::{ParameterRef, RefTarget};
use crate::design::parameter::ParameterDefinition;
use crate::design::program::{ParameterProgram, ProgramBody, Statement};
use crate::report::check::Finding;

pub(super) fn check_programs(index: &DesignRecords<'_>, findings: &mut Vec<Finding>) {
    let design = &index.ir.design;
    let mut programs: BTreeMap<&str, &ParameterProgram> = BTreeMap::new();
    for program in &design.programs {
        programs.entry(program.id.as_str()).or_insert(program);
    }
    let mut assigned: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for parameter in &design.parameters {
        if let ParameterDefinition::Assigned(program) = &parameter.definition {
            if programs.contains_key(program.as_str()) {
                assigned
                    .entry(program.as_str())
                    .or_default()
                    .push(parameter.id.as_str());
            } else {
                super::error(
                    findings,
                    parameter.id.as_str(),
                    format_args!(
                        "parameter is assigned by {}, which is no program of the design",
                        program.as_str()
                    ),
                );
            }
        }
    }
    for program in &design.programs {
        check_owner(index, program, findings);
        let ProgramBody::Neutral(statements) = &program.body else {
            continue;
        };
        let id = program.id.as_str();
        let targets = assign_targets(statements);
        let mut named = BTreeSet::new();
        for target in targets {
            // An `External` target names a parameter of a document that was
            // not read, so no record states its definition (interim, G2-04).
            let RefTarget::Parameter(parameter) = &target.target else {
                continue;
            };
            named.insert(parameter.as_str());
            let assigns_here = index.parameters.get(parameter.as_str()).is_some_and(|record| {
                matches!(&record.definition, ParameterDefinition::Assigned(by) if by.as_str() == id)
            });
            if !assigns_here {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "program assigns {}, whose definition is not `assigned` by this program",
                        target_text(target)
                    ),
                );
            }
        }
        for parameter in assigned.get(id).into_iter().flatten() {
            if !named.contains(parameter) {
                super::error(
                    findings,
                    id,
                    format_args!("program assigns {parameter} on no path"),
                );
            }
        }
    }
}

/// A program's owner is a feature of its scope, and a program with an owner
/// has no stage.
fn check_owner(index: &DesignRecords<'_>, program: &ParameterProgram, findings: &mut Vec<Finding>) {
    let id = program.id.as_str();
    let Some(owner) = &program.owner else {
        return;
    };
    let owner = owner.as_str();
    let scope = program.scope.as_str();
    if !index.features.contains_key(owner) {
        super::error(
            findings,
            id,
            format_args!("program's owner {owner} is no feature of the design"),
        );
    } else if index.scope_of(MemberKey::Feature(owner)) != Some(scope) {
        super::error(
            findings,
            id,
            format_args!("program's owner {owner} is no feature of its scope {scope}"),
        );
    }
    if let Some(stage) = &program.stage {
        super::error(
            findings,
            id,
            format_args!("program has an owner and states the stage {stage:?}"),
        );
    }
}

fn target_text(target: &ParameterRef) -> String {
    match &target.target {
        RefTarget::Parameter(parameter) if target.through.is_none() => {
            parameter.as_str().to_owned()
        }
        RefTarget::Parameter(parameter) => {
            format!("{} through an occurrence path", parameter.as_str())
        }
        RefTarget::External { definition, name } => {
            format!("{name} of the external definition {}", definition.as_str())
        }
    }
}

/// Every `Assign` target of a statement list, on every path, walked without
/// recursion.
pub(super) fn assign_targets(statements: &[Statement]) -> Vec<&ParameterRef> {
    let mut targets = Vec::new();
    let mut pending: Vec<&Statement> = statements.iter().rev().collect();
    while let Some(statement) = pending.pop() {
        match statement {
            Statement::Assign { target, .. } => targets.push(target),
            Statement::If {
                then, otherwise, ..
            } => {
                pending.extend(otherwise.iter().rev());
                pending.extend(then.iter().rev());
            }
        }
    }
    targets
}
