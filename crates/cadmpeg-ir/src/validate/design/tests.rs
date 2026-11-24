// SPDX-License-Identifier: Apache-2.0
//! Tests of the design rules: a document that meets every rule, and one
//! breach per rule, each reported with its check and severity.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::design::container::Container;
use crate::design::evaluation::{CurrentState, EvaluationStatus};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::names::{
    GenerationRole, HoleFaceRole, InstanceIndex, NameEntry, NameKind, NameTable, TopoName,
};
use crate::design::refs::EntityRef;
use crate::design::scope::DesignScope;
use crate::design::selection::{Anchor, PickTarget, Picked, Witness};
use crate::document::{CadIr, Topology};
use crate::report::check::{Check, Finding};
use crate::report::Severity;

mod refusals;

fn id<T>(kind: &str, key: &str) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:design:{kind}#{key}")).unwrap()
}

fn key(kind: &str, key: &str) -> String {
    format!("test:design:{kind}#{key}")
}

fn from<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}

fn feature(name: &str, operation: Value) -> Feature {
    Feature {
        id: id("feature", name),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: from(operation),
            assembly: None,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn principal(entity: &str) -> Value {
    json!({"target": {"kind": "principal", "value": {"entity": entity}}})
}

fn datum_pick(name: &str) -> Value {
    json!({"target": {"kind": "datum", "value": {"feature": key("feature", name)}}})
}

fn named(at: Value, picks: Vec<Value>) -> Value {
    let mut selection = json!({"picks": {"kind": "named"}});
    selection["at"] = at;
    selection["picks"]["value"] = Value::Array(picks);
    selection
}

fn input(relation: &str, reference: Option<Value>, value: Option<Value>) -> Value {
    let mut input = json!({"role": {"subject": "whole", "relation": {"kind": relation}}});
    if let Some(reference) = reference {
        input["reference"] = json!({"kind": "picks", "value": reference});
    }
    if let Some(value) = value {
        input["value"] = value;
    }
    input
}

fn construction(inputs: Vec<Value>) -> Value {
    let mut construction = json!({"after": [], "resolved_in": {}});
    construction["inputs"] = Value::Array(inputs);
    construction
}

fn plane_datum(construction: Value) -> Value {
    let mut datum = json!({
        "kind": "datum",
        "value": {
            "element": {"kind": "plane"},
            "native_options": {},
            "source_settings": {}
        }
    });
    datum["value"]["element"]["value"] = construction;
    datum
}

fn length(value: f64) -> Value {
    json!({"kind": "length", "value": {"kind": "literal", "value": value}})
}

fn input_anchor() -> Value {
    json!({"kind": "input"})
}

/// A timeline of features in one scope.
fn timeline(features: Vec<Feature>) -> CadIr {
    let mut ir = CadIr::empty();
    let members = features
        .iter()
        .map(|feature| json!({"kind": "feature", "value": feature.id.as_str()}))
        .collect::<Vec<_>>();
    ir.design.containers.push(from(json!({
        "kind": "ordered",
        "value": {
            "id": key("container", "timeline"),
            "kind": "timeline",
            "members": members,
            "folders": [],
            "rollback": {"kind": "end"}
        }
    })));
    ir.design.scopes.push(DesignScope {
        definition: id("product_definition", "part"),
        roots: vec![id("container", "timeline")],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: Vec::new(),
        unproduced: BTreeSet::default(),
    });
    ir.design.features = features;
    ir.design
        .features
        .sort_by(|left, right| left.id.cmp(&right.id));
    ir
}

/// A datum plane on the XY plane, and a datum plane offset from it.
fn planes() -> Vec<Feature> {
    vec![
        feature(
            "a",
            plane_datum(construction(vec![input(
                "coincident",
                Some(named(input_anchor(), vec![principal("xy_plane")])),
                None,
            )])),
        ),
        feature(
            "b",
            plane_datum(construction(vec![input(
                "offset",
                Some(named(input_anchor(), vec![datum_pick("a")])),
                Some(length(10.0)),
            )])),
        ),
    ]
}

fn findings(ir: &CadIr) -> Vec<Finding> {
    let report = crate::validate::validate_neutral(ir, Vec::new()).unwrap();
    report.findings
}

#[track_caller]
fn assert_finding(ir: &CadIr, check: Check, severity: Severity, text: &str) {
    let findings = findings(ir);
    assert!(
        findings.iter().any(|finding| finding.check == check
            && finding.severity == severity
            && finding.message.contains(text)),
        "no {check} {severity:?} finding containing {text:?}: {findings:#?}"
    );
}

#[track_caller]
fn assert_clean(ir: &CadIr) {
    let findings = findings(ir);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn a_design_that_meets_every_rule_has_no_finding() {
    assert_clean(&timeline(planes()));
}

#[test]
fn an_input_anchor_outside_an_ordered_container_is_an_error() {
    let mut ir = timeline(planes());
    ir.design.containers[0] = from(json!({
        "kind": "unordered",
        "value": {
            "id": key("container", "timeline"),
            "kind": "group",
            "members": [
                {"kind": "feature", "value": key("feature", "a")},
                {"kind": "feature", "value": key("feature", "b")}
            ]
        }
    }));
    assert_finding(&ir, Check::Design, Severity::Error, "anchored Input");
}

#[test]
fn a_pick_of_a_later_feature_is_an_error_and_a_cycle_back_edge_a_warning() {
    let mut features = planes();
    features.reverse();
    let mut ir = timeline(Vec::new());
    ir.design.features.clone_from(&features);
    ir.design
        .features
        .sort_by(|left, right| left.id.cmp(&right.id));
    let Container::Ordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    *members = features
        .iter()
        .map(|feature| from(json!({"kind": "feature", "value": feature.id.as_str()})))
        .collect();
    assert_finding(
        &ir,
        Check::Design,
        Severity::Error,
        "does not depend-order before its anchor",
    );

    let back = feature(
        "a",
        plane_datum(construction(vec![input(
            "offset",
            Some(named(input_anchor(), vec![datum_pick("b")])),
            Some(length(1.0)),
        )])),
    );
    let mut cycle = timeline(vec![back, planes().remove(1)]);
    cycle
        .design
        .features
        .sort_by(|left, right| left.id.cmp(&right.id));
    assert_finding(
        &cycle,
        Check::Design,
        Severity::Warning,
        "back-edge of a dependency cycle",
    );
}

#[test]
fn a_pick_of_a_missing_record_is_an_error() {
    let mut features = planes();
    features[1] = feature(
        "b",
        plane_datum(construction(vec![input(
            "offset",
            Some(named(input_anchor(), vec![datum_pick("missing")])),
            Some(length(10.0)),
        )])),
    );
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "does not exist",
    );
}

#[test]
fn a_native_pick_without_a_native_name_or_witness_is_an_error() {
    let mut features = planes();
    features[1] = feature(
        "b",
        plane_datum(construction(vec![input(
            "offset",
            Some(named(
                input_anchor(),
                vec![json!({"target": {"kind": "native", "value": {}}})],
            )),
            Some(length(10.0)),
        )])),
    );
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "neither a native name nor a witness",
    );
}

#[test]
fn a_datum_part_the_datum_lacks_is_an_error() {
    let mut features = planes();
    features[1] = feature(
        "b",
        plane_datum(construction(vec![input(
            "offset",
            Some(named(
                input_anchor(),
                vec![json!({"target": {"kind": "datum", "value": {
                    "feature": key("feature", "a"),
                    "part": {"kind": "point", "value": 3}
                }}})],
            )),
            Some(length(10.0)),
        )])),
    );
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "does not have",
    );
}

#[test]
fn rule_terms_are_checked() {
    let rule = |terms: Vec<Value>| json!({"at": {"kind": "input"}, "picks": {"kind": "rule", "value": {"terms": terms, "excluded": []}}});
    let explicit = json!({"rule": {"kind": "explicit"}, "seeds": [datum_pick("a")]});
    let seeded_all =
        json!({"rule": {"kind": "all", "value": {"kind": "face"}}, "seeds": [datum_pick("a")]});
    for (terms, check, text) in [
        (vec![explicit.clone()], Check::Design, "is Named"),
        (
            vec![explicit.clone(), explicit],
            Check::Design,
            "equal propagation",
        ),
        (vec![seeded_all], Check::Design, "All rule term has seeds"),
    ] {
        let mut features = planes();
        features[1] = feature(
            "b",
            plane_datum(construction(vec![input(
                "offset",
                Some(rule(terms)),
                Some(length(10.0)),
            )])),
        );
        assert_finding(&timeline(features), check, Severity::Error, text);
    }
}

#[test]
fn construction_inputs_are_checked_against_their_kind_and_relation() {
    let point = |inputs: Vec<Value>| {
        json!({"kind": "datum", "value": {
            "element": {"kind": "points", "value": [construction(inputs)]},
            "native_options": {},
            "source_settings": {}
        }})
    };
    let at_a = || Some(named(input_anchor(), vec![datum_pick("a")]));
    let mut origin = input("coincident", at_a(), None);
    origin["role"]["subject"] = json!("origin");
    let mut reversed = input("offset", at_a(), Some(length(1.0)));
    reversed["reversed"] = json!(true);
    for (operation, text) in [
        (point(vec![origin]), "has a Origin subject"),
        (
            plane_datum(construction(vec![input("offset", at_a(), None)])),
            "has a value it does not take",
        ),
        (
            plane_datum(construction(vec![input(
                "radius",
                at_a(),
                Some(length(1.0)),
            )])),
            "occurs on a plane construction",
        ),
        (plane_datum(construction(vec![reversed])), "states reversed"),
        (
            plane_datum(construction(Vec::new())),
            "no input, no step, and no resolved value",
        ),
        (
            plane_datum(construction(vec![json!({})])),
            "an input with neither a reference nor a value",
        ),
    ] {
        let mut features = planes();
        features[1] = feature("b", operation);
        assert_finding(&timeline(features), Check::Design, Severity::Error, text);
    }
    let mut features = planes();
    let mut flipped = construction(vec![input("offset", at_a(), Some(length(1.0)))]);
    flipped["after"] = json!([{"kind": "flip", "value": {"kind": "literal", "value": false}}]);
    flipped["solution"] = json!(0);
    features[1] = feature("b", plane_datum(flipped));
    let ir = timeline(features);
    assert_finding(&ir, Check::Design, Severity::Error, "literal false");
    assert_finding(&ir, Check::Design, Severity::Error, "solution is 0");
}

#[test]
fn a_built_centroid_reference_is_a_warning() {
    let point = |inputs: Vec<Value>| {
        json!({"kind": "datum", "value": {
            "element": {"kind": "points", "value": [construction(inputs)]},
            "native_options": {},
            "source_settings": {}
        }})
    };
    let built = json!({
        "role": {"subject": "whole", "relation": {"kind": "centroid"}},
        "reference": {"kind": "built", "value": {
            "kind": "point",
            "inputs": [input(
                "coincident",
                Some(named(input_anchor(), vec![principal("origin")])),
                None,
            )],
            "after": [],
            "resolved_in": {}
        }}
    });
    let mut features = planes();
    features[1] = feature("b", point(vec![built]));
    let ir = timeline(features);
    let message = "a nested construction is the reference of a Centroid input";
    assert_finding(&ir, Check::Design, Severity::Warning, message);
    let findings = findings(&ir);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.severity == Severity::Error
                && finding.message.contains("nested construction")),
        "{findings:#?}"
    );
}

#[test]
fn a_resolved_plane_whose_axes_are_not_perpendicular_is_an_error() {
    let mut features = planes();
    let mut skew = construction(vec![input(
        "offset",
        Some(named(input_anchor(), vec![datum_pick("a")])),
        Some(length(10.0)),
    )]);
    skew["resolved"] = json!({
        "origin": {"x": 0.0, "y": 0.0, "z": 10.0},
        "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
        "x_axis": {"x": 0.0, "y": 0.6, "z": 0.8}
    });
    features[1] = feature("b", plane_datum(skew));
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "not perpendicular",
    );
}

#[test]
fn a_principal_entity_a_datum_restates_is_picked_through_the_datum() {
    let mut features = planes();
    features.push(feature(
        "c",
        plane_datum(construction(vec![input(
            "offset",
            Some(named(input_anchor(), vec![principal("xy_plane")])),
            Some(length(5.0)),
        )])),
    ));
    assert_finding(
        &timeline(features.clone()),
        Check::Design,
        Severity::Error,
        "picks that feature as Datum",
    );
    features[2] = feature(
        "c",
        plane_datum(construction(vec![input(
            "coincident",
            Some(named(input_anchor(), vec![principal("xy_plane")])),
            None,
        )])),
    );
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "already restates",
    );
}

/// The planes in a Fusion scope `part` that places component `bracket`,
/// with plane `b` a feature of `bracket` picking the timeline's origin.
fn bracket_plane_on_the_timeline_origin() -> CadIr {
    use crate::products::{Occurrence, ProductDefinition, Storage};
    let mut features = planes();
    features[1] = feature(
        "b",
        plane_datum(construction(vec![input(
            "coincident",
            Some(named(
                input_anchor(),
                vec![json!({"target": {"kind": "principal", "value": {
                    "entity": "xy_plane",
                    "container": key("container", "timeline")
                }}})],
            )),
            None,
        )])),
    );
    features[1].component = Some(id("product_definition", "bracket"));
    let mut ir = timeline(features);
    let occurrence = Occurrence::new(
        id("occurrence", "bracket"),
        id("product_definition", "bracket"),
        None,
    );
    let mut part = ProductDefinition::new(id("product_definition", "part"), Storage::Embedded);
    part.occurrences.push(occurrence.id.clone());
    ir.model.product_definitions = vec![
        ProductDefinition::new(id("product_definition", "bracket"), Storage::Embedded),
        part,
    ];
    ir.model.occurrences.push(occurrence);
    crate::examples::add_document_root(&mut ir);
    ir
}

#[test]
fn a_principal_container_is_of_the_scope_that_spans_the_picks_definition() {
    assert_clean(&bracket_plane_on_the_timeline_origin());

    let mut ir = bracket_plane_on_the_timeline_origin();
    ir.model.product_definitions[1].occurrences.clear();
    ir.model.occurrences.clear();
    assert_finding(
        &ir,
        Check::Design,
        Severity::Error,
        "not an ordered container of the scope that spans its definition",
    );
}

#[test]
fn a_coordinates_reference_picks_a_frame_first() {
    let mut features = planes();
    features[1] = feature(
        "b",
        json!({"kind": "datum", "value": {
            "element": {"kind": "points", "value": [construction(vec![input(
                "coordinates",
                Some(named(input_anchor(), vec![datum_pick("a")])),
                Some(json!({"kind": "position", "value": {
                    "x": {"kind": "literal", "value": 1.0},
                    "y": {"kind": "literal", "value": 2.0},
                    "z": {"kind": "literal", "value": 3.0}
                }})),
            )])]},
            "native_options": {},
            "source_settings": {}
        }}),
    );
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "does not pick a frame first",
    );
}

#[test]
fn a_pinned_position_that_does_not_resolve_is_an_error() {
    let mut features = planes();
    features[1] = feature(
        "b",
        plane_datum(construction(vec![input(
            "offset",
            Some(named(
                json!({"kind": "pinned", "value": {"kind": "after", "value": {"kind": "feature", "value": key("feature", "gone")}}}),
                vec![datum_pick("a")],
            )),
            Some(length(10.0)),
        )])),
    );
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "does not resolve",
    );
}

/// Datum `a` on the XY plane and datum `b` offset from it, members of one
/// unordered group, `b`'s input anchored at `anchor`.
fn grouped(anchor: Value) -> CadIr {
    let features = vec![
        feature(
            "a",
            plane_datum(construction(vec![input(
                "coincident",
                Some(named(
                    json!({"kind": "current"}),
                    vec![principal("xy_plane")],
                )),
                None,
            )])),
        ),
        feature(
            "b",
            plane_datum(construction(vec![input(
                "offset",
                Some(named(anchor, vec![datum_pick("a")])),
                Some(length(10.0)),
            )])),
        ),
    ];
    let mut ir = timeline(features);
    ir.design.containers[0] = from(json!({
        "kind": "unordered",
        "value": {
            "id": key("container", "timeline"),
            "kind": "group",
            "members": [
                {"kind": "feature", "value": key("feature", "a")},
                {"kind": "feature", "value": key("feature", "b")}
            ]
        }
    }));
    ir
}

fn pinned_after(member: &str, key: &str) -> Value {
    json!({"kind": "pinned", "value": {"kind": "after", "value": {"kind": member, "value": key}}})
}

#[test]
fn a_group_member_is_pinned_only_after_a_member_with_a_result_of_its_own() {
    assert_clean(&grouped(json!({"kind": "current"})));
    // A datum is stateless, so it has no result of its own (§5.3, §5.4).
    assert_finding(
        &grouped(pinned_after("feature", &key("feature", "a"))),
        Check::Design,
        Severity::Error,
        "with no result of its own in any configuration",
    );
    let mut ir = grouped(pinned_after("feature", &key("feature", "a")));
    ir.design.features[0].definition.operation = FeatureOperation::Unresolved {
        family: crate::design::feature::OperationFamily::Extrude,
    };
    assert_clean(&ir);
}

#[test]
fn a_group_member_is_not_pinned_after_a_container() {
    let mut ir = grouped(pinned_after("container", &key("container", "body")));
    ir.design.containers.push(from(json!({
        "kind": "ordered",
        "value": {
            "id": key("container", "body"),
            "kind": "body",
            "members": [],
            "folders": [],
            "rollback": {"kind": "end"}
        }
    })));
    let Container::Unordered { members, .. } = &mut ir.design.containers[0] else {
        unreachable!()
    };
    members.insert(from(
        json!({"kind": "container", "value": key("container", "body")}),
    ));
    assert_finding(
        &ir,
        Check::Design,
        Severity::Error,
        "is pinned to a state other than another member's result",
    );
    assert_finding(
        &grouped(pinned_after("container", &key("container", "timeline"))),
        Check::Design,
        Severity::Error,
        "pinned to a position that does not resolve",
    );
}

#[test]
fn a_linked_datum_states_no_link_source() {
    let mut features = planes();
    let FeatureOperation::Datum(datum) = &mut features[1].definition.operation else {
        unreachable!()
    };
    datum.link = Some(from(
        json!({"source": named(json!({"kind": "current"}), vec![datum_pick("a")])}),
    ));
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "states a source",
    );
}

#[test]
fn an_occurrence_path_with_an_occurrence_after_a_native_step_is_an_error() {
    let mut features = planes();
    features[1] = feature(
        "b",
        plane_datum(construction(vec![input(
            "offset",
            Some(named(
                input_anchor(),
                vec![json!({
                    "target": {"kind": "principal", "value": {"entity": "yz_plane"}},
                    "context": {"kind": "occurrence", "value": [
                        {"occurrence": {"kind": "native", "value": {"codec": "test", "value": {"kind": "text", "value": "gone"}}}},
                        {"occurrence": {"kind": "occurrence", "value": key("occurrence", "1")}}
                    ]}
                })],
            )),
            Some(length(10.0)),
        )])),
    );
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "after a Native step",
    );
}

fn generated(kind: &str, feature: &str) -> TopoName {
    from(json!({"kind": "generated", "value": {"kind": kind, "feature": key("feature", feature)}}))
}

/// The unit cube as the current state after the plane `b`, its first face
/// named by `a`.
fn named_cube() -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    let design = timeline(planes()).design;
    ir.design = design;
    ir.design.scopes[0].current.push(CurrentState {
        at: from(
            json!({"kind": "after", "value": {"kind": "feature", "value": key("feature", "b")}}),
        ),
        bodies: ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect(),
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    });
    let face = EntityRef::Face(ir.geometry.faces[0].id.clone());
    ir.design.names = NameTable::from(BTreeMap::from([(
        face,
        NameEntry {
            name: Some(generated("face", "a")),
            native: None,
        },
    )]));
    crate::examples::add_document_root(&mut ir);
    ir
}

#[test]
fn name_tables_are_checked_in_their_states() {
    assert_clean(&named_cube());

    let mut ir = named_cube();
    let mut entries = ir.design.names.entries().clone();
    let entry = entries.values_mut().next().unwrap();
    entry.name = Some(generated("edge", "a"));
    ir.design.names = entries.into();
    assert_finding(&ir, Check::Design, Severity::Error, "not the entity's kind");

    let mut ir = named_cube();
    let mut entries = ir.design.names.entries().clone();
    entries.insert(
        EntityRef::Face(id("face", "missing")),
        NameEntry {
            name: None,
            native: None,
        },
    );
    ir.design.names = entries.into();
    assert_finding(&ir, Check::Design, Severity::Error, "does not hold");
    assert_finding(
        &ir,
        Check::Design,
        Severity::Error,
        "neither a name nor a native name",
    );

    let mut ir = named_cube();
    ir.design.scopes[0].current[0].at =
        from(json!({"kind": "after", "value": {"kind": "feature", "value": key("feature", "a")}}));
    let mut entries = ir.design.names.entries().clone();
    let entry = entries.values_mut().next().unwrap();
    entry.name = Some(generated("face", "b"));
    ir.design.names = entries.into();
    assert_finding(&ir, Check::Design, Severity::Error, "does not depend on");
}

#[test]
fn instance_names_and_hole_roles_name_the_features_that_make_them() {
    let mut ir = named_cube();
    let mut entries = ir.design.names.entries().clone();
    let entry = entries.values_mut().next().unwrap();
    entry.name = Some(from(json!({"kind": "instance", "value": {
        "feature": key("feature", "a"),
        "instance": [1],
        "of": generated("face", "a")
    }})));
    ir.design.names = entries.into();
    assert_finding(
        &ir,
        Check::Design,
        Severity::Error,
        "names no implicit copy",
    );

    let mut ir = named_cube();
    let mut entries = ir.design.names.entries().clone();
    let entry = entries.values_mut().next().unwrap();
    entry.name = Some(from(json!({"kind": "generated", "value": {
        "kind": "face",
        "feature": key("feature", "a"),
        "role": {"kind": "hole", "value": "bore_wall"}
    }})));
    ir.design.names = entries.into();
    assert_finding(&ir, Check::Design, Severity::Error, "not a hole's face");
}

fn current_pick(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

/// The current geometry's scope publishes one pick.
fn publish(ir: &mut CadIr, target: Picked) {
    ir.design
        .publications
        .push(crate::design::configuration::Publication {
            id: id("publication", "1"),
            scope: id("product_definition", "part"),
            name: cadmpeg_core::text::NonBlankString::new("face").unwrap(),
            target,
        });
}

#[test]
fn an_entity_pick_matches_a_name_of_its_anchored_state() {
    let mut ir = named_cube();
    publish(
        &mut ir,
        current_pick(PickTarget::Entity(generated("face", "a"))),
    );
    assert_clean(&ir);

    let mut ir = named_cube();
    publish(
        &mut ir,
        current_pick(PickTarget::Entity(generated("face", "b"))),
    );
    assert_finding(&ir, Check::Design, Severity::Warning, "matches no entity");

    for name in [
        TopoName::Generated {
            kind: NameKind::Face,
            feature: id("feature", "removed"),
            role: Some(GenerationRole::Hole(HoleFaceRole::BoreWall)),
            generator: None,
            index: None,
        },
        TopoName::Instance {
            feature: id("feature", "removed"),
            instance: InstanceIndex(vec![1]),
            of: Box::new(generated("face", "a")),
        },
    ] {
        let mut ir = named_cube();
        publish(&mut ir, current_pick(PickTarget::Entity(name.clone())));
        let errors: Vec<_> = findings(&ir)
            .into_iter()
            .filter(|finding| finding.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "a lost operand is no Error: {errors:#?}");
        assert_finding(&ir, Check::Design, Severity::Warning, "lost operand");
    }

    let mut ir = named_cube();
    let mut pick = current_pick(PickTarget::Entity(generated("face", "a")));
    pick.resolved = Some(vec![EntityRef::Face(id("face", "elsewhere"))]);
    publish(&mut ir, pick);
    assert_finding(
        &ir,
        Check::Design,
        Severity::Error,
        "not a record of its anchored state",
    );
}

#[test]
fn a_witness_body_is_closed_over_its_own_references() {
    let mut ir = named_cube();
    let mut body = ir.geometry.clone();
    body.shells.clear();
    let mut pick = current_pick(PickTarget::Native { within: None });
    pick.witness = Some(witness_body(body));
    publish(&mut ir, pick);
    let findings = findings(&ir);
    assert!(
        findings
            .iter()
            .any(|finding| finding.message.starts_with("witness body:")
                && finding.severity == Severity::Error),
        "{findings:#?}"
    );
}

fn witness_body(body: Topology) -> Witness {
    Witness {
        point: None,
        direction: None,
        endpoints: None,
        frame: None,
        curves: Vec::new(),
        surface: None,
        body: Some(Box::new(body)),
    }
}

#[test]
fn a_witness_body_meets_the_rules_of_one_topology() {
    let witness_findings = |body: Topology| {
        let mut ir = named_cube();
        let mut pick = current_pick(PickTarget::Native { within: None });
        pick.witness = Some(witness_body(body));
        publish(&mut ir, pick);
        findings(&ir)
            .into_iter()
            .filter(|finding| finding.message.starts_with("witness body:"))
            .collect::<Vec<_>>()
    };
    let ir = named_cube();
    let found = witness_findings(ir.geometry.clone());
    assert!(found.is_empty(), "{found:#?}");

    let mut body = ir.geometry.clone();
    let first = body.coedges[0].clone();
    body.coedges[0].radial_next = body
        .coedges
        .iter()
        .find(|coedge| coedge.edge != first.edge)
        .unwrap()
        .id
        .clone();
    let found = witness_findings(body);
    assert!(
        found
            .iter()
            .any(|finding| finding.severity == Severity::Error
                && finding.message
                    == format!(
                        "witness body: record {}: radial ring crosses edges",
                        first.id.as_str()
                    )),
        "{found:#?}"
    );
}

/// A construction nested `depth` levels, its innermost
/// reference a pick.
fn nested_plane(depth: usize) -> Value {
    let mut inner = construction(vec![input(
        "coincident",
        Some(named(input_anchor(), vec![principal("xy_plane")])),
        None,
    )]);
    for _ in 1..depth {
        let mut outer = construction(vec![json!({
            "role": {"subject": "whole", "relation": {"kind": "parallel"}},
            "reference": {"kind": "built", "value": {"kind": "plane", "inputs": inner["inputs"], "after": [], "resolved_in": {}}}
        })]);
        std::mem::swap(&mut inner, &mut outer);
    }
    inner
}

#[test]
fn the_walk_of_the_deepest_construction_runs_on_a_small_stack() {
    // A nested construction is four levels of JSON: inputs, input, reference
    // and construction, so no document holds a deeper nesting.
    let deepest = crate::bounds::MAX_DOCUMENT_DEPTH / 4;
    let ir = timeline(vec![feature("a", plane_datum(nested_plane(deepest)))]);
    let findings = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || findings(&ir))
        .unwrap()
        .join()
        .unwrap();
    assert!(
        !findings
            .iter()
            .any(|finding| finding.severity == Severity::Error),
        "{findings:#?}"
    );
}

#[test]
fn the_walk_reaches_a_selection_inside_an_operation() {
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let feature = feature(
        "a",
        json!({"kind": "extract", "value": {
            "source": {"at": {"kind": "current"}, "picks": {"kind": "unread"}},
            "native_options": {},
            "source_settings": {}
        }}),
    );
    let mut into = super::walk::Collect::new(&ctx);
    super::walk::Walk::walk(&feature, &mut into).unwrap();
    assert_eq!(into.selections.len(), 1);
    assert!(matches!(into.selections[0].at, Anchor::Current));
    assert!(matches!(
        into.selections[0].picks,
        super::walk::PicksView::Unread
    ));
    drop(into);
    ctx.finish_session().unwrap();
}
