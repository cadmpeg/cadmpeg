// SPDX-License-Identifier: Apache-2.0
//! Circle parameter records of a sketch.

use super::super::{circle_parameter, curve, SketchProjection, CIRCLE_PARAMETER_TYPE};
use super::*;

/// The sketch record ordinal of the square fixture.
const SKETCH_ORDINAL: u32 = 2;
/// The sketch reference that entity records of the square fixture store.
const SKETCH_REFERENCE: u32 = 3;
/// The ordinal of the circle that the fixture adds.
const CIRCLE_ORDINAL: u32 = 11;
/// The ordinal of the circle parameter that the fixture adds.
const PARAMETER_ORDINAL: u32 = 12;

fn located<T>(value: T, type_id: [u8; 16], ordinal: u32) -> Located<T> {
    Located::new(
        value,
        crate::record_identity::RecordTypeId::from_bytes(type_id),
        cadmpeg_ir::identity_key!("segment"),
        ordinal,
    )
}

fn projection(inventory: &SketchInventory) -> SketchProjection {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project(&ctx, inventory, &[]).expect("sketch projection")
}

/// A circle parameter body after a constraint header with the parameter
/// reference `parameter`: `state:u32`, the entity reference, and four u32
/// values.
fn parameter_bytes(parameter: u32, entity: u32) -> Vec<u8> {
    let mut bytes = constraint_header(PARAMETER_ORDINAL, parameter);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(0x8000_0000 | entity).to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes
}

fn parameter(bytes: &[u8]) -> PmDcSketchConstraint {
    let parsed = parse(bytes, |_, source| {
        parse_constraint(SketchConstraintTag::CircleParameter, source, 22)
            .expect("circle parameter")
    });
    located(parsed, CIRCLE_PARAMETER_TYPE, PARAMETER_ORDINAL)
}

/// The square fixture with a full circle of record `CIRCLE_ORDINAL`, in sketch
/// `circle_sketch`, and a sketch that lists the circle and the constraint.
fn inventory_with(circle_sketch: u32, constraint: PmDcSketchConstraint) -> SketchInventory {
    let (mut inventory, _) = closed_square_inventory();
    let circle = parse(
        &circle_bytes(CIRCLE_ORDINAL, circle_sketch, [0, 0], 4, 1),
        |_, source| parse_entity(SketchEntityTag::Circle, source, 22).expect("circle"),
    );
    inventory
        .entities
        .push(located(circle, CIRCLE_TYPE, CIRCLE_ORDINAL));
    inventory.constraints.push(constraint);
    list_members(&mut inventory, &[CIRCLE_ORDINAL + 1, PARAMETER_ORDINAL + 1]);
    inventory
}

fn list_members(inventory: &mut SketchInventory, indices: &[u32]) {
    let mut sketch = PmDcSketchPayload::clone(&inventory.sketches[0]);
    let (marker, metadata, mut references) = sketch.entities.into_parts();
    references.extend(indices.iter().copied().map(PmDcReference::from_packed));
    sketch.entities = PmDcReferenceList::new(marker, metadata, references).expect("member list");
    inventory.sketches[0] = located(sketch, SKETCH_TYPE, SKETCH_ORDINAL);
}

#[test]
fn a_circle_parameter_parses_with_the_radius_layout() {
    let parsed = parameter(&parameter_bytes(0x8000_0020, 12));
    let PmDcSketchConstraintKind::CircleParameter {
        state,
        entity,
        values,
    } = parsed.kind
    else {
        panic!("circle parameter form");
    };
    assert_eq!(state, 0);
    assert_eq!(entity.index(), 12);
    assert_eq!(values, [0; 4]);
    assert_eq!(
        parsed.header.parameter.map(PmDcReference::index),
        Some(0x20)
    );
    assert!(!curve::relation_stays_native(&parsed, |_| true));
}

#[test]
fn a_circle_parameter_of_a_circle_of_the_sketch_keeps_the_sketch_closed() {
    let constraint = parameter(&parameter_bytes(0x8000_0020, CIRCLE_ORDINAL + 1));
    let inventory = inventory_with(SKETCH_REFERENCE, constraint);
    let admitted = circle_parameter::admitted_keys(&inventory);
    assert_eq!(admitted.len(), 1);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.unresolved_sketches, 0);
    assert!(projected.constraints.is_empty());
    assert_eq!(projected.unresolved_constraints, 1);
}

#[test]
fn a_circle_parameter_is_reported_with_its_own_loss() {
    let constraint = parameter(&parameter_bytes(0x8000_0020, CIRCLE_ORDINAL + 1));
    assert!(circle_parameter::loss(&[]).is_none());
    let loss = circle_parameter::loss(&[constraint]).expect("circle parameter loss");
    assert_eq!(loss.code.local_code(), "sketch.circle-parameter-native");
    assert!(loss.message.contains("Retained 1"));
}

#[test]
fn a_circle_parameter_of_a_line_keeps_the_sketch_open() {
    // Ordinal 7 is the first square side.
    let constraint = parameter(&parameter_bytes(0x8000_0020, 8));
    let inventory = inventory_with(SKETCH_REFERENCE, constraint);
    assert!(circle_parameter::admitted_keys(&inventory).is_empty());
    assert!(projection(&inventory).sketches.is_empty());
}

#[test]
fn a_circle_parameter_of_a_circle_of_another_sketch_keeps_the_sketch_open() {
    let constraint = parameter(&parameter_bytes(0x8000_0020, CIRCLE_ORDINAL + 1));
    let inventory = inventory_with(SKETCH_REFERENCE + 1, constraint);
    assert!(circle_parameter::admitted_keys(&inventory).is_empty());
    assert!(projection(&inventory).sketches.is_empty());
}

#[test]
fn a_circle_parameter_without_a_parameter_keeps_the_sketch_open() {
    let constraint = parameter(&parameter_bytes(0, CIRCLE_ORDINAL + 1));
    let inventory = inventory_with(SKETCH_REFERENCE, constraint);
    assert!(circle_parameter::admitted_keys(&inventory).is_empty());
    assert!(projection(&inventory).sketches.is_empty());
}

#[test]
fn a_circle_parameter_of_a_missing_record_keeps_the_sketch_open() {
    let constraint = parameter(&parameter_bytes(0x8000_0020, 90));
    let inventory = inventory_with(SKETCH_REFERENCE, constraint);
    assert!(circle_parameter::admitted_keys(&inventory).is_empty());
    assert!(projection(&inventory).sketches.is_empty());
}

#[test]
fn a_circle_parameter_listed_twice_keeps_the_sketch_open() {
    let constraint = parameter(&parameter_bytes(0x8000_0020, CIRCLE_ORDINAL + 1));
    let mut inventory = inventory_with(SKETCH_REFERENCE, constraint);
    list_members(&mut inventory, &[PARAMETER_ORDINAL + 1]);
    assert!(circle_parameter::admitted_keys(&inventory).is_empty());
    assert!(projection(&inventory).sketches.is_empty());
}

#[test]
fn a_circle_parameter_with_a_constraint_map_keeps_the_sketch_open() {
    let mut bytes = content(PARAMETER_ORDINAL);
    bytes.extend_from_slice(&(-1i32).to_le_bytes());
    bytes.extend_from_slice(&0x8000_000cu32.to_le_bytes());
    // A scalar map of one entry, then an empty reference map.
    bytes.extend_from_slice(&6u16.to_le_bytes());
    bytes.extend_from_slice(&0x3000u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.extend_from_slice(&8u32.to_le_bytes());
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.extend(list(6, &[]));
    bytes.extend_from_slice(&0x8000_0020u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&(0x8000_0000 | (CIRCLE_ORDINAL + 1)).to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    let constraint = parameter(&bytes);
    assert!(!constraint.header.scalar_map.entries().is_empty());
    let inventory = inventory_with(SKETCH_REFERENCE, constraint);
    assert!(circle_parameter::admitted_keys(&inventory).is_empty());
    assert!(projection(&inventory).sketches.is_empty());
}

#[test]
fn a_truncated_circle_parameter_record_becomes_an_issue() {
    let mut bytes = parameter_bytes(0x8000_0020, CIRCLE_ORDINAL + 1);
    let admitted = inventory_with_record(CIRCLE_PARAMETER_TYPE, &bytes, DecodePolicy::service())
        .expect("circle parameter is admitted");
    assert_eq!(admitted.constraints.len(), 1);
    assert!(admitted.issues.is_empty());
    bytes.pop();
    let admitted = inventory_with_record(CIRCLE_PARAMETER_TYPE, &bytes, DecodePolicy::service())
        .expect("truncated circle parameter becomes an issue");
    assert!(admitted.constraints.is_empty());
    assert_eq!(admitted.issues.len(), 1);
}

#[test]
fn a_circle_parameter_listed_by_a_second_sketch_keeps_the_sketch_open() {
    let constraint = parameter(&parameter_bytes(0x8000_0020, CIRCLE_ORDINAL + 1));
    let mut inventory = inventory_with(SKETCH_REFERENCE, constraint);
    let mut second = PmDcSketchPayload::clone(&inventory.sketches[0]);
    let (marker, metadata, _) = second.entities.into_parts();
    second.entities = PmDcReferenceList::new(
        marker,
        metadata,
        vec![PmDcReference::from_packed(PARAMETER_ORDINAL + 1)],
    )
    .expect("member list");
    inventory
        .sketches
        .push(located(second, SKETCH_TYPE, SKETCH_ORDINAL + 100));
    assert!(circle_parameter::admitted_keys(&inventory).is_empty());
}
