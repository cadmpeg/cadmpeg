// SPDX-License-Identifier: Apache-2.0
//! Bindings (§5.14, §7.7): every `Choice`'s parameters, every `ParamRef`,
//! and the dependencies among process parameters.

use std::collections::{BTreeMap, BTreeSet};

use super::order::on_cycles;
use super::records::ProcessRecords;
use super::walk::Walked;
use crate::design::dimension::{ParamRef, ParameterRef, RefTarget};
use crate::report::check::Finding;

pub(super) fn check_bindings(
    records: &ProcessRecords<'_>,
    walked: &[Walked<'_>],
    findings: &mut Vec<Finding>,
) {
    let ir = records.ir;
    let v10_parameters: BTreeSet<&str> = ir
        .model
        .parameters
        .iter()
        .map(|parameter| parameter.id.as_str())
        .collect();
    let mut graph: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for record in walked {
        let id = record.id;
        for choice in &record.found.choices {
            for parameter in &choice.parameters {
                if !records.parameters.contains_key(parameter.as_str()) {
                    super::error(
                        findings,
                        id,
                        format_args!(
                            "`Choice` at {} names {parameter}, which is no process parameter",
                            choice.at
                        ),
                    );
                }
            }
            if choice.parameters.is_empty() && !choice.has_value {
                super::error(
                    findings,
                    id,
                    format_args!(
                        "`Choice` at {} names no parameter and states no value",
                        choice.at
                    ),
                );
            }
        }
        // A process parameter's references are its expression's, which the
        // design layer's expression rules resolve (§5.13); only its
        // dependencies are read here.
        let expression = record.arena == "parameters";
        for (at, reference) in &record.found.references {
            if expression {
                if let ParamRef::Process(parameter) = reference {
                    graph.entry(id).or_default().push(parameter.as_str());
                }
                continue;
            }
            match reference {
                ParamRef::Process(parameter) => {
                    if !records.has_parameter(parameter) {
                        super::error(
                            findings,
                            id,
                            format_args!(
                                "reference at {at} names {}, which is no process parameter",
                                parameter.as_str()
                            ),
                        );
                    }
                }
                ParamRef::Design(ParameterRef {
                    target: RefTarget::Parameter(parameter),
                    ..
                }) => {
                    let exists = records.design.parameters.contains_key(parameter.as_str())
                        || v10_parameters.contains(parameter.as_str());
                    if !exists {
                        super::error(
                            findings,
                            id,
                            format_args!(
                                "reference at {at} names {}, which is no design parameter",
                                parameter.as_str()
                            ),
                        );
                    }
                }
                ParamRef::Design(ParameterRef {
                    target: RefTarget::External { definition, .. },
                    ..
                }) => {
                    if !records.design.definitions.contains_key(definition.as_str()) {
                        super::error(
                            findings,
                            id,
                            format_args!(
                                "reference at {at} names a parameter of {}, which is no product definition",
                                definition.as_str()
                            ),
                        );
                    }
                }
            }
        }
    }
    for id in on_cycles(&graph) {
        super::warning(
            findings,
            id,
            format_args!("process parameter depends on itself through its expression"),
        );
    }
}

#[cfg(test)]
mod tests;
