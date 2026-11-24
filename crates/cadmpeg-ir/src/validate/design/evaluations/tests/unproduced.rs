// SPDX-License-Identifier: Apache-2.0

use crate::design::selection::{OccurrencePath, PathStep, StepRef};
use crate::ids::{OccurrenceId, ProductDefinitionId};
use crate::products::{ExternalDocument, InstanceBodies, Occurrence, ProductDefinition, Storage};
use crate::validate::design::fixture::{
    assert_finding, assert_no_design_finding, body, document, id,
};
use crate::CadIr;

fn unproduced() -> CadIr {
    let mut ir = document();
    let body = body(&ir);
    ir.design.scopes[0].current[0].bodies.clear();
    ir.design.scopes[0].unproduced.insert(body);
    ir
}

#[test]
fn a_scoped_body_can_be_unproduced() {
    assert_no_design_finding(&unproduced());
}

#[test]
fn an_unproduced_entry_is_a_body_of_current_geometry() {
    let mut ir = unproduced();
    ir.design.scopes[0].unproduced.insert(id("body", "missing"));
    assert_finding(&ir, "must be a body of the current geometry");
    let mut ir = unproduced();
    ir.design.scopes[0]
        .unproduced
        .insert(ir.geometry.faces[0].id.as_str().try_into().unwrap());
    assert_finding(&ir, "must be a body of the current geometry");
}

#[test]
fn an_unproduced_body_has_a_spanned_owner() {
    let mut ir = unproduced();
    ir.model.product_definitions[0].bodies.clear();
    assert_finding(&ir, "must be owned by a definition the scope spans");
    let body = body(&ir);
    ir.model.product_definitions.push(ProductDefinition {
        bodies: vec![body],
        ..ProductDefinition::new(id("definition", "other"), Storage::Embedded)
    });
    assert_finding(&ir, "must be owned by a definition the scope spans");
}

#[test]
fn current_and_unproduced_membership_is_exclusive() {
    let mut ir = unproduced();
    let body = body(&ir);
    ir.design.scopes[0].current[0].bodies.push(body);
    assert_finding(&ir, "belongs to 2 current states or unproduced sets");
    let mut ir = unproduced();
    let mut other = ir.design.scopes[0].clone();
    other.definition = id("definition", "other");
    other.roots.clear();
    other.current.clear();
    other.active_configuration = None;
    ir.model.product_definitions.push(ProductDefinition::new(
        other.definition.clone(),
        Storage::Embedded,
    ));
    ir.design.scopes.push(other);
    assert_finding(&ir, "belongs to 2 current states or unproduced sets");
    ir.design.scopes[1].unproduced.clear();
    ir.design.scopes[1].current = ir.design.scopes[0].current.clone();
    let body = crate::validate::design::fixture::body(&ir);
    ir.design.scopes[1].current[0].bodies.push(body);
    assert_finding(&ir, "belongs to 2 current states or unproduced sets");
}

fn place_body(ir: &mut CadIr, storage: Storage) {
    let body = body(ir);
    ir.model.product_definitions[0].bodies.clear();
    let prototype: ProductDefinitionId = id("definition", "child");
    let occurrence: OccurrenceId = id("occurrence", "child");
    ir.model.product_definitions[0]
        .occurrences
        .push(occurrence.clone());
    ir.model.product_definitions.push(ProductDefinition {
        bodies: vec![body],
        ..ProductDefinition::new(prototype.clone(), storage)
    });
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence, prototype, None));
}

#[test]
fn membership_spans_embedded_and_file_definitions_and_stops_at_scope_boundaries() {
    let mut ir = unproduced();
    place_body(&mut ir, Storage::Embedded);
    assert_no_design_finding(&ir);
    ir.design.scopes[0].unproduced.clear();
    assert_finding(
        &ir,
        "belongs to no current state or unproduced set of that scope",
    );
    let child_body = body(&ir);
    ir.design.scopes[0].unproduced.insert(child_body);
    let mut child_scope = ir.design.scopes[0].clone();
    child_scope.definition = id("definition", "child");
    child_scope.roots.clear();
    child_scope.current.clear();
    child_scope.unproduced.clear();
    child_scope.active_configuration = None;
    ir.design.scopes.push(child_scope);
    assert_finding(&ir, "must be owned by a definition the scope spans");

    let mut ir = unproduced();
    place_body(
        &mut ir,
        Storage::File {
            document: ExternalDocument::document_id("child"),
            source: None,
        },
    );
    assert_finding(&ir, "must be owned by a definition the scope spans");
    ir.design.features[0].component = Some(id("definition", "part"));
    ir.design.features[1].component = Some(id("definition", "child"));
    assert_no_design_finding(&ir);
}

#[test]
fn membership_covers_instance_bodies() {
    let mut ir = unproduced();
    let body = body(&ir);
    ir.model.product_definitions[0].bodies.clear();
    let occurrence: OccurrenceId = id("occurrence", "child");
    let prototype: ProductDefinitionId = id("definition", "child");
    ir.model.product_definitions[0]
        .occurrences
        .push(occurrence.clone());
    ir.model
        .occurrences
        .push(Occurrence::new(occurrence.clone(), prototype.clone(), None));
    ir.model
        .product_definitions
        .push(ProductDefinition::new(prototype, Storage::Embedded));
    ir.model.product_definitions[0]
        .instance_bodies
        .push(InstanceBodies {
            occurrence: OccurrencePath(vec![PathStep {
                occurrence: StepRef::Occurrence(occurrence),
                configuration: None,
            }]),
            bodies: vec![body.clone()],
        });
    assert_no_design_finding(&ir);
    ir.design.scopes[0].unproduced.clear();
    assert_finding(
        &ir,
        "belongs to no current state or unproduced set of that scope",
    );
    ir.design.scopes[0].current[0].bodies.push(body);
    assert_no_design_finding(&ir);
}

#[test]
fn a_file_scope_spans_its_unplaced_embedded_definitions() {
    let mut ir = unproduced();
    let body = body(&ir);
    ir.model.product_definitions[0].storage = Storage::File {
        document: ExternalDocument::document_id("part"),
        source: None,
    };
    ir.model.product_definitions[0].bodies.clear();
    ir.model.product_definitions.push(ProductDefinition {
        bodies: vec![body],
        ..ProductDefinition::new(id("definition", "unplaced"), Storage::Embedded)
    });
    assert_no_design_finding(&ir);
    ir.design.scopes[0].unproduced.clear();
    assert_finding(
        &ir,
        "belongs to no current state or unproduced set of that scope",
    );
}
