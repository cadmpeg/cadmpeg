// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::FiniteVector3;
use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::products::{
    Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference,
};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;

use super::class;
use crate::compare::PlacementClass;

fn definition(ir: &CadIr) -> ProductDefinition {
    ProductDefinition {
        id: ProductDefinitionId::mint("test:model:product#1").expect("id"),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: BTreeMap::new(),
        bodies: ir.model.bodies.iter().map(|body| body.id.clone()).collect(),
        native_ref: None,
    }
}

fn occurrence(n: u32, transform: Transform) -> Occurrence {
    Occurrence {
        id: OccurrenceId::mint(format!("test:model:occurrence#{n}")).expect("id"),
        prototype: PrototypeReference::Local {
            definition: ProductDefinitionId::mint("test:model:product#1").expect("id"),
        },
        parent: OccurrenceParent::Root {},
        ordinal: n,
        transform: Some(transform),
        linked_prototype: None,
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

fn cube_with(occurrences: Vec<Occurrence>) -> CadIr {
    let mut cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    let definition = definition(&cube);
    cube.model.product_definitions.push(definition);
    cube.model.occurrences = occurrences;
    cube
}

#[test]
fn a_document_without_occurrences_is_trivial() {
    let cube = cadmpeg_ir::examples::unit_cube().expect("fixture");
    assert_eq!(class(&cube), PlacementClass::Trivial);
}

#[test]
fn one_identity_root_occurrence_is_trivial() {
    let cube = cube_with(vec![occurrence(0, Transform::identity())]);
    assert_eq!(class(&cube), PlacementClass::Trivial);
}

#[test]
fn a_translated_occurrence_places_its_bodies() {
    let shift = FiniteVector3::new(Vector3::new(15.0, 10.0, 0.0)).expect("finite");
    let cube = cube_with(vec![occurrence(
        0,
        Transform::identity().with_translation(shift),
    )]);
    assert_eq!(class(&cube), PlacementClass::Occurrences);
}

#[test]
fn a_definition_instanced_twice_places_its_bodies() {
    let cube = cube_with(vec![
        occurrence(0, Transform::identity()),
        occurrence(1, Transform::identity()),
    ]);
    assert_eq!(class(&cube), PlacementClass::Occurrences);
}
