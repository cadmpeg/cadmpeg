// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, LimitScope, ResourceLimit};
use cadmpeg_core::CodecError;

use crate::design::configuration::{FeatureStateOverride, ParameterOverride};
use crate::design::container::{Container, OrderedKind, Rollback};
use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::evaluation::{CurrentState, EvaluationStatus, StateBase};
use crate::design::expr::{parse, Expression, ExpressionDialect, ExpressionToken, Neutral, Node};
use crate::design::feature::{FeatureOperation, OperationFamily, StoredOrigin};
use crate::design::parameter::{ParameterDefinition, ParameterValue};
use crate::design::read;
use crate::design::refs::{Evaluated, Member, Position};
use crate::design::units::ValueKind;
use crate::document::CadIr;
use crate::report::check::{Check, Finding};
use crate::validate::design::fixture::{
    after, assert_finding, assert_no_design_finding, body, configuration, design_findings,
    document, evaluation, feature, id, parameter, set_base, stored, unresolved,
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
        "missing",
    ))));
    assert_finding(&ir, "names no member of a container");
    let mut ir = document();
    ir.design.evaluations[1].of = Evaluated::State(Position::After(Member::Container(id(
        "container",
        "timeline",
    ))));
    assert_finding(
        &ir,
        "which is the state at that ordered container's rollback position",
    );
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
        rollback: Some(Rollback::End),
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

fn literal(value: bool) -> Expression {
    Expression::Neutral(Neutral(Node::Boolean(value)))
}

/// The baseline with feature `extrude` suppressed by parameter `flag`, whose
/// definition and cache are `cached`, configuration `large` overriding `flag`
/// with the literal `overridden` and no cache, and an evaluation of `large`
/// after `extrude`.
fn suppressed_by_override(cached: bool, overridden: bool) -> CadIr {
    let mut ir = document();
    let mut flag = parameter("flag", ValueKind::Boolean);
    flag.definition = ParameterDefinition::Expression(literal(cached));
    flag.value = Some(ParameterValue::Boolean(cached));
    ir.design.parameters.push(flag);
    ir.design.features[0].suppressed = Some(Dimension::Parameter(id("parameter", "flag")));
    ir.design.configurations[1].overrides.parameters.insert(
        id("parameter", "flag"),
        ParameterOverride {
            expression: Some(literal(overridden)),
            value: None,
        },
    );
    ir.design.evaluations.push(evaluation(
        "large-after-extrude",
        "large",
        Evaluated::State(after("extrude")),
        Some(stored(StateBase::Current(after("fillet")))),
    ));
    ir
}

#[test]
fn a_suppression_override_with_no_cache_resolves_through_its_expression() {
    let ir = suppressed_by_override(false, true);
    assert_finding(&ir, "is stateless in the evaluation's configuration");
    let mut ir = suppressed_by_override(true, false);
    ir.design.evaluations.remove(0);
    assert_no_design_finding(&ir);
}

/// The baseline with feature `extrude` suppressed by parameter `flag`, whose
/// expression is the literal `false` and which has no cache, a driven
/// Boolean `other` with no cache, configuration `large` overriding `flag`
/// with `text` (`$0` reads `other`) and no cache, and an evaluation of
/// `large` after `extrude`.
fn suppressed_by_text(text: &str) -> CadIr {
    let mut ir = suppressed_by_override(false, false);
    ir.design.parameters[0].value = None;
    let mut undecided = parameter("other", ValueKind::Boolean);
    undecided.definition = ParameterDefinition::Expression(Expression::Source {
        dialect: ExpressionDialect::Fusion,
        tokens: vec![ExpressionToken::Text("unread".into())],
    });
    ir.design.parameters.push(undecided);
    let other = ParameterRef {
        target: RefTarget::Parameter(id("parameter", "other")),
        through: None,
    };
    let node = parse(text, &[other], &[]).unwrap();
    ir.design.configurations[1].overrides.parameters.insert(
        id("parameter", "flag"),
        ParameterOverride {
            expression: Some(Expression::Neutral(Neutral(node))),
            value: None,
        },
    );
    ir
}

fn reports_stateless(ir: &CadIr) -> bool {
    design_findings(ir).iter().any(|finding| {
        finding
            .message
            .contains("is stateless in the evaluation's configuration")
    })
}

#[test]
fn a_boolean_operator_with_an_undecided_operand_decides_no_suppression() {
    for text in [
        "true or $0",
        "$0 or true",
        "true or \"mm\"",
        "not (false and $0)",
        "not ($0 and false)",
        "if($0, true, true)",
    ] {
        assert!(!reports_stateless(&suppressed_by_text(text)), "{text}");
    }
    for text in [
        "true or false",
        "not false",
        "true != false",
        "if(true, true, $0)",
    ] {
        assert!(reports_stateless(&suppressed_by_text(text)), "{text}");
    }
    for text in [
        "true and false",
        "false or false",
        "not (true or false)",
        "if(true, false, $0)",
        "if(false, true, false)",
    ] {
        assert_no_design_finding(&suppressed_by_text(text));
    }
}

/// The baseline with feature `extrude` suppressed by parameter `flag`, whose
/// cache is true and whose definition is `definition`; where `differs`, the
/// base configuration that the evaluation after `extrude` names overrides
/// the fillet's suppression.
fn suppressed_by_cache(definition: ParameterDefinition, differs: bool) -> CadIr {
    let mut ir = document();
    let mut flag = parameter("flag", ValueKind::Boolean);
    flag.definition = definition;
    flag.value = Some(ParameterValue::Boolean(true));
    ir.design.parameters.push(flag);
    ir.design.features[0].suppressed = Some(Dimension::Parameter(id("parameter", "flag")));
    if differs {
        ir.design.configurations[0].overrides.features.insert(
            feature("fillet"),
            FeatureStateOverride {
                suppressed: Some(false),
                definition: None,
            },
        );
    }
    ir
}

#[test]
fn a_cache_that_reads_a_measure_or_the_geometry_holds_only_where_nothing_differs() {
    use crate::design::expr::{Measure, MeasureKind, MeasureTarget};
    use crate::design::selection::{Anchor, Picks, Selection};
    use crate::scalar::FiniteReal;

    let measured = Measure {
        kind: MeasureKind::Length,
        of: MeasureTarget::Selection(Selection {
            at: Anchor::Pinned(after("extrude")),
            picks: Picks::Unread,
            native: None,
        }),
        cached: Some(FiniteReal::new(2.0).unwrap()),
    };
    let reads_measure = || {
        ParameterDefinition::Expression(Expression::Neutral(Neutral(
            parse("@0 > 1[mm]", &[], std::slice::from_ref(&measured)).unwrap(),
        )))
    };
    for (name, definition) in [
        ("a measure", reads_measure()),
        ("the geometry", ParameterDefinition::Driven),
    ] {
        assert!(
            reports_stateless(&suppressed_by_cache(definition.clone(), false)),
            "{name}: the cache holds where the configuration states nothing"
        );
        assert!(
            !reports_stateless(&suppressed_by_cache(definition, true)),
            "{name}: the cache decides nothing where the configuration differs"
        );
    }
}

/// The baseline with `count` configurations of its scope, each with an
/// evaluation after `extrude` whose base is the previous one's, and the first
/// based on `Empty`.
fn state_chain(count: usize) -> CadIr {
    let mut ir = document();
    for at in 0..count {
        let key = format!("c{at}");
        ir.design.configurations.push(configuration(&key, None));
        let base = match at.checked_sub(1) {
            Some(previous) => StateBase::Evaluation(id("evaluation", &format!("e{previous}"))),
            None => StateBase::Empty,
        };
        ir.design.evaluations.push(evaluation(
            &format!("e{at}"),
            &key,
            Evaluated::State(after("extrude")),
            Some(stored(base)),
        ));
    }
    ir.finalize();
    ir
}

/// The design findings of `ir` under `policy`, or the validation's refusal.
fn findings_under(ir: &CadIr, policy: &DecodePolicy) -> Result<Vec<Finding>, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy).unwrap();
    let report = crate::validate::validate_neutral_for_decode(&ctx, ir, Vec::new());
    ctx.finish_session().unwrap();
    Ok(report?
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .collect())
}

#[test]
fn a_state_chain_past_the_ceiling_refuses_the_session_and_is_no_finding() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_state_chain_depth = 8;
    let ceiling = policy.limits.max_state_chain_depth;
    let depth = usize::try_from(ceiling).unwrap();
    let ir = state_chain(depth + 1);
    let refusal = findings_under(&ir, &policy).unwrap_err();
    assert!(
        matches!(
            refusal,
            CodecError::ResourceLimit(ResourceLimit {
                scope: LimitScope::Session,
                limit,
                ..
            }) if limit == ceiling
        ),
        "{refusal:?}"
    );
    let text = serde_json::to_string(&ir).unwrap();
    assert!(read::from_str::<CadIr>(&text, &policy.limits).is_err());
    let ir = state_chain(depth);
    let findings = findings_under(&ir, &policy).unwrap();
    assert!(findings.is_empty(), "{findings:#?}");
    let text = serde_json::to_string(&ir).unwrap();
    assert!(read::from_str::<CadIr>(&text, &policy.limits).is_ok());
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
    ir.design.scopes[0].current[0].at = timeline_end();
    assert_finding(
        &ir,
        "a stated rollback position is spelled through its member",
    );
    let mut ir = document();
    set_base(&mut ir, 0, StateBase::Current(timeline_end()));
    assert_finding(
        &ir,
        "a stated rollback position is spelled through its member",
    );
}

fn timeline_end() -> Position {
    Position::After(Member::Container(id("container", "timeline")))
}

/// The baseline with the timeline's rollback unstated, its current state at
/// `After(Container(timeline))`, and the evaluation after `extrude` based on
/// that state.
fn unstated_rollback() -> CadIr {
    let mut ir = document();
    let Container::Ordered { rollback, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    *rollback = None;
    ir.design.scopes[0].current[0].at = timeline_end();
    set_base(&mut ir, 0, StateBase::Current(timeline_end()));
    ir
}

#[test]
fn a_root_at_an_unstated_rollback_spells_its_current_state_after_the_container() {
    assert_no_design_finding(&unstated_rollback());
    let mut ir = unstated_rollback();
    ir.design.scopes[0].current[0].at = after("fillet");
    set_base(&mut ir, 0, StateBase::Current(after("fillet")));
    assert_finding(
        &ir,
        "not at the rollback position of container test:design:container#timeline",
    );
    let mut ir = unstated_rollback();
    ir.design.evaluations[1].of = Evaluated::State(timeline_end());
    assert_finding(
        &ir,
        "which is the state at that ordered container's rollback position",
    );
}

/// The baseline with an ordered `Body` container `body`, holding feature
/// `pad`, as the timeline's last member, rollback `rollback`, and a current
/// state at `at` holding no body.
fn nested_body(rollback: Option<Rollback>, at: Position) -> CadIr {
    let mut ir = document();
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.push(Member::Container(id("container", "body")));
    ir.design.containers.push(Container::Ordered {
        id: id("container", "body"),
        kind: OrderedKind::Body,
        members: vec![Member::Feature(feature("pad"))],
        folders: Vec::new(),
        rollback,
        placement: None,
    });
    ir.design
        .features
        .push(unresolved("pad", OperationFamily::Extrude));
    ir.design.scopes[0].current.push(CurrentState {
        at,
        bodies: Vec::new(),
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    });
    ir
}

#[test]
fn a_nested_container_at_an_unstated_rollback_spells_its_state_after_the_container() {
    let body = || Position::After(Member::Container(id("container", "body")));
    assert_no_design_finding(&nested_body(None, body()));
    assert_finding(
        &nested_body(None, after("pad")),
        "not at the rollback position of container test:design:container#body",
    );
    assert_finding(
        &nested_body(Some(Rollback::End), body()),
        "a stated rollback position is spelled through its member",
    );
    assert_no_design_finding(&nested_body(Some(Rollback::End), after("pad")));
}

#[test]
fn the_state_leaving_a_container_member_is_spelled_through_the_feature_before_it() {
    let body = || Position::After(Member::Container(id("container", "body")));
    for rollback in [
        Rollback::End,
        Rollback::Before(Member::Container(id("container", "body"))),
    ] {
        let mut ir = nested_body(None, body());
        let Container::Ordered { rollback: at, .. } = &mut ir.design.containers[0] else {
            unreachable!()
        };
        *at = Some(rollback);
        assert_eq!(ir.design.scopes[0].current[0].at, after("fillet"));
        assert_no_design_finding(&ir);
    }
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

#[test]
fn a_stored_feature_names_definition_meshes_of_its_scope() {
    use crate::tessellation::{Meshed, Tessellation, TessellationMesh};
    let with_mesh = |of: Meshed| {
        let mut ir = document();
        let mesh = Tessellation::new(
            id("tessellation", "mesh"),
            TessellationMesh::List {
                vertices: vec![
                    crate::math::Point3::new(0.0, 0.0, 0.0),
                    crate::math::Point3::new(1.0, 0.0, 0.0),
                    crate::math::Point3::new(0.0, 1.0, 0.0),
                ],
                triangles: vec![[0, 1, 2]],
            },
            Vec::new(),
        )
        .unwrap()
        .with_of(of);
        ir.model.tessellations.push(mesh);
        ir.design.features[1].definition.operation = FeatureOperation::Stored {
            bodies: Some(Vec::new()),
            tessellations: vec![id("tessellation", "mesh")],
            origin: StoredOrigin::Base {},
        };
        ir
    };
    let definition = |key| Meshed::Definition {
        definition: id("definition", key),
        name: None,
        color: None,
        visible: None,
    };
    let ir = with_mesh(definition("part"));
    assert!(!design_findings(&ir)
        .iter()
        .any(|finding| finding.message.contains("stored feature names")));
    assert_finding(
        &with_mesh(definition("other")),
        "a mesh of test:design:definition#other, which the feature's scope does not span",
    );
    assert_finding(
        &with_mesh(Meshed::Display {
            definition: id("definition", "part"),
        }),
        "whose `of` is not `Definition`",
    );
}

mod own_results;
mod tool_bodies;
mod unproduced;
