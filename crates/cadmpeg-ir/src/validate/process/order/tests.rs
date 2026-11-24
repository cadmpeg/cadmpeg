// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_core::decode::ContentScope;

use super::{READER_JSON_DEPTH, RECORD_PREFIX_DEPTH};
use crate::document::CadIr;
use crate::process::operation::OperationKind;
use crate::process::setup::{ProgramStep, Setup, Step};
use crate::process::stock::StockRule;
use crate::validate::process::fixture::{
    assert_error, assert_no_process_finding, constant, document, id, process_findings,
};
use crate::validate::process::walk::walk;

#[test]
fn the_fixture_breaks_no_process_rule() {
    assert_no_process_finding(&document());
}

#[test]
fn every_operation_is_in_exactly_one_setup() {
    let mut ir = document();
    ir.process.setups[0].steps.remove(0);
    assert_error(&ir, "operation is in no setup");

    let mut ir = document();
    ir.process.setups[0]
        .steps
        .push(Step::Operation(id("operation", "mill")));
    assert_error(&ir, "operation is listed 2 times in setup steps");

    let mut ir = document();
    let mut second = ir.process.setups[0].clone();
    second.id = id("setup", "2");
    second.steps = vec![Step::Operation(id("operation", "drill"))];
    second.machine = None;
    ir.process.setups.push(second);
    assert_error(&ir, "operation is in setups");

    let mut ir = document();
    ir.process.setups[0]
        .steps
        .push(Step::Operation(id("operation", "missing")));
    assert_error(&ir, "which is no operation");
}

fn inputs(ir: &mut CadIr) -> &mut Vec<crate::design::ids::OperationId> {
    match &mut ir.process.operations[0].kind {
        OperationKind::Modifier { inputs, .. } => inputs,
        OperationKind::Machining { .. } => {
            unreachable!("the fixture's first operation is a modifier")
        }
    }
}

#[test]
fn modifier_inputs_are_nonempty_existing_and_acyclic() {
    let mut ir = document();
    inputs(&mut ir).clear();
    assert_error(&ir, "modifier takes no input");

    let mut ir = document();
    inputs(&mut ir).push(id("operation", "missing"));
    assert_error(
        &ir,
        "modifier input test:process:operation#missing is no operation",
    );

    let mut ir = document();
    inputs(&mut ir).push(id("operation", "dogbone"));
    assert_error(&ir, "operation is its own input through modifier inputs");
}

#[test]
fn every_operation_on_a_modifier_input_cycle_is_reported() {
    let mut ir = document();
    let modifier = ir.process.operations[0].clone();
    for (key, targets) in [
        ("a", &["b", "d"][..]),
        ("b", &["c"]),
        ("c", &["a"]),
        ("d", &["b"]),
    ] {
        let mut operation = modifier.clone();
        operation.id = id("operation", key);
        let OperationKind::Modifier { inputs, .. } = &mut operation.kind else {
            unreachable!("the fixture's first operation is a modifier")
        };
        *inputs = targets
            .iter()
            .map(|&target| id("operation", target))
            .collect();
        ir.process.operations.push(operation);
    }
    let reported: BTreeSet<String> = process_findings(&ir)
        .into_iter()
        .filter(|finding| finding.message == "operation is its own input through modifier inputs")
        .filter_map(|finding| finding.entity)
        .collect();
    let expected: BTreeSet<String> = ["a", "b", "c", "d"]
        .into_iter()
        .map(|key| format!("test:process:operation#{key}"))
        .collect();
    assert_eq!(reported, expected);
}

#[test]
fn program_steps_name_existing_setups_and_operations() {
    let mut ir = document();
    ir.process.programs[0].steps.push(ProgramStep::Group {
        name: None,
        steps: vec![ProgramStep::Setup(id("setup", "missing"))],
    });
    assert_error(
        &ir,
        "program step names test:process:setup#missing, which is no setup",
    );
}

fn second_setup(ir: &mut CadIr) -> &mut Setup {
    let mut second = ir.process.setups[0].clone();
    second.id = id("setup", "2");
    second.steps.clear();
    ir.process.setups.push(second);
    &mut ir.process.setups[1]
}

#[test]
fn a_previous_setup_is_another_existing_setup_without_a_cycle() {
    let mut ir = document();
    ir.process.setups[0].stock = Some(constant(StockRule::PreviousSetup(id("setup", "1"))));
    assert_error(&ir, "setup takes its stock from itself");

    let mut ir = document();
    ir.process.setups[0].stock = Some(constant(StockRule::PreviousSetup(id("setup", "9"))));
    assert_error(&ir, "which is no setup");

    let mut ir = document();
    ir.process.setups[0].stock = Some(constant(StockRule::PreviousSetup(id("setup", "2"))));
    second_setup(&mut ir).stock = Some(constant(StockRule::PreviousSetup(id("setup", "1"))));
    assert_error(&ir, "through previous setups");
}

/// Steps nested `depth` folders deep around the fixture's milling step.
fn nested(depth: usize) -> CadIr {
    let mut ir = document();
    let mut step = Step::Operation(id("operation", "mill"));
    for _ in 0..depth {
        step = Step::Folder {
            name: None,
            steps: vec![step],
        };
    }
    ir.process.setups[0].steps[0] = step;
    ir
}

/// The deepest folder nesting the rule admits.
fn deepest() -> usize {
    let depths = (1..READER_JSON_DEPTH).collect::<Vec<_>>();
    let admitted = depths.partition_point(|&depth| {
        RECORD_PREFIX_DEPTH + walk(&nested(depth).process.setups[0]).depth <= READER_JSON_DEPTH
    });
    depths[admitted - 1]
}

/// The nesting rule refuses exactly the documents the reader refuses, so a
/// validated document reads back.
#[test]
fn a_record_nests_no_deeper_than_the_reader_admits() {
    let deepest = deepest();
    let admitted = nested(deepest);
    assert_no_process_finding(&admitted);
    let text = serde_json::to_string(&admitted).unwrap();
    let back = CadIr::from_json(&text).unwrap();
    assert_eq!(serde_json::to_string(&back).unwrap(), text);

    let refused = nested(deepest + 1);
    assert_error(
        &refused,
        "JSON levels deep in the document, past the 127 a reader admits",
    );
    let text = serde_json::to_string(&refused).unwrap();
    let error = CadIr::from_json(&text).unwrap_err();
    assert!(
        error.to_string().contains("recursion limit exceeded"),
        "{error}"
    );
}

#[test]
fn an_empty_process_is_not_checked() {
    let mut ir = crate::validate::design::fixture::document();
    ir.set_scope(ContentScope::full()).unwrap();
    assert_no_process_finding(&ir);
}
