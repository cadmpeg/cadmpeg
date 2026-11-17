// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::design::configuration::FeatureStateOverride;
use crate::design::evaluation::StateBase;
use crate::design::feature::{FeatureDefinition, FeatureOperation, StoredOrigin};
use crate::design::names::NameEntry;
use crate::design::refs::EntityRef;
use crate::document::CadIr;
use crate::ids::BodyId;
use crate::validate::design::fixture::{
    after, assert_finding, assert_no_design_finding, body, document, feature, id, set_base,
};

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
fn a_resolved_state_is_closed_over_ids_in_no_topology() {
    let mut ir = document();
    let mut vertex = ir.geometry.vertices[0].clone();
    vertex.point = id("point", "ghost");
    let content = ir.design.evaluations[0].content.as_mut().unwrap();
    content.base = StateBase::Empty;
    content.delta.records.vertices.push(vertex);
    assert_finding(
        &ir,
        "references test:design:point#ghost, which the state does not hold",
    );

    let mut ir = document();
    let mut vertex = ir.geometry.vertices[0].clone();
    vertex.native_ref = Some("absent:vertex#0".to_owned());
    let point = ir
        .geometry
        .points
        .iter()
        .find(|point| point.id == vertex.point)
        .unwrap()
        .clone();
    let content = ir.design.evaluations[0].content.as_mut().unwrap();
    content.base = StateBase::Empty;
    content.delta.records.vertices.push(vertex);
    content.delta.records.points.push(point);
    assert_no_design_finding(&ir);
}

/// The baseline with configuration `configuration` overriding feature
/// `fillet`'s definition with a stored definition of `bodies`.
fn stored_override(configuration: usize, bodies: Vec<BodyId>) -> CadIr {
    let mut ir = document();
    ir.design.configurations[configuration]
        .overrides
        .features
        .insert(
            feature("fillet"),
            FeatureStateOverride {
                suppressed: None,
                definition: Some(FeatureDefinition {
                    operation: FeatureOperation::Stored {
                        bodies: Some(bodies),
                        tessellations: Vec::new(),
                        origin: StoredOrigin::Base {},
                    },
                    refine: None,
                    fuzzy_tolerance: None,
                }),
            },
        );
    ir
}

#[test]
fn a_stored_definition_override_names_bodies_of_the_state_after_it() {
    let mut ir = stored_override(1, vec![id("body", "missing")]);
    set_base(&mut ir, 1, StateBase::Empty);
    assert_finding(
        &ir,
        "names test:design:body#missing, which is no body of the state after it",
    );
    let ir = stored_override(0, vec![id("body", "missing")]);
    assert_finding(
        &ir,
        "names test:design:body#missing, which is no body of the current state after it",
    );

    let body = body(&document());
    let mut ir = stored_override(1, vec![body.clone()]);
    set_base(&mut ir, 1, StateBase::Current(after("fillet")));
    assert_no_design_finding(&ir);
    let ir = stored_override(0, vec![body]);
    assert_no_design_finding(&ir);
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
