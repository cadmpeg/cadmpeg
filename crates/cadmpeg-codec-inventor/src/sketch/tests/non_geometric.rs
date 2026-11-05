// SPDX-License-Identifier: Apache-2.0

use super::super::{
    curve, SketchProjection, BLOCK_GROUP_TYPE, BLOCK_MEMBER_TYPE, POLYGON_CONTROL_TYPE,
    POLYGON_RELATION_TYPE, POLYGON_TYPE,
};
use super::*;
use cadmpeg_ir::sketches::SketchGeometryDefinition;

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

fn add_members(inventory: &mut SketchInventory, indices: &[u32]) {
    let mut sketch = PmDcSketchPayload::clone(&inventory.sketches[0]);
    let (marker, metadata, mut references) = sketch.entities.into_parts();
    references.extend(indices.iter().copied().map(PmDcReference::from_packed));
    sketch.entities = PmDcReferenceList::new(marker, metadata, references).expect("member list");
    inventory.sketches[0] = located(sketch, SKETCH_TYPE, 2);
}

#[test]
fn polygon_controls_keep_edge_lines_and_native_relation_losses() {
    let (mut inventory, _) = closed_square_inventory();
    for entity in &mut inventory.entities {
        crate::test_support::sketch_curves::edit_entity(entity, |payload| {
            match &mut payload.kind {
                PmDcSketchEntityKind::Point { position, .. } => {
                    position[1] = real(position[1].get() * 2.0);
                }
                PmDcSketchEntityKind::Line {
                    origin, direction, ..
                } => {
                    origin[1] = real(origin[1].get() * 2.0);
                    direction[1] = real(direction[1].get() * 2.0);
                }
                _ => panic!("square point or line"),
            }
        });
    }
    let mut polygon = entity_prefix(11, 3, 0);
    polygon.extend_from_slice(&8_u32.to_le_bytes());
    polygon.extend_from_slice(&0_u32.to_le_bytes());
    let polygon = parse(&polygon, |_, source| {
        parse_entity(SketchEntityTag::Polygon, source, 22).expect("polygon control")
    });
    inventory.entities.push(located(polygon, POLYGON_TYPE, 11));
    let mut relation = constraint_header(12, 0);
    for value in [8_u32, 9, 8, 12, 12, 0, 0] {
        relation.extend_from_slice(&value.to_le_bytes());
    }
    let relation = parse(&relation, |_, source| {
        parse_constraint(SketchConstraintTag::PolygonRelation, source, 22)
            .expect("polygon relation")
    });
    inventory
        .constraints
        .push(located(relation, POLYGON_RELATION_TYPE, 12));
    add_members(&mut inventory, &[12, 13]);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.entities.len(), 8);
    assert!(projected.entities.iter().all(|entity| !matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Native { .. }
    )));
    assert_eq!(
        projected
            .entities
            .iter()
            .filter(|entity| matches!(
                entity.geometry.definition(),
                SketchGeometryDefinition::Line { .. }
            ))
            .count(),
        4
    );
    assert_eq!(projected.sketches[0].profiles[0].len(), 4);
    assert_eq!(projected.unresolved_entities, 1);
    assert_eq!(projected.unresolved_constraints, 1);
    assert!(projected.constraints.is_empty());
    let graph_loss = super::super::graph_loss(
        projected.unresolved_sketches,
        projected.unresolved_entities,
        projected.unresolved_constraints,
    )
    .expect("retained graph loss");
    assert_eq!(graph_loss.code.local_code(), "sketch.graph-open");
    assert!(graph_loss.message.contains(
        "0 Inventor sketch record(s), 1 sketch-entity record(s), and 1 sketch-constraint record(s)"
    ));
    let losses = curve::native_losses(&inventory.entities);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "sketch.curve-native");
    assert!(losses[0].message.contains("Retained 1"));
}

/// A polygon control record after the constraint header: `value:u32`, the
/// reference entity, the vertex reference twice, the vertex position as two
/// f64 values, and three flag bytes.
fn polygon_control_bytes(ordinal: u32, references: [u32; 3], flags: [u8; 3]) -> Vec<u8> {
    let mut bytes = constraint_header(ordinal, 0);
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    for reference in references {
        bytes.extend_from_slice(&(0x8000_0000 | reference).to_le_bytes());
    }
    bytes.extend_from_slice(&2.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&flags);
    bytes
}

#[test]
fn a_polygon_control_relation_parses_its_operands_and_stays_native() {
    let bytes = polygon_control_bytes(11, [4, 5, 5], [0, 0, 1]);
    let parsed = parse(&bytes, |_, source| {
        parse_constraint(SketchConstraintTag::PolygonControl, source, 22).expect("polygon control")
    });
    let PmDcSketchConstraintKind::PolygonControl {
        value,
        reference,
        vertex,
        position,
        flags,
    } = parsed.kind
    else {
        panic!("polygon control form");
    };
    assert_eq!(value, 7);
    assert_eq!(reference.index(), 4);
    assert_eq!(vertex.map(crate::pmdc::PmDcReference::index), [5, 5]);
    assert_eq!(position.map(FiniteReal::get), [2.0, 0.0]);
    assert_eq!(flags, [0, 0, 1]);
    assert!(parsed.header.parameter.is_some());
    assert!(curve::relation_stays_native(
        &located(parsed, POLYGON_CONTROL_TYPE, 11),
        |_| false
    ));
}

#[test]
fn a_polygon_control_relation_keeps_the_sketch_closed_and_stays_a_native_constraint() {
    let (mut inventory, _) = closed_square_inventory();
    let control = parse(
        &polygon_control_bytes(11, [4, 5, 5], [0, 0, 1]),
        |_, source| {
            parse_constraint(SketchConstraintTag::PolygonControl, source, 22)
                .expect("polygon control")
        },
    );
    inventory
        .constraints
        .push(located(control, POLYGON_CONTROL_TYPE, 11));
    add_members(&mut inventory, &[12]);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.entities.len(), 8);
    assert!(projected.constraints.is_empty());
    assert_eq!(projected.unresolved_sketches, 0);
    assert_eq!(projected.unresolved_constraints, 1);
}

#[test]
fn a_polygon_control_member_without_a_record_keeps_the_sketch_unresolved() {
    let (mut inventory, _) = closed_square_inventory();
    add_members(&mut inventory, &[12]);
    let projected = projection(&inventory);
    assert!(projected.sketches.is_empty());
    assert!(projected.entities.is_empty());
    assert_eq!(projected.unresolved_sketches, 1);
}

#[test]
fn a_polygon_control_member_listed_twice_keeps_the_sketch_unresolved() {
    let (mut inventory, _) = closed_square_inventory();
    let control = parse(
        &polygon_control_bytes(11, [4, 5, 5], [0, 0, 1]),
        |_, source| {
            parse_constraint(SketchConstraintTag::PolygonControl, source, 22)
                .expect("polygon control")
        },
    );
    inventory
        .constraints
        .push(located(control, POLYGON_CONTROL_TYPE, 11));
    add_members(&mut inventory, &[12, 12]);
    assert!(projection(&inventory).sketches.is_empty());
}

#[test]
fn a_truncated_polygon_control_record_becomes_an_issue() {
    let mut bytes = polygon_control_bytes(11, [4, 5, 5], [0, 0, 1]);
    let admitted = inventory_with_record(POLYGON_CONTROL_TYPE, &bytes, DecodePolicy::service())
        .expect("polygon control is admitted");
    assert_eq!(admitted.constraints.len(), 1);
    assert!(admitted.issues.is_empty());
    bytes.pop();
    let admitted = inventory_with_record(POLYGON_CONTROL_TYPE, &bytes, DecodePolicy::service())
        .expect("truncated polygon control becomes an issue");
    assert!(admitted.constraints.is_empty());
    assert_eq!(admitted.issues.len(), 1);
}

fn block_inventory() -> SketchInventory {
    let (mut inventory, _) = closed_square_inventory();
    inventory
        .entities
        .push(block_instance(11, [0x0111, 0x00ca], &[0.0, 0.0]));
    crate::test_support::sketch_curves::edit_entity(
        inventory.entities.last_mut().expect("instance"),
        |payload| {
            let PmDcSketchEntityKind::BlockInstance { group, .. } = &mut payload.kind else {
                panic!("block instance");
            };
            *group = PmDcReference::from_packed(14);
        },
    );
    let mut member = entity_prefix(12, 3, 0);
    member.extend(list(2, &[12]));
    member.extend_from_slice(&4_u32.to_le_bytes());
    let member = parse(&member, |_, source| {
        parse_entity(SketchEntityTag::BlockMember, source, 22).expect("block member")
    });
    inventory
        .entities
        .push(located(member, BLOCK_MEMBER_TYPE, 12));
    let mut group = constraint_header(13, 0);
    group.truncate(group.len() - 4);
    group.extend(list(2, &[12, 13]));
    let group = parse(&group, |_, source| {
        parse_constraint(SketchConstraintTag::BlockGroup, source, 22).expect("block group")
    });
    inventory
        .constraints
        .push(located(group, BLOCK_GROUP_TYPE, 13));
    inventory.entities.extend(block_definition());
    add_members(&mut inventory, &[12, 13, 14]);
    inventory
}

#[test]
fn block_members_and_groups_keep_placed_curves_without_native_entities() {
    let inventory = block_inventory();
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.entities.len(), 10);
    assert!(projected.entities.iter().all(|entity| !matches!(
        entity.geometry.definition(),
        SketchGeometryDefinition::Native { .. }
    )));
    assert!(projected
        .entities
        .iter()
        .any(|entity| entity.id().as_str() == "inventor:design:sketch-entity#segment-11-21"));
    assert_eq!(projected.unresolved_constraints, 1);
    assert!(projected.constraints.is_empty());
    assert_eq!(projected.unresolved_entities, 7);
    let losses = curve::native_losses(&inventory.entities);
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code.local_code() == "sketch.curve-native"
            && loss.message.contains("Retained 1")));
}

#[test]
fn unknown_grammar_and_duplicate_members_keep_the_sketch_unresolved() {
    for indices in [&[12][..], &[4][..], &[0][..]] {
        let (mut inventory, _) = closed_square_inventory();
        add_members(&mut inventory, indices);
        let projected = projection(&inventory);
        assert!(projected.sketches.is_empty());
        assert!(projected.entities.is_empty());
        assert_eq!(projected.unresolved_sketches, 1);
    }
}

#[test]
fn graph_losses_count_each_native_category() {
    assert!(super::super::graph_loss(0, 0, 0).is_none());
    for (sketches, entities, constraints) in [(1, 0, 0), (0, 1, 0), (0, 0, 1)] {
        let loss = super::super::graph_loss(sketches, entities, constraints)
            .expect("native category loss");
        assert_eq!(loss.code.local_code(), "sketch.graph-open");
        assert!(loss.message.contains(&format!("{sketches} Inventor sketch record(s), {entities} sketch-entity record(s), and {constraints} sketch-constraint record(s)")));
    }
}

#[test]
fn decoded_relations_to_block_members_stay_native_without_removing_the_sketch() {
    use super::super::{DISTANCE_TYPE, PARALLEL_TYPE, PERPENDICULAR_TYPE, TANGENT_TYPE};
    for (tag, record_type) in [
        (SketchConstraintTag::Coincident, COINCIDENT_TYPE),
        (SketchConstraintTag::Parallel, PARALLEL_TYPE),
        (SketchConstraintTag::Perpendicular, PERPENDICULAR_TYPE),
        (SketchConstraintTag::Tangent, TANGENT_TYPE),
        (SketchConstraintTag::Horizontal, HORIZONTAL_TYPE),
        (SketchConstraintTag::Distance, DISTANCE_TYPE),
        (SketchConstraintTag::Midpoint, MIDPOINT_TYPE),
    ] {
        let mut inventory = block_inventory();
        let mut bytes = constraint_header(14, 0);
        if matches!(tag, SketchConstraintTag::Distance) {
            bytes.extend_from_slice(&0_u32.to_le_bytes());
        }
        bytes.extend_from_slice(&13_u32.to_le_bytes());
        if matches!(tag, SketchConstraintTag::Horizontal) {
            bytes.push(1);
        } else {
            bytes.extend_from_slice(&4_u32.to_le_bytes());
        }
        match tag {
            SketchConstraintTag::Parallel | SketchConstraintTag::Perpendicular => {
                bytes.extend_from_slice(&0_u16.to_le_bytes());
            }
            SketchConstraintTag::Distance => bytes.extend_from_slice(&[0; 48]),
            SketchConstraintTag::Midpoint => bytes.extend_from_slice(&0_u32.to_le_bytes()),
            _ => {}
        }
        let source = parse(&bytes, |_, source| {
            parse_constraint(tag, source, 22).expect("relation to block member")
        });
        inventory.constraints.push(located(source, record_type, 14));
        add_members(&mut inventory, &[15]);
        let projected = projection(&inventory);
        assert_eq!(projected.sketches.len(), 1);
        assert_eq!(projected.entities.len(), 10);
        assert!(projected.constraints.is_empty());
        assert_eq!(projected.unresolved_constraints, 2);
        assert_eq!(projected.unresolved_sketches, 0);
        let loss = super::super::graph_loss(
            projected.unresolved_sketches,
            projected.unresolved_entities,
            projected.unresolved_constraints,
        )
        .expect("native relation loss");
        assert_eq!(loss.code.local_code(), "sketch.graph-open");
        assert!(loss.message.contains("2 sketch-constraint record(s)"));
    }
}

#[test]
fn duplicate_native_members_and_relations_keep_the_sketch_unresolved() {
    for duplicate_relation in [false, true] {
        let mut inventory = block_inventory();
        if duplicate_relation {
            inventory.constraints.push(inventory.constraints[0].clone());
        } else {
            inventory.entities.push(inventory.entities[8].clone());
        }
        assert!(projection(&inventory).sketches.is_empty());
    }
}
