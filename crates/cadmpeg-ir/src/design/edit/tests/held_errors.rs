// SPDX-License-Identifier: Apache-2.0
//! An editor over a document that already holds an Error, and how an edit's
//! added Error findings are counted (§4).

use super::super::added_errors;
use super::{cache, errors_only, feature, id, mm, operation, parameter_id, real, timeline};
use crate::design::container::{Container, Rollback};
use crate::design::edit::{EditError, Literal};
use crate::design::feature::FeatureOperation;
use crate::design::parameter::ParameterValue;
use crate::design::refs::Member;
use crate::report::check::{Check, Finding, RuleName};
use crate::report::Severity;
use crate::CadIr;

/// The timeline with its rollback before feature 2, so that its current
/// state, after feature 3, is off the rollback: an Error that no edit below
/// touches.
fn off_its_rollback() -> CadIr {
    let mut document = timeline();
    let Container::Ordered { rollback, .. } = &mut document.design.containers[0] else {
        panic!("ordered");
    };
    *rollback = Some(Rollback::Before(Member::Feature(feature(2))));
    document
}

fn error_findings(document: &CadIr) -> Vec<Finding> {
    crate::validate::validate_neutral(document, Vec::new())
        .expect("validation runs")
        .findings
        .into_iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .collect()
}

#[test]
fn an_editor_over_a_document_that_holds_an_error_reports_it() {
    let mut document = off_its_rollback();
    let held = error_findings(&document);
    assert!(
        errors_only(&held),
        "a current state off the rollback is an Error"
    );
    let editor = document
        .edit(&crate::report::strictness::Strictness::default())
        .expect("an Error the document holds refuses nothing");
    let reported: Vec<_> = editor
        .findings()
        .iter()
        .filter(|finding| finding.severity >= Severity::Error)
        .cloned()
        .collect();
    assert_eq!(reported, held);
}

#[test]
fn edit_refuses_a_report_only_at_the_requested_floor() {
    use crate::report::strictness::Strictness;

    let mut document = off_its_rollback();
    let before = document.clone();
    let strictness = Strictness {
        floor: Some(Severity::Error),
        ..Strictness::default()
    };
    let Err(EditError::Refused(report)) = document.edit(&strictness) else {
        panic!("the error finding reaches the floor");
    };
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Error));
    assert_eq!(document, before);
    let strictness = Strictness {
        floor: Some(Severity::Blocking),
        pinned: true,
        ..Strictness::default().with_code("anything")
    };
    assert!(document.edit(&strictness).is_ok());
}

#[test]
fn edit_preserves_the_validation_resource_refusal() {
    use crate::design::evaluation::{StateBase, StateContent, StateDelta};
    use crate::design::refs::Evaluated;
    use crate::report::strictness::Strictness;
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension, ResourceFailure};

    let limit = DecodePolicy::default().limits.max_state_chain_depth;
    let limit_index = u32::try_from(limit).unwrap();
    let mut document = CadIr::empty();
    document.design.evaluations = (0..=limit_index)
        .map(|key| {
            let base = if key == 0 {
                StateBase::Empty
            } else {
                StateBase::Evaluation(id("evaluation", key - 1))
            };
            super::evaluation(
                key,
                Evaluated::Tool(feature(1)),
                None,
                Some(StateContent {
                    base,
                    delta: StateDelta::default(),
                }),
            )
        })
        .collect();
    document.design.evaluations.sort_by(|a, b| a.id.cmp(&b.id));
    let Err(EditError::ResourceLimit(refusal)) = document.edit(&Strictness::default()) else {
        panic!("the state chain exceeds the validation ceiling");
    };
    assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
    assert_eq!(refusal.limit, limit);
    assert_eq!(refusal.additional, 1);
    assert_eq!(
        document.design.evaluations.len(),
        usize::try_from(limit_index).unwrap() + 1
    );
}

#[test]
fn an_edit_of_a_document_that_holds_an_error_keeps_the_error_and_applies() {
    let mut document = off_its_rollback();
    let held = error_findings(&document);
    let mut editor = document
        .edit(&crate::report::strictness::Strictness::default())
        .unwrap();
    editor
        .set_dimension(
            &parameter_id(1),
            Literal::Number {
                value: real(12.0),
                unit: mm(),
            },
        )
        .expect("an edit that adds no Error applies");
    assert_eq!(error_findings(editor.document()), held);
    assert_eq!(
        cache(&document, 1),
        Some(ParameterValue::Number(real(12.0)))
    );
}

#[test]
fn an_edit_that_adds_an_error_to_a_document_that_holds_one_is_refused() {
    let mut document = off_its_rollback();
    let before = document.clone();
    let held = error_findings(&document);
    let stateless = operation(2, FeatureOperation::Spreadsheet(id("spreadsheet", 2))).definition;
    match document
        .edit(&crate::report::strictness::Strictness::default())
        .unwrap()
        .set_feature_definition(&feature(2), stateless)
    {
        Err(EditError::Invalid(added)) => {
            assert!(errors_only(&added), "{added:?}");
            assert!(
                added.iter().all(|finding| !held.contains(finding)),
                "only the added findings are returned: {added:?}"
            );
        }
        other => panic!("a stateless feature with evaluations is an Error: {other:?}"),
    }
    assert_eq!(document, before, "a refused edit changes nothing");
}

fn finding(rule: RuleName, severity: Severity, entity: Option<&str>, message: &str) -> Finding {
    Finding {
        check: Check::Design,
        rule,
        severity,
        message: message.to_owned(),
        entity: entity.map(str::to_owned),
        related: Vec::new(),
    }
}

#[test]
fn added_errors_count_each_rule_on_each_record() {
    use Severity::{Error, Warning};
    let stateless = crate::rule_name!("test.stateless-feature-evaluation");
    let held = finding(
        stateless.clone(),
        Error,
        Some("test:edit:feature#1"),
        "evaluation test:edit:evaluation#4 of a stateless feature",
    );
    let before = vec![held.clone()];
    let reworded = finding(
        stateless.clone(),
        Error,
        Some("test:edit:feature#1"),
        "the stateless feature has evaluation test:edit:evaluation#5 at 3.5 mm",
    );
    assert!(
        added_errors(&before, std::slice::from_ref(&reworded)).is_empty(),
        "the same rule on the same record is not added, whatever its message says"
    );
    let second = vec![held.clone(), reworded.clone()];
    assert_eq!(
        added_errors(&before, &second),
        vec![reworded],
        "a second finding of the rule on the record is added"
    );
    let elsewhere = finding(
        stateless.clone(),
        Error,
        Some("test:edit:feature#2"),
        "evaluation test:edit:evaluation#4 of a stateless feature",
    );
    assert_eq!(
        added_errors(&before, std::slice::from_ref(&elsewhere)),
        vec![elsewhere],
        "the rule on another record is added"
    );
    let other_rule = finding(
        crate::rule_name!("test.other-rule"),
        Error,
        Some("test:edit:feature#1"),
        "evaluation test:edit:evaluation#4 of a stateless feature",
    );
    assert_eq!(
        added_errors(&before, std::slice::from_ref(&other_rule)),
        vec![other_rule],
        "another rule on the record is added, though its message is the same"
    );
    let unnamed = finding(
        crate::rule_name!("test.root-container"),
        Error,
        None,
        "a scope holds no root container",
    );
    assert!(added_errors(
        std::slice::from_ref(&unnamed),
        std::slice::from_ref(&unnamed)
    )
    .is_empty());
    assert_eq!(
        added_errors(&[], std::slice::from_ref(&unnamed)),
        vec![unnamed]
    );
    let warning = finding(stateless, Warning, Some("test:edit:feature#1"), "a cycle");
    assert!(added_errors(&[], &[warning]).is_empty());
    assert!(added_errors(&before, &[]).is_empty());
}
