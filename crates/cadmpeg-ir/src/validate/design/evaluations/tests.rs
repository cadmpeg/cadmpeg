// SPDX-License-Identifier: Apache-2.0

use crate::design::configuration::FeatureStateOverride;
use crate::design::container::{Container, OrderedKind, Rollback};
use crate::design::dimension::Dimension;
use crate::design::evaluation::{EvaluationStatus, StateBase};
use crate::design::feature::{FeatureOperation, OperationFamily, StoredOrigin};
use crate::design::parameter::ParameterValue;
use crate::design::refs::{Evaluated, Member, Position};
use crate::design::units::ValueKind;
use crate::validate::design::fixture::{
    after, assert_finding, body, document, feature, id, parameter, set_base,
};

#[test]
fn an_evaluation_names_a_configuration_exactly_when_its_scope_has_them() {
    let mut ir = document();
    ir.design.evaluations[0].configuration = None;
    assert_finding(
        &ir,
        "states no configuration, but its scope has configurations",
    );
    let mut ir = document();
    ir.design.evaluations[0].configuration = Some(id("configuration", "missing"));
    assert_finding(&ir, "which is no configuration of its scope");
}

#[test]
fn evaluation_keys_and_positions() {
    let mut ir = document();
    let mut repeat = ir.design.evaluations[0].clone();
    repeat.id = id("evaluation", "repeat");
    ir.design.evaluations.push(repeat);
    assert_finding(&ir, "repeats the scope, configuration, and `of`");
    let mut ir = document();
    ir.design.evaluations[1].of = Evaluated::State(Position::After(Member::Container(id(
        "container",
        "timeline",
    ))));
    assert_finding(&ir, "names no member of a container");
    let mut ir = document();
    ir.design.evaluations[1].of = Evaluated::State(Position::Start(id("container", "missing")));
    assert_finding(&ir, "names no ordered container");
    let mut ir = document();
    ir.design.evaluations[1].of = Evaluated::Tool(feature("missing"));
    assert_finding(&ir, "which is no feature of the design");
}

#[test]
fn an_evaluation_after_an_ordered_container_is_refused() {
    let mut ir = document();
    ir.design.containers.push(Container::Ordered {
        id: id("container", "body"),
        kind: OrderedKind::Body,
        members: Vec::new(),
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.push(Member::Container(id("container", "body")));
    ir.design.evaluations[1].of =
        Evaluated::State(Position::After(Member::Container(id("container", "body"))));
    assert_finding(
        &ir,
        "which is the state at that ordered container's rollback position",
    );
}

#[test]
fn a_stateless_feature_has_no_evaluation() {
    let mut ir = document();
    ir.design.features[0].definition.operation = FeatureOperation::Unresolved {
        family: OperationFamily::Sketch,
    };
    assert_finding(&ir, "is stateless in the evaluation's configuration");
    let mut ir = document();
    ir.design.configurations[0].overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: Some(true),
            definition: None,
        },
    );
    assert_finding(&ir, "is stateless in the evaluation's configuration");
    let mut ir = document();
    ir.design.features[1].suppressed = Some(Dimension::Literal(true));
    assert_finding(&ir, "is stateless in the evaluation's configuration");
    let mut ir = document();
    let mut flag = parameter("flag", ValueKind::Boolean);
    flag.value = Some(ParameterValue::Boolean(false));
    ir.design.parameters.push(flag);
    ir.design.features[0].suppressed = Some(Dimension::Negated(id("parameter", "flag")));
    assert_finding(&ir, "is stateless in the evaluation's configuration");
}

#[test]
fn an_evaluation_with_no_content_is_stale_or_failed() {
    let mut ir = document();
    ir.design.evaluations[1].status = EvaluationStatus::Current;
    assert_finding(&ir, "with no content has status `current`");
}

#[test]
fn the_active_configuration_has_no_evaluation_at_a_current_state() {
    let mut ir = document();
    ir.design.evaluations[1].configuration = Some(id("configuration", "base"));
    assert_finding(&ir, "repeats the current state");
}

#[test]
fn state_bases_name_states_with_content() {
    let mut ir = document();
    set_base(&mut ir, 0, StateBase::Current(after("extrude")));
    assert_finding(&ir, "is the position of no current state");
    let mut ir = document();
    set_base(
        &mut ir,
        0,
        StateBase::Evaluation(id("evaluation", "missing")),
    );
    assert_finding(&ir, "is no evaluation of the design");
    ir.fragment = true;
    assert_finding(&ir, "the fragment does not hold the evaluation's base");
    let mut ir = document();
    set_base(
        &mut ir,
        0,
        StateBase::Evaluation(id("evaluation", "large-after-fillet")),
    );
    assert_finding(&ir, "is an evaluation with no content");
    let mut ir = document();
    set_base(
        &mut ir,
        1,
        StateBase::Evaluation(id("evaluation", "after-extrude")),
    );
    set_base(
        &mut ir,
        0,
        StateBase::Evaluation(id("evaluation", "large-after-fillet")),
    );
    assert_finding(&ir, "is its own ancestor through its state bases");
}

#[test]
fn current_states_list_bodies_of_the_current_geometry_once() {
    let mut ir = document();
    ir.design.scopes[0].current[0]
        .bodies
        .push(id("body", "missing"));
    assert_finding(&ir, "which is no body of the current geometry");
    let mut ir = document();
    let body = body(&ir);
    ir.design.scopes[0].current[0].bodies.push(body);
    assert_finding(&ir, "belongs to 2 current states");
    let mut ir = document();
    ir.design.scopes[0].current[0].bodies.clear();
    assert_finding(&ir, "belongs to no current state");
}

#[test]
fn a_current_state_in_an_ordered_container_is_at_its_rollback() {
    let mut ir = document();
    ir.design.scopes[0].current[0].at = after("extrude");
    assert_finding(&ir, "not at the rollback position");
    let mut ir = document();
    ir.design.scopes[0].current[0].at =
        Position::After(Member::Container(id("container", "timeline")));
    assert_finding(&ir, "names no member of a container");
}

#[test]
fn a_stored_feature_names_bodies_of_the_state_after_it() {
    let mut ir = document();
    ir.design.features[1].definition.operation = FeatureOperation::Stored {
        bodies: Some(vec![id("body", "missing")]),
        tessellations: vec![id("tessellation", "missing")],
        origin: StoredOrigin::Base {},
    };
    assert_finding(&ir, "which is no body of the current state after it");
    assert_finding(&ir, "which is no tessellation of the model");
    let mut ir = document();
    ir.design.features[0].definition.operation = FeatureOperation::Stored {
        bodies: Some(vec![id("body", "missing")]),
        tessellations: Vec::new(),
        origin: StoredOrigin::Base {},
    };
    assert_finding(&ir, "which is no body of the state after it");
}
