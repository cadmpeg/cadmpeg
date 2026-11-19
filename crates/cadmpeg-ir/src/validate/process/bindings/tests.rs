// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::design::dimension::{Dimension, ParamRef, ParameterRef, RefTarget};
use crate::design::expr::{Expression, Neutral, Node};
use crate::process::groups::ProcessDefinition;
use crate::process::setup::JobKind;
use crate::process::Choice;
use crate::report::Severity;
use crate::validate::process::fixture::{assert_error, assert_finding, document, id};

#[test]
fn every_choice_parameter_resolves_and_a_choice_without_one_has_a_value() {
    let mut ir = document();
    ir.process.setups[0].job = Some(Choice {
        value: None,
        parameters: BTreeSet::from([id("parameter", "missing")]),
    });
    assert_error(
        &ir,
        "`Choice` at /job names test:process:parameter#missing, which is no process parameter",
    );

    let mut ir = document();
    ir.process.setups[0].job = Some(Choice::<JobKind> {
        value: None,
        parameters: BTreeSet::new(),
    });
    assert_error(
        &ir,
        "`Choice` at /job names no parameter and states no value",
    );
}

#[test]
fn every_param_ref_resolves() {
    let mut ir = document();
    ir.process.operations[2].technology.stepdown = Some(Dimension::Parameter(ParamRef::Process(
        id("parameter", "missing"),
    )));
    assert_error(
        &ir,
        "reference at /technology/stepdown/value names test:process:parameter#missing",
    );

    let mut ir = document();
    ir.process.operations[2].technology.stepover =
        Some(Dimension::Negated(ParamRef::Design(ParameterRef {
            target: RefTarget::Parameter(id("parameter", "missing")),
            through: None,
        })));
    assert_error(&ir, "which is no design parameter");
}

#[test]
fn a_parameter_dependency_cycle_is_a_warning() {
    let mut ir = document();
    ir.process.parameters[0].definition = ProcessDefinition::Expression(Expression::Neutral(
        Neutral(Node::Reference(ParamRef::Process(id("parameter", "depth")))),
    ));
    assert_finding(
        &ir,
        Severity::Warning,
        "process parameter depends on itself through its expression",
    );
}

#[test]
fn a_parameter_expressions_missing_reference_is_reported_once() {
    let mut ir = document();
    ir.process.parameters[0].definition =
        ProcessDefinition::Expression(Expression::Neutral(Neutral(Node::Reference(
            ParamRef::Process(id("parameter", "missing")),
        ))));
    let findings: Vec<_> = crate::validate::validate_neutral(&ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.message.contains("parameter#missing"))
        .collect();
    assert_eq!(findings.len(), 1, "{findings:#?}");
}
