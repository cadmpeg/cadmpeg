// SPDX-License-Identifier: Apache-2.0
//! Breaches of the name, selection, and construction rules, each beside a
//! document that meets the rule.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use crate::design::evaluation::{EvaluationStatus, StateContent, StateDelta};
use crate::design::names::NameEntry;
use crate::design::refs::EntityRef;
use crate::design::scope::DesignScope;
use crate::document::CadIr;
use crate::report::check::Check;
use crate::report::Severity;

use super::{
    assert_clean, assert_finding, construction, datum_pick, feature, findings, from, generated, id,
    input, input_anchor, key, length, named, named_cube, plane_datum, planes, principal, timeline,
};

#[track_caller]
fn assert_no_finding(ir: &CadIr, text: &str) {
    let findings = findings(ir);
    assert!(
        !findings
            .iter()
            .any(|finding| finding.message.contains(text)),
        "a finding containing {text:?}: {findings:#?}"
    );
}

fn literal(value: f64) -> Value {
    json!({"kind": "literal", "value": value})
}

fn after_feature(name: &str) -> Value {
    json!({"kind": "after", "value": {"kind": "feature", "value": key("feature", name)}})
}

/// A timeline holding the body container `body`, which holds the datum `a`
/// and rolls back at its end, then the feature `b` whose one input picks
/// `pick` at the end of `body`. One evaluation stores the state after `a`:
/// the unit cube, its first face named by `a`.
fn rolled_back(pick: &Value) -> CadIr {
    let a = feature(
        "a",
        json!({"kind": "unresolved", "value": {"family": "extrude"}}),
    );
    let b = feature(
        "b",
        plane_datum(construction(vec![input(
            "coincident",
            Some(named(
                json!({"kind": "pinned", "value": {"kind": "after", "value": {"kind": "container", "value": key("container", "body")}}}),
                vec![json!({"target": {"kind": "entity", "value": pick}})],
            )),
            None,
        )])),
    );
    let mut ir = CadIr::empty();
    ir.design.containers.push(from(json!({
        "kind": "ordered",
        "value": {
            "id": key("container", "body"),
            "kind": "body",
            "members": [{"kind": "feature", "value": key("feature", "a")}],
            "folders": [],
            "rollback": {"kind": "end"}
        }
    })));
    ir.design.containers.push(from(json!({
        "kind": "ordered",
        "value": {
            "id": key("container", "timeline"),
            "kind": "timeline",
            "members": [
                {"kind": "container", "value": key("container", "body")},
                {"kind": "feature", "value": key("feature", "b")}
            ],
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
    });
    ir.design.features = vec![a, b];
    let records = crate::examples::unit_cube().unwrap().geometry;
    let face = EntityRef::Face(records.faces[0].id.clone());
    ir.design
        .evaluations
        .push(crate::design::evaluation::Evaluation {
            id: id("evaluation", "after_a"),
            scope: id("product_definition", "part"),
            configuration: None,
            of: from(json!({"kind": "state", "value": after_feature("a")})),
            status: EvaluationStatus::Current,
            content: Some(StateContent {
                base: from(json!({"kind": "empty"})),
                delta: StateDelta {
                    records,
                    names: BTreeMap::from([(
                        face,
                        Some(NameEntry {
                            name: Some(generated("face", "a")),
                            native: None,
                        }),
                    )]),
                    ..StateDelta::default()
                },
            }),
            native_ref: None,
        });
    ir
}

#[test]
fn a_pick_at_the_end_of_an_ordered_container_resolves_at_its_rollback() {
    let face_of = |feature: &str| json!({"kind": "generated", "value": {"kind": "face", "feature": key("feature", feature)}});
    assert_clean(&rolled_back(&face_of("a")));
    assert_finding(
        &rolled_back(&face_of("ghost")),
        Check::Design,
        Severity::Warning,
        "matches no entity of its anchored state",
    );
}

/// The planes `a` and `b`, `b` with one more input.
fn plane_b_with(extra: Value) -> CadIr {
    let mut features = planes();
    features[1] = feature(
        "b",
        plane_datum(construction(vec![
            input(
                "offset",
                Some(named(input_anchor(), vec![datum_pick("a")])),
                Some(length(10.0)),
            ),
            extra,
        ])),
    );
    timeline(features)
}

#[test]
fn reversed_occurs_on_a_coincident_input_only_where_its_subject_is_a_line() {
    let mut whole = input(
        "coincident",
        Some(named(input_anchor(), vec![principal("xy_plane")])),
        None,
    );
    whole["reversed"] = json!(true);
    let mut features = planes();
    features[1] = feature("b", plane_datum(construction(vec![whole])));
    assert_finding(
        &timeline(features),
        Check::Design,
        Severity::Error,
        "a Coincident input states reversed",
    );

    let mut axis = input(
        "coincident",
        Some(named(input_anchor(), vec![principal("x_axis")])),
        None,
    );
    axis["role"]["subject"] = json!("x_axis");
    axis["reversed"] = json!(true);
    assert_clean(&plane_b_with(axis));
}

#[test]
fn reversed_does_not_occur_on_a_native_relation() {
    let attach = |reversed: Option<bool>| {
        let mut input = json!({"role": {"subject": "whole", "relation": {"kind": "native", "value": "cad:attach"}}});
        if let Some(reversed) = reversed {
            input["reversed"] = json!(reversed);
        }
        timeline(vec![feature("a", plane_datum(construction(vec![input])))])
    };
    assert_clean(&attach(None));
    assert_finding(
        &attach(Some(true)),
        Check::Design,
        Severity::Error,
        "input states reversed",
    );
}

#[test]
fn coordinates_on_an_axis_subject_are_components() {
    let coordinates = |kind: &str| {
        let mut input = input("coordinates", None, None);
        input["role"]["subject"] = json!("x_axis");
        input["value"] = json!({"kind": kind, "value": {"x": literal(1.0), "y": literal(0.0), "z": literal(0.0)}});
        timeline(vec![feature("a", plane_datum(construction(vec![input])))])
    };
    assert_clean(&coordinates("components"));
    assert_finding(
        &coordinates("position"),
        Check::Design,
        Severity::Error,
        "a Coordinates input of a plane construction has a value it does not take",
    );
}

#[test]
fn a_parameter_ratio_is_on_an_at_parameter_input_that_picks_one_edge() {
    let at_parameter = |reference: Value| {
        plane_b_with(input(
            "at_parameter",
            Some(reference),
            Some(json!({"kind": "parameter_ratio", "value": literal(0.5)})),
        ))
    };
    let edge = json!({"target": {"kind": "entity", "value": generated("edge", "a")}});
    assert_clean(&at_parameter(named(input_anchor(), vec![edge])));
    let rule = json!({"at": input_anchor(), "picks": {"kind": "rule", "value": {
        "terms": [{"rule": {"kind": "of", "value": {"kind": "edge"}}, "seeds": [datum_pick("a")]}],
        "excluded": []
    }}});
    assert_finding(
        &at_parameter(rule),
        Check::Design,
        Severity::Error,
        "a ParameterRatio value is not on an AtParameter input",
    );
}

/// The axis datum `a` on the X axis, then the plane datum `b` whose one
/// input is `AtParameter` at `ParameterRatio` 0.5 of `pick`.
fn ratio_on(pick: Value) -> CadIr {
    let mut axis = plane_datum(construction(vec![input(
        "coincident",
        Some(named(input_anchor(), vec![principal("x_axis")])),
        None,
    )]));
    axis["value"]["element"]["kind"] = json!("axis");
    timeline(vec![
        feature("a", axis),
        feature(
            "b",
            plane_datum(construction(vec![input(
                "at_parameter",
                Some(named(input_anchor(), vec![pick])),
                Some(json!({"kind": "parameter_ratio", "value": literal(0.5)})),
            )])),
        ),
    ])
}

/// [`ratio_on`] picking the entity `e` of a sketch of `a`, whose curve is
/// `curve`.
fn ratio_on_sketch_entity(curve: &Value) -> CadIr {
    let mut ir =
        ratio_on(json!({"target": {"kind": "sketch_entity", "value": key("sketch_entity", "e")}}));
    ir.design.sketches.push(from(json!({
        "id": key("sketch", "s"),
        "owner": key("feature", "a"),
        "plane": {"kind": "block"},
        "solve": {"status": "current"},
        "native_options": {},
        "source_settings": {}
    })));
    ir.design.sketch_entities.push(from(json!({
        "id": key("sketch_entity", "e"),
        "sketch": key("sketch", "s"),
        "definition": {"kind": "free", "value": curve},
        "roles": [],
        "native_options": {},
        "source_settings": {}
    })));
    ir
}

const RATIO_REFUSED: &str =
    "a ParameterRatio value is not on an AtParameter input that picks one edge or curve";

#[test]
fn a_parameter_ratio_does_not_pick_an_axis_datum() {
    assert_finding(
        &ratio_on(datum_pick("a")),
        Check::Design,
        Severity::Error,
        RATIO_REFUSED,
    );
}

#[test]
fn a_parameter_ratio_does_not_pick_a_sketch_point() {
    assert_finding(
        &ratio_on_sketch_entity(&json!({"kind": "point", "value": {"u": 0.0, "v": 0.0}})),
        Check::Design,
        Severity::Error,
        RATIO_REFUSED,
    );
}

#[test]
fn a_parameter_ratio_picks_a_sketch_curve_or_an_edge() {
    assert_clean(&ratio_on_sketch_entity(&json!({"kind": "line", "value": {
        "start": {"u": 0.0, "v": 0.0},
        "end": {"u": 1.0, "v": 0.0}
    }})));
    assert_clean(&ratio_on_sketch_entity(&json!({"kind": "arc", "value": {
        "center": {"u": 0.0, "v": 0.0},
        "radius": 1.0,
        "start": 0.0,
        "end": 1.0
    }})));
    assert_clean(&ratio_on(
        json!({"target": {"kind": "entity", "value": generated("edge", "a")}}),
    ));
}

/// The named unit cube with a linear pattern `p` of `run` after `b`, the
/// current state after `p`, and its first face named as instance `instance`
/// of `a`'s face.
fn patterned(run: &Value, instance: &[i32]) -> CadIr {
    patterned_in(
        &json!({"kind": "linear", "value": {"first": {
            "direction": {"inputs": [], "after": [], "resolved_in": {}, "resolved": {"x": 1.0, "y": 0.0, "z": 0.0}},
            "run": run
        }}}),
        instance,
    )
}

/// As [`patterned`], with the pattern laid out along an unread path.
fn path_patterned(run: &Value, instance: &[i32]) -> CadIr {
    patterned_in(
        &json!({"kind": "path", "value": {
            "path": {"at": input_anchor(), "picks": {"kind": "unread"}},
            "run": run
        }}),
        instance,
    )
}

fn patterned_in(layout: &Value, instance: &[i32]) -> CadIr {
    let mut ir = named_cube();
    ir.design.features.push(feature(
        "p",
        json!({"kind": "pattern", "value": {
            "seeds": {"kind": "features", "features": {"at": input_anchor(), "picks": {"kind": "unread"}}},
            "layout": layout,
            "instances": {"kind": "implicit", "value": []},
            "native_options": {},
            "source_settings": {}
        }}),
    ));
    let crate::design::container::Container::Ordered { members, .. } = &mut ir.design.containers[0]
    else {
        unreachable!()
    };
    members.push(from(
        json!({"kind": "feature", "value": key("feature", "p")}),
    ));
    ir.design.scopes[0].current[0].at = from(after_feature("p"));
    let mut entries = ir.design.names.entries().clone();
    let entry = entries.values_mut().next().unwrap();
    entry.name = Some(from(json!({"kind": "instance", "value": {
        "feature": key("feature", "p"),
        "instance": instance,
        "of": generated("face", "a")
    }})));
    ir.design.names = entries.into();
    ir
}

#[test]
fn a_linear_instance_index_lies_in_its_run() {
    let run = |sides: Option<&str>, seed: Option<u32>| {
        let mut run = json!({
            "spacing": {"kind": "pitch", "value": {"count": {"kind": "literal", "value": 2}, "pitch": literal(1.0)}},
            "increments": []
        });
        if let Some(sides) = sides {
            run["sides"] = json!({"kind": sides});
        }
        if let Some(seed) = seed {
            run["seed"] = json!({"kind": "literal", "value": seed});
        }
        run
    };
    let refused = "names no implicit copy";
    assert_clean(&patterned(&run(None, None), &[1]));
    assert_finding(
        &patterned(&run(None, None), &[99]),
        Check::Design,
        Severity::Error,
        refused,
    );
    assert_no_finding(&patterned(&run(None, None), &[-1]), refused);
    assert_no_finding(&patterned(&run(None, None), &[-5]), refused);
    assert_finding(
        &patterned(&run(None, Some(3)), &[2]),
        Check::Design,
        Severity::Error,
        refused,
    );
    let mut parameter_seed = run(None, None);
    parameter_seed["seed"] = json!({"kind": "parameter", "value": "test:design:parameter#n"});
    assert_finding(
        &patterned(&parameter_seed, &[2]),
        Check::Design,
        Severity::Error,
        refused,
    );
    assert_finding(
        &patterned(&run(Some("one"), None), &[-1]),
        Check::Design,
        Severity::Error,
        refused,
    );
    assert_no_finding(&patterned(&run(Some("one"), Some(1)), &[-1]), refused);
    assert_finding(
        &patterned(&run(Some("one"), Some(1)), &[1]),
        Check::Design,
        Severity::Error,
        refused,
    );
}

/// Interim, gap 20261119-ERR1-C-H6-2: an unread side of a `Path` run may be
/// `Against`, which reaches the seed's step count, so its upper bound is the
/// greater of `last` and a literal seed, and there is none for a parameter
/// seed.
#[test]
fn a_path_run_with_unread_sides_reaches_its_seed() {
    let run = |seed: Value| {
        let mut run = json!({
            "spacing": {"kind": "pitch", "value": {"count": {"kind": "literal", "value": 2}, "pitch": literal(1.0)}},
            "increments": []
        });
        if !seed.is_null() {
            run["seed"] = seed;
        }
        run
    };
    let refused = "names no implicit copy";
    let parameter = run(json!({"kind": "parameter", "value": "test:design:parameter#n"}));
    assert_no_finding(&path_patterned(&parameter, &[2]), refused);
    assert_no_finding(&path_patterned(&parameter, &[-5]), refused);
    let seeded = run(json!({"kind": "literal", "value": 3}));
    assert_no_finding(&path_patterned(&seeded, &[3]), refused);
    assert_no_finding(&path_patterned(&seeded, &[-5]), refused);
    assert_finding(
        &path_patterned(&seeded, &[4]),
        Check::Design,
        Severity::Error,
        refused,
    );
    let unseeded = run(Value::Null);
    assert_no_finding(&path_patterned(&unseeded, &[1]), refused);
    assert_finding(
        &path_patterned(&unseeded, &[2]),
        Check::Design,
        Severity::Error,
        refused,
    );
}
