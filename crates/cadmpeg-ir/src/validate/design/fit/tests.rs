// SPDX-License-Identifier: Apache-2.0
//! A nested construction's kind fits its relation, a measuring origin is a
//! point, and a `Fold` reference picks straight edges.

use serde_json::{json, Value};

use super::super::fixture::{design_findings, document};
use crate::document::CadIr;
use crate::report::Severity;

fn principal(entity: &str) -> Value {
    json!({"target": {"kind": "principal", "value": {"entity": entity}}})
}

fn picks(picks: &[Value]) -> Value {
    json!({"kind": "picks", "value": {"at": {"kind": "current"}, "picks": {"kind": "named", "value": picks}}})
}

fn built(kind: &str) -> Value {
    json!({"kind": "built", "value": {"kind": kind, "inputs": [{
        "role": {"subject": "whole", "relation": {"kind": "coincident"}},
        "reference": picks(&[principal("origin")])
    }], "after": [], "resolved_in": {}}})
}

fn input(relation: &str, reference: &Value, value: Option<Value>) -> Value {
    let mut input = json!({"role": {"subject": "whole", "relation": {"kind": relation}}, "reference": reference});
    if let Some(value) = value {
        input["value"] = value;
    }
    input
}

/// The fixture with its extrude a datum of `element` built from `inputs`.
fn datum(element: &str, inputs: &[Value]) -> CadIr {
    let mut ir = document();
    let construction = json!({"inputs": inputs, "after": [], "resolved_in": {}});
    let value = if element == "points" {
        json!([construction])
    } else {
        construction
    };
    ir.design.features[0].definition.operation =
        cadmpeg_core::serde_erase::json::from_value(json!({"kind": "datum", "value": {
            "element": {"kind": element, "value": value},
            "native_options": {},
            "source_settings": {}
        }}))
        .unwrap();
    ir
}

fn errors(ir: &CadIr, fragment: &str) -> bool {
    design_findings(ir)
        .iter()
        .any(|finding| finding.severity == Severity::Error && finding.message.contains(fragment))
}

#[test]
fn a_nested_construction_fits_its_relation() {
    let refused = "which does not take one";
    assert!(errors(
        &datum("plane", &[input("parallel", &built("point"), None)]),
        refused
    ));
    assert!(!errors(
        &datum("plane", &[input("parallel", &built("axis"), None)]),
        refused
    ));
    let position = json!({"kind": "position", "value": {
        "x": {"kind": "literal", "value": 1.0},
        "y": {"kind": "literal", "value": 2.0},
        "z": {"kind": "literal", "value": 3.0}
    }});
    assert!(errors(
        &datum(
            "points",
            &[input(
                "coordinates",
                &built("plane"),
                Some(position.clone())
            )]
        ),
        refused
    ));
    assert!(!errors(
        &datum(
            "points",
            &[input("coordinates", &built("frame"), Some(position))]
        ),
        refused
    ));
    let length = json!({"kind": "length", "value": {"kind": "literal", "value": 1.0}});
    assert!(errors(
        &datum(
            "plane",
            &[input("offset", &built("direction"), Some(length))]
        ),
        refused
    ));
    assert!(errors(
        &datum("points", &[input("center", &built("plane"), None)]),
        refused
    ));
    assert!(errors(
        &datum("axis", &[input("principal_normal", &built("axis"), None)]),
        refused
    ));
    assert!(errors(
        &datum("axis", &[input("binormal", &built("axis"), None)]),
        refused
    ));
    assert!(!errors(
        &datum("axis", &[input("principal_normal", &built("curve"), None)]),
        refused
    ));
    assert!(errors(
        &datum("plane", &[input("coincident", &built("direction"), None)]),
        refused
    ));
}

#[test]
fn a_measuring_origin_is_a_point() {
    let ratio = json!({"kind": "ratio", "value": {"kind": "literal", "value": 0.5}});
    let origin = "the measuring origin";
    let on_axis = |second: Value| {
        datum(
            "points",
            &[input(
                "at_parameter",
                &picks(&[principal("x_axis"), second]),
                Some(ratio.clone()),
            )],
        )
    };
    assert!(errors(&on_axis(principal("xy_plane")), origin));
    assert!(!errors(&on_axis(principal("origin")), origin));
    let sketch = json!({"target": {"kind": "sketch", "value": "test:design:sketch#s"}});
    let feature = json!({"target": {"kind": "feature", "value": "test:design:feature#extrude"}});
    let body = json!({"target": {"kind": "entity", "value": {"kind": "generated", "value": {
        "kind": "body", "feature": "test:design:feature#extrude"
    }}}});
    assert!(!errors(&on_axis(sketch), origin));
    assert!(!errors(&on_axis(feature), origin));
    assert!(!errors(&on_axis(body), origin));
}

#[test]
fn a_fold_reference_picks_straight_edges() {
    let fold = |last: Value| {
        datum(
            "frame",
            &[input(
                "fold",
                &picks(&[
                    principal("x_axis"),
                    principal("y_axis"),
                    principal("z_axis"),
                    last,
                ]),
                None,
            )],
        )
    };
    let edge = json!({"target": {"kind": "entity", "value": {"kind": "generated", "value": {
        "kind": "edge", "feature": "test:design:feature#extrude"
    }}}});
    let sketch = json!({"target": {"kind": "sketch", "value": "test:design:sketch#s"}});
    let feature = json!({"target": {"kind": "feature", "value": "test:design:feature#extrude"}});
    let body = json!({"target": {"kind": "entity", "value": {"kind": "generated", "value": {
        "kind": "body", "feature": "test:design:feature#extrude"
    }}}});
    let refused = "other than a straight edge";
    assert!(errors(&fold(principal("xy_plane")), refused));
    assert!(!errors(&fold(edge), refused));
    assert!(!errors(&fold(sketch), refused));
    assert!(!errors(&fold(feature), refused));
    assert!(!errors(&fold(body), refused));
}

#[test]
fn a_pick_fits_its_relation() {
    let refused = "cannot supply what it reads";
    let sketch = json!({"target": {"kind": "sketch", "value": "test:design:sketch#s"}});
    let feature = json!({"target": {"kind": "feature", "value": "test:design:feature#extrude"}});
    let body = json!({"target": {"kind": "entity", "value": {"kind": "generated", "value": {
        "kind": "body", "feature": "test:design:feature#extrude"
    }}}});
    let edge = json!({"target": {"kind": "entity", "value": {"kind": "generated", "value": {
        "kind": "edge", "feature": "test:design:feature#extrude"
    }}}});
    assert!(errors(
        &datum(
            "plane",
            &[input("parallel", &picks(&[principal("origin")]), None)]
        ),
        refused
    ));
    assert!(!errors(
        &datum(
            "plane",
            &[input("parallel", &picks(&[principal("x_axis")]), None)]
        ),
        refused
    ));
    assert!(errors(
        &datum(
            "points",
            &[input("center", &picks(&[principal("xy_plane")]), None)]
        ),
        refused
    ));
    assert!(errors(
        &datum(
            "axis",
            &[input(
                "principal_normal",
                &picks(&[principal("x_axis")]),
                None
            )]
        ),
        refused
    ));
    assert!(!errors(
        &datum("axis", &[input("principal_normal", &picks(&[edge]), None)]),
        refused
    ));
    for target in [sketch, feature, body] {
        assert!(!errors(
            &datum("points", &[input("center", &picks(&[target]), None)]),
            refused
        ));
    }
}
