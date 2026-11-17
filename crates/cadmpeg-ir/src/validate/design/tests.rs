// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use cadmpeg_core::decode::ContentScope;
use cadmpeg_core::text::NonBlankString;

use crate::design::configuration::{
    Arrangement, ArrangementKind, DefinitionOverride, DesignConfiguration, FeatureStateOverride,
    Overrides, ParameterOverride, PartNumber,
};
use crate::design::container::{Container, Folder, OrderedKind, Rollback};
use crate::design::dimension::Dimension;
use crate::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::feature::{
    Feature, FeatureDefinition, FeatureOperation, OperationFamily, StoredOrigin,
};
use crate::design::names::NameEntry;
use crate::design::parameter::{
    DesignParameter, Deviations, ParameterDefinition, ParameterValue, Tolerance,
};
use crate::design::refs::{EntityRef, Evaluated, Member, Position};
use crate::design::scope::DesignScope;
use crate::design::units::{Quantity, ValueKind};
use crate::document::CadIr;
use crate::features::FeatureId;
use crate::ids::BodyId;
use crate::pmi::LimitsAndFits;
use crate::products::{ProductDefinition, ProductDefinitionKind};
use crate::report::check::{Check, Finding};
use crate::report::Severity;
use crate::scalar::FiniteReal;

fn id<T>(kind: &str, key: &str) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

fn feature(key: &str) -> FeatureId {
    id("feature", key)
}

fn after(key: &str) -> Position {
    Position::After(Member::Feature(feature(key)))
}

fn unresolved(key: &str, family: OperationFamily) -> Feature {
    Feature {
        id: feature(key),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Unresolved { family },
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn configuration(key: &str, parent: Option<&str>) -> DesignConfiguration {
    DesignConfiguration {
        id: id("configuration", key),
        scope: id("definition", "part"),
        name: key.to_owned(),
        parent: parent.map(|parent| id("configuration", parent)),
        comment: None,
        overrides: Overrides::default(),
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    }
}

fn evaluation(
    key: &str,
    configuration: &str,
    of: Evaluated,
    content: Option<StateContent>,
) -> Evaluation {
    Evaluation {
        id: id("evaluation", key),
        scope: id("definition", "part"),
        configuration: Some(id("configuration", configuration)),
        of,
        status: if content.is_some() {
            EvaluationStatus::Current
        } else {
            EvaluationStatus::Stale
        },
        content,
        native_ref: None,
    }
}

fn stored(base: StateBase) -> StateContent {
    StateContent {
        base,
        delta: StateDelta::default(),
    }
}

fn set_base(ir: &mut CadIr, evaluation: usize, base: StateBase) {
    ir.design.evaluations[evaluation].content = Some(stored(base));
}

fn body(ir: &CadIr) -> BodyId {
    ir.geometry.bodies[0].id.clone()
}

fn arrangement(key: &str, kind: ArrangementKind) -> Arrangement {
    Arrangement {
        id: id("arrangement", key),
        scope: id("definition", "part"),
        name: NonBlankString::new(key).unwrap(),
        kind,
        configuration: None,
        overrides: Overrides::default(),
        appearances: Vec::new(),
        moves: Vec::new(),
        nested: Vec::new(),
        native_ref: None,
    }
}

fn parameter(key: &str, kind: ValueKind) -> DesignParameter {
    DesignParameter {
        id: id("parameter", key),
        scope: id("definition", "part"),
        owner: None,
        name: Some(NonBlankString::new(key).unwrap()),
        kind,
        unit: None,
        definition: ParameterDefinition::Driven,
        value: None,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    }
}

/// A unit cube whose part has one scope: a timeline of an extrude and a
/// fillet, rolled to its end; two configurations; the current state after the
/// fillet holding the cube; and an evaluation of the base configuration after
/// the extrude, stored against the current state.
fn document() -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.set_scope(ContentScope::full()).unwrap();
    let body = body(&ir);
    ir.model.product_definitions.push(ProductDefinition {
        id: id("definition", "part"),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies: vec![body.clone()],
        sketches: Vec::new(),
        native_ref: None,
    });
    let design = &mut ir.design;
    design.scopes.push(DesignScope {
        definition: id("definition", "part"),
        roots: vec![id("container", "timeline")],
        active_configuration: Some(id("configuration", "base")),
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: after("fillet"),
            bodies: vec![body],
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
    });
    design.containers.push(Container::Ordered {
        id: id("container", "timeline"),
        kind: OrderedKind::Timeline,
        members: vec![
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
        ],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    design
        .features
        .push(unresolved("extrude", OperationFamily::Extrude));
    design
        .features
        .push(unresolved("fillet", OperationFamily::Fillet));
    design.configurations.push(configuration("base", None));
    design
        .configurations
        .push(configuration("large", Some("base")));
    design.evaluations.push(evaluation(
        "after-extrude",
        "base",
        Evaluated::State(after("extrude")),
        Some(stored(StateBase::Current(after("fillet")))),
    ));
    design.evaluations.push(evaluation(
        "large-after-fillet",
        "large",
        Evaluated::State(after("fillet")),
        None,
    ));
    ir
}

fn design_findings(ir: &CadIr) -> Vec<Finding> {
    super::super::validate_neutral(ir, Vec::new())
        .unwrap()
        .findings
        .into_iter()
        .filter(|finding| finding.check == Check::Design)
        .collect()
}

#[track_caller]
fn assert_finding(ir: &CadIr, fragment: &str) {
    let findings = design_findings(ir);
    assert!(
        findings.iter().any(
            |finding| finding.severity == Severity::Error && finding.message.contains(fragment)
        ),
        "no design error containing {fragment:?} in {findings:#?}"
    );
}

#[test]
fn the_baseline_document_breaks_no_rule() {
    let ir = document();
    let report = super::super::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
}

#[test]
fn design_records_outside_the_scope_break_the_scope_invariants() {
    let mut ir = document();
    let design = std::mem::take(&mut ir.design);
    ir.set_scope(ContentScope::geometry()).unwrap();
    ir.design = design;
    assert_finding(&ir, "breaks the scope invariants");
}

#[test]
fn design_identities_are_unique() {
    let mut ir = document();
    ir.design
        .features
        .push(unresolved("fillet", OperationFamily::Fillet));
    assert_finding(&ir, "is not unique among the design records");
}

#[test]
fn every_feature_and_container_is_a_member_exactly_once() {
    let mut ir = document();
    ir.design
        .features
        .push(unresolved("orphan", OperationFamily::Hole));
    assert_finding(&ir, "is a member or scope root 0 times");
    let mut ir = document();
    ir.design.scopes[0].roots.push(id("container", "timeline"));
    assert_finding(&ir, "is a member or scope root 2 times");
    let mut ir = document();
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.push(Member::Feature(feature("missing")));
    assert_finding(&ir, "which is no record of the design");
}

#[test]
fn containers_form_a_tree() {
    let mut ir = document();
    for (this, other) in [("a", "b"), ("b", "a")] {
        ir.design.containers.push(Container::Ordered {
            id: id("container", this),
            kind: OrderedKind::Body,
            members: vec![Member::Container(id("container", other))],
            folders: Vec::new(),
            rollback: Rollback::End,
            placement: None,
        });
    }
    assert_finding(&ir, "is its own ancestor through membership");
}

#[test]
fn rollback_and_folders_stay_within_the_container() {
    let mut ir = document();
    let Container::Ordered {
        rollback, folders, ..
    } = &mut ir.design.containers[0]
    else {
        unreachable!()
    };
    *rollback = Rollback::Before(Member::Feature(feature("missing")));
    folders.push(Folder {
        name: None,
        first: 0,
        last: 2,
    });
    assert_finding(&ir, "which is no member of the container");
    assert_finding(&ir, "is no run of the container's 2 members");
    let mut ir = document();
    let Container::Ordered { folders, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    folders.push(Folder {
        name: None,
        first: 0,
        last: 0,
    });
    folders.push(Folder {
        name: None,
        first: 0,
        last: 1,
    });
    assert!(design_findings(&ir).is_empty());
    let mut ir = document();
    ir.design.containers[0] = Container::Ordered {
        id: id("container", "timeline"),
        kind: OrderedKind::Timeline,
        members: vec![
            Member::Feature(feature("extrude")),
            Member::Feature(feature("fillet")),
            Member::Feature(feature("hole")),
        ],
        folders: vec![
            Folder {
                name: None,
                first: 0,
                last: 1,
            },
            Folder {
                name: None,
                first: 1,
                last: 2,
            },
        ],
        rollback: Rollback::Before(Member::Feature(feature("hole"))),
        placement: None,
    };
    ir.design
        .features
        .push(unresolved("hole", OperationFamily::Hole));
    assert_finding(&ir, "neither nest nor are disjoint");
}

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
fn a_resolved_state_holds_one_record_per_id() {
    let mut ir = document();
    let mut face = ir.geometry.faces[0].clone();
    face.id = body(&ir).as_str().to_owned().try_into().unwrap();
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta.records.faces.push(face);
    assert_finding(&ir, "two records of the resolved state share id");
}

#[test]
fn resolved_state_entries_name_what_the_state_holds() {
    let mut ir = document();
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta.names.insert(
        EntityRef::Face(id("face", "missing")),
        Some(NameEntry {
            name: None,
            native: None,
        }),
    );
    delta
        .owners
        .insert(id("body", "missing"), Some(id("definition", "part")));
    delta
        .members
        .insert(id("occurrence", "missing"), Some(id("definition", "part")));
    assert_finding(&ir, "name entry names");
    assert_finding(&ir, "owner entry names");
    assert_finding(&ir, "member entry names");
    let mut ir = document();
    let body = body(&ir);
    let delta = &mut ir.design.evaluations[0].content.as_mut().unwrap().delta;
    delta
        .removed
        .push(crate::design::refs::RecordRef::Body(body.clone()));
    delta.owners.insert(body, Some(id("definition", "part")));
    assert_finding(&ir, "which is no body of the resolved state");
}

#[test]
fn a_resolved_state_is_closed() {
    let mut ir = document();
    let face = ir.geometry.faces[0].clone();
    let content = ir.design.evaluations[0].content.as_mut().unwrap();
    content.base = StateBase::Empty;
    content.delta.records.faces.push(face);
    assert_finding(&ir, "which the state does not hold");
}

#[test]
fn the_current_geometry_names_its_own_entities() {
    let mut ir = document();
    ir.design.names = BTreeMap::from([(
        EntityRef::Edge(id("edge", "missing")),
        NameEntry {
            name: None,
            native: None,
        },
    )])
    .into();
    assert_finding(&ir, "which is no record of the current geometry's edges");
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
fn a_scope_names_an_active_configuration_of_its_own() {
    let mut ir = document();
    ir.design.scopes[0].active_configuration = None;
    assert_finding(&ir, "states no active configuration");
    let mut ir = document();
    ir.design.scopes[0].active_configuration = Some(id("configuration", "missing"));
    assert_finding(&ir, "is no configuration of the scope");
}

#[test]
fn configuration_parents_stay_in_one_scope_without_cycles() {
    let mut ir = document();
    ir.design.configurations[0].parent = Some(id("configuration", "large"));
    assert_finding(&ir, "is its own ancestor through `parent`");
    let mut ir = document();
    ir.design.configurations[1].parent = Some(id("configuration", "missing"));
    assert_finding(&ir, "is no configuration of the design");
}

#[test]
fn overrides_name_what_they_override_and_state_a_field() {
    let mut ir = document();
    ir.design
        .parameters
        .push(parameter("driven", ValueKind::Quantity(Quantity::LENGTH)));
    let overrides = &mut ir.design.configurations[1].overrides;
    overrides.parameters.insert(
        id("parameter", "driven"),
        ParameterOverride {
            expression: None,
            value: None,
        },
    );
    overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: None,
            definition: None,
        },
    );
    overrides
        .definitions
        .insert(id("definition", "part"), DefinitionOverride::default());
    assert_finding(&ir, "which is not defined by an expression");
    assert_finding(&ir, "states neither an expression nor a value");
    assert_finding(&ir, "feature-state override of");
    assert_finding(&ir, "definition override of");
    let mut ir = document();
    ir.design.features[0].suppressed = Some(Dimension::Parameter(id("parameter", "flag")));
    ir.design.configurations[1].overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: Some(false),
            definition: None,
        },
    );
    assert_finding(&ir, "whose suppression is bound to a parameter");
}

#[test]
fn part_number_rules_need_what_they_read() {
    let mut ir = document();
    ir.design.configurations[0].overrides.definitions.insert(
        id("definition", "part"),
        DefinitionOverride {
            part_number: Some(PartNumber::ParentConfigurationName),
            ..DefinitionOverride::default()
        },
    );
    ir.design.configurations[1].overrides.definitions.insert(
        id("definition", "part"),
        DefinitionOverride {
            part_number: Some(PartNumber::DocumentName),
            ..DefinitionOverride::default()
        },
    );
    assert_finding(
        &ir,
        "is the parent configuration's name, but no parent is stated",
    );
    assert_finding(
        &ir,
        "neither the definition nor the override states a `Title`",
    );
}

#[test]
fn arrangements_follow_their_kind_and_configuration() {
    let mut ir = document();
    let mut explode = arrangement("explode", ArrangementKind::Explode);
    explode.overrides.definitions.insert(
        id("definition", "part"),
        DefinitionOverride {
            label: Some("x".to_owned()),
            ..DefinitionOverride::default()
        },
    );
    explode.overrides.features.insert(
        feature("extrude"),
        FeatureStateOverride {
            suppressed: Some(true),
            definition: None,
        },
    );
    ir.design.arrangements.push(explode);
    assert_finding(&ir, "an arrangement's overrides hold a definition entry");
    assert_finding(
        &ir,
        "an explode arrangement states a parameter or feature override",
    );
    let mut ir = document();
    let mut owned = arrangement("owned", ArrangementKind::Display);
    owned.configuration = Some(id("configuration", "large"));
    ir.design.arrangements.push(owned);
    ir.design
        .arrangements
        .push(arrangement("other", ArrangementKind::Display));
    ir.design.scopes[0].active_arrangements =
        BTreeSet::from([id("arrangement", "owned"), id("arrangement", "other")]);
    assert_finding(&ir, "active arrangements name two arrangements of one kind");
    ir.design.scopes[0].active_arrangements = BTreeSet::from([id("arrangement", "owned")]);
    assert_finding(&ir, "is in force in configuration");
    ir.design.configurations[0].active_arrangements = BTreeSet::from([id("arrangement", "other")]);
    assert!(design_findings(&ir).is_empty());
}

#[test]
fn parameter_tolerances_need_a_quantity() {
    let mut ir = document();
    let mut text = parameter("text", ValueKind::Text);
    text.tolerance = Some(Tolerance::Basic);
    text.decimals = Some(2);
    let mut angle = parameter("angle", ValueKind::Quantity(Quantity::ANGLE));
    angle.tolerance = Some(Tolerance::Fit {
        fit: LimitsAndFits {
            form_variance: "H".to_owned(),
            zone_variance: "hole".to_owned(),
            grade: "7".to_owned(),
            source: "ISO 286".to_owned(),
        },
        mate: None,
        deviations: None,
    });
    let mut length = parameter("length", ValueKind::Quantity(Quantity::LENGTH));
    length.tolerance = Some(Tolerance::Deviations(Deviations {
        lower: FiniteReal::new(0.2).unwrap(),
        upper: FiniteReal::new(-0.1).unwrap(),
    }));
    ir.design.parameters.extend([angle, length, text]);
    assert_finding(&ir, "states a tolerance but its kind is no quantity");
    assert_finding(&ir, "states `decimals` but its kind is no quantity");
    assert_finding(&ir, "states a fit but its kind is no length");
    assert_finding(&ir, "is not below its upper deviation");
}
