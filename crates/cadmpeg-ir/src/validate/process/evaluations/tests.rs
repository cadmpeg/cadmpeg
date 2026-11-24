// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

use crate::design::dimension::{Dimension, ParamRef};
use crate::design::evaluation::{EvaluationStatus, StateBase};
use crate::design::feature::FeatureOperation;
use crate::document::CadIr;
use crate::process::state::{ProcessBase, ProcessStateOf};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::validate::design::fixture as design;
use crate::validate::process::fixture::{
    assert_error, assert_no_process_finding, document, id, process_findings, state,
};

#[test]
fn toolpath_keys_are_unique_and_keyed_one_way_per_operation() {
    let mut ir = document();
    let mut twin = ir.process.toolpaths[0].clone();
    twin.id = id("toolpath", "twin");
    ir.process.toolpaths.push(twin);
    assert_error(
        &ir,
        "toolpath repeats the operation, feature, and configuration",
    );

    let mut ir = document();
    let mut keyed = ir.process.toolpaths[0].clone();
    keyed.id = id("toolpath", "nb");
    keyed.configuration = Some(design::id("configuration", "base"));
    ir.process.toolpaths.insert(0, keyed);
    assert_error(
        &ir,
        "toolpaths keyed by no configuration and toolpaths keyed by one",
    );

    let mut ir = document();
    ir.process.toolpaths[0].configuration = Some(design::id("configuration", "missing"));
    assert_error(&ir, "which is no configuration of its setup's scope");
}

#[test]
fn configuration_states_name_configurations_of_the_scope() {
    let mut ir = document();
    ir.process.operations[2]
        .configuration_states
        .insert(design::id("configuration", "large"), true);
    assert_no_process_finding(&ir);
    ir.process.operations[2]
        .configuration_states
        .insert(design::id("configuration", "missing"), true);
    assert_error(&ir, "which is no configuration of its setup's scope");

    let mut ir = document();
    ir.process.operations[2]
        .configuration_states
        .insert(design::id("configuration", "large"), true);
    ir.process.operations[2].suppressed = Some(Dimension::Parameter(ParamRef::Process(id(
        "parameter",
        "depth",
    ))));
    assert_error(
        &ir,
        "binds a parameter, so it states no configuration state",
    );
}

#[test]
fn process_state_keys_are_unique_and_keyed_one_way_per_of() {
    let mut ir = document();
    let mut twin = ir.process.states[1].clone();
    twin.id = id("state", "twin");
    ir.process.states.push(twin);
    assert_error(&ir, "repeats the `of` and configuration of process state");

    let mut ir = document();
    let mut keyed = ir.process.states[1].clone();
    keyed.id = id("state", "stock-base");
    keyed.configuration = Some(design::id("configuration", "base"));
    ir.process.states.insert(1, keyed);
    assert_error(&ir, "are keyed by no configuration and by one");

    let mut ir = document();
    ir.process.states[1].of = ProcessStateOf::Stock(id("setup", "missing"));
    assert_error(
        &ir,
        "process state is of test:process:setup#missing, which is no setup",
    );
}

#[test]
fn process_bases_resolve_to_states_with_content() {
    let mut ir = document();
    ir.process.states[0] = state(
        "after-mill",
        ProcessStateOf::After(id("operation", "mill")),
        ProcessBase::State(id("state", "missing")),
    );
    assert_error(
        &ir,
        "state base test:process:state#missing is no process state",
    );

    let mut ir = document();
    ir.process.states[1].content = None;
    ir.process.states[1].status = EvaluationStatus::Stale;
    assert_error(&ir, "is a process state with no content");

    let mut ir = document();
    ir.process.states[1].content = None;
    assert_error(
        &ir,
        "process state with no content and no feature has status `current`",
    );

    let mut ir = document();
    if let Some(content) = &mut ir.process.states[1].content {
        content.base = ProcessBase::Design(StateBase::Current(design::after("extrude")));
    }
    assert_error(
        &ir,
        "which is the position of no current state of the setup's scope",
    );

    let mut ir = document();
    if let Some(content) = &mut ir.process.states[1].content {
        content.base = ProcessBase::Design(StateBase::Evaluation(design::id(
            "evaluation",
            "after-extrude",
        )));
    }
    assert_no_process_finding(&ir);
    if let Some(content) = &mut ir.process.states[1].content {
        content.base = ProcessBase::Design(StateBase::Evaluation(design::id(
            "evaluation",
            "large-after-fillet",
        )));
    }
    assert_error(&ir, "is an evaluation with no content");
}

/// The process findings of `ir` under `policy`.
fn findings_under(ir: &CadIr, policy: &DecodePolicy) -> Vec<Finding> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
    let report = crate::validate::validate_neutral_for_decode(&ctx, ir, Vec::new()).unwrap();
    ctx.finish_session().unwrap();
    report
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Process)
        .collect()
}

#[test]
fn process_bases_form_a_forest_within_the_reader_ceiling() {
    let mut ir = document();
    if let Some(content) = &mut ir.process.states[1].content {
        content.base = ProcessBase::State(id("state", "after-mill"));
    }
    assert_error(
        &ir,
        "process state is its own ancestor through its state bases",
    );

    let mut policy = DecodePolicy::default();
    policy.limits.max_state_chain_depth = 8;
    let ceiling = policy.limits.max_state_chain_depth;
    let mut ir = document();
    ir.process.states.clear();
    let key = |at: u64| format!("{at:05}");
    for at in 1..=ceiling + 1 {
        let base = if at == 1 {
            ProcessBase::Design(StateBase::Empty)
        } else {
            ProcessBase::State(id("state", &key(at - 1)))
        };
        let mut chained = state(&key(at), ProcessStateOf::Stock(id("setup", "1")), base);
        chained.configuration = Some(design::id("configuration", &key(at)));
        ir.process.states.push(chained);
    }
    let findings = findings_under(&ir, &policy);
    assert!(
        findings
            .iter()
            .any(|finding| finding.message.contains(&format!(
                "chain of state bases is {} deep, past the ceiling of {ceiling}",
                ceiling + 1
            ))),
        "the chain past the ceiling is not refused"
    );
    assert!(
        !findings
            .iter()
            .any(|finding| finding.message.contains(&format!("is {ceiling} deep"))),
        "the chain at the ceiling is refused"
    );
}

#[test]
fn a_state_feature_is_a_removal_states_feature_of_the_scope() {
    let mut ir = document();
    ir.process.states[0].feature = Some(design::feature("extrude"));
    assert_error(&ir, "but only a `removal` state names one");

    let mut ir = document();
    ir.process.states.insert(
        1,
        state(
            "removal",
            ProcessStateOf::Removal(id("operation", "mill")),
            ProcessBase::Design(StateBase::Empty),
        ),
    );
    ir.process.states[1].content = None;
    ir.process.states[1].feature = Some(design::feature("extrude"));
    assert_no_process_finding(&ir);
    ir.process.states[1].feature = Some(design::feature("missing"));
    assert_error(&ir, "which is no feature of its setup's scope");
}

#[test]
fn a_material_removal_feature_is_named_by_removal_states_of_one_operation() {
    let mut ir = document();
    ir.design.features[0].definition.operation = FeatureOperation::MaterialRemoval;
    for (key, operation) in [("removal-drill", "drill"), ("removal-mill", "mill")] {
        let mut removal = state(
            key,
            ProcessStateOf::Removal(id("operation", operation)),
            ProcessBase::Design(StateBase::Empty),
        );
        removal.content = None;
        removal.feature = Some(design::feature("extrude"));
        ir.process.states.push(removal);
    }
    ir.process
        .states
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    assert_error(
        &ir,
        "material-removal feature is named by removal states of 2 operations: test:process:operation#drill, test:process:operation#mill",
    );
}

#[test]
fn removal_states_of_one_operation_in_several_configurations_name_one_feature() {
    let mut ir = document();
    ir.design.features[0].definition.operation = FeatureOperation::MaterialRemoval;
    for (key, configuration) in [("removal-base", "base"), ("removal-large", "large")] {
        let mut removal = state(
            key,
            ProcessStateOf::Removal(id("operation", "mill")),
            ProcessBase::Design(StateBase::Empty),
        );
        removal.configuration = Some(design::id("configuration", configuration));
        removal.content = None;
        removal.feature = Some(design::feature("extrude"));
        ir.process.states.push(removal);
    }
    ir.process
        .states
        .sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    assert_no_process_finding(&ir);
}

#[test]
fn a_resolved_process_state_names_what_it_holds() {
    let mut ir = document();
    if let Some(content) = &mut ir.process.states[1].content {
        content.delta.owners.insert(
            design::id("body", "missing"),
            Some(design::id("definition", "part")),
        );
    }
    assert_error(
        &ir,
        "owner entry names test:design:body#missing, which is no body of the resolved state",
    );
}

#[test]
fn a_process_state_meets_the_rules_of_one_topology() {
    let mut ir = document();
    let first = ir.geometry.coedges[0].clone();
    let other = ir
        .geometry
        .coedges
        .iter()
        .find(|coedge| coedge.edge != first.edge)
        .unwrap()
        .id
        .clone();
    let mut crossing = first.clone();
    crossing.radial_next = other;
    let stock = ir
        .process
        .states
        .iter()
        .position(|state| state.id.as_str().ends_with("#stock"))
        .unwrap();
    if let Some(content) = &mut ir.process.states[stock].content {
        content.delta.records.coedges.push(crossing);
    }
    let message = format!(
        "resolved state: record {}: radial ring crosses edges",
        first.id.as_str()
    );
    let found: Vec<_> = process_findings(&ir)
        .into_iter()
        .filter(|finding| finding.message == message)
        .collect();
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(
        found[0].entity.as_deref(),
        Some(ir.process.states[stock].id.as_str())
    );
}
