// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::design::evaluation::StateBase;
use crate::design::names::NameEntry;
use crate::design::refs::EntityRef;
use crate::validate::design::fixture::{assert_finding, body, document, id};

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
