// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use crate::design::configuration::FeatureStateOverride;
use crate::design::evaluation::StateBase;
use crate::design::feature::{FeatureDefinition, FeatureOperation, StoredOrigin};
use crate::design::names::NameEntry;
use crate::design::refs::{EntityRef, Evaluated};
use crate::document::CadIr;
use crate::ids::BodyId;
use crate::validate::design::fixture::{
    after, assert_finding, assert_no_design_finding, body, configuration, design_findings,
    document, evaluation, feature, id, set_base, stored,
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

/// The baseline with evaluations after `extrude`, each of its own
/// configuration: `e0` on `Empty`, whose delta has an owner entry for a body
/// that no state holds, `e1` on `e0`, and `e2` and `e3` on `e1`.
fn owner_chain() -> CadIr {
    let mut ir = document();
    for (at, base) in [(0, None), (1, Some(0)), (2, Some(1)), (3, Some(1))] {
        let key = format!("e{at}");
        ir.design.configurations.push(configuration(&key, None));
        let mut content = stored(base.map_or(StateBase::Empty, |base| {
            StateBase::Evaluation(id("evaluation", &format!("e{base}")))
        }));
        if base.is_none() {
            content
                .delta
                .owners
                .insert(id("body", "missing"), Some(id("definition", "part")));
        }
        ir.design.evaluations.push(evaluation(
            &key,
            &key,
            Evaluated::State(after("extrude")),
            Some(content),
        ));
    }
    ir
}

/// The evaluations whose resolved state has the owner entry of `e0`.
fn with_owner_entry(ir: &CadIr) -> BTreeSet<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| {
            finding
                .message
                .contains("owner entry names test:design:body#missing, which is no body")
        })
        .filter_map(|finding| finding.entity)
        .collect()
}

#[test]
fn a_base_delta_reaches_each_state_above_it_in_any_arena_order() {
    let expected: BTreeSet<String> = (0..4)
        .map(|at| format!("test:design:evaluation#e{at}"))
        .collect();
    let mut ir = owner_chain();
    assert_eq!(with_owner_entry(&ir), expected);
    ir.design.evaluations.reverse();
    assert_eq!(with_owner_entry(&ir), expected);
}

mod unproduced;
