// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;

use cadmpeg_core::json;

use crate::design::evaluation::StateBase;
use crate::design::names::{NameEntry, TopoName};
use crate::design::refs::EntityRef;
use crate::validate::design::fixture::{
    assert_finding, assert_no_design_finding, body, document, id,
};

#[test]
fn an_evaluation_cannot_state_an_unproduced_body() {
    let mut ir = document();
    let body_id = body(&ir);
    let body = ir.geometry.bodies[0].clone();
    ir.design.scopes[0].current[0].bodies.clear();
    ir.design.scopes[0].unproduced.insert(body_id);
    assert_no_design_finding(&ir);
    let content = ir.design.evaluations[0].content.as_mut().unwrap();
    content.base = StateBase::Empty;
    content.delta.records.bodies.push(body);
    assert_finding(&ir, "evaluation states unproduced body");
}

#[test]
fn names_on_an_unproduced_extent_are_native() {
    let mut ir = document();
    let body = body(&ir);
    ir.design.scopes[0].current[0].bodies.clear();
    ir.design.scopes[0].unproduced.insert(body);
    let native: NameEntry = json::from_value(json!({
        "native": { "codec": "test", "value": {"kind": "text", "value": "face"} }
    }))
    .unwrap();
    let entity = EntityRef::Face(ir.geometry.faces[0].id.clone());
    ir.design.names = BTreeMap::from([(entity.clone(), native.clone())]).into();
    assert_no_design_finding(&ir);
    ir.design.names = BTreeMap::from([(
        entity,
        NameEntry {
            name: Some(TopoName::Root {
                container: id("container", "timeline"),
            }),
            ..native
        },
    )])
    .into();
    assert_finding(
        &ir,
        "an entity of an unproduced body must have no structured name",
    );
    ir.design.evaluations.clear();
    assert_finding(
        &ir,
        "an entity of an unproduced body must have no structured name",
    );
}

#[test]
fn an_unproduced_extent_is_excluded_from_a_current_state_base() {
    let mut ir = document();
    let body = body(&ir);
    ir.design.scopes[0].current[0].bodies.clear();
    ir.design.scopes[0].unproduced.insert(body.clone());
    let content = ir.design.evaluations[0].content.as_mut().unwrap();
    content
        .delta
        .owners
        .insert(body, Some(id("definition", "part")));
    assert_finding(&ir, "which is no body of the resolved state");
}
