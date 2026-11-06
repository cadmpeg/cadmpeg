// SPDX-License-Identifier: Apache-2.0
//! Construction lines whose stored carrier and endpoints disagree.

use super::{closed_square_inventory, hinted_relation, real};
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::Located;
use crate::sketch::{
    project, PmDcSketchEntityKind, SketchInventory, SketchProjection, SKETCH_TYPE,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

const CONSTRUCTION_FLAG: u32 = 0x8_0000;
const PROFILE_FLAG: u32 = 0x40;
/// Native reference of the first square side.
const FIRST_SIDE: &str = "inventor:pmdc:sketch-entity#segment-7";

/// Sets the flags of the first square side and, when `skew` is true, turns
/// its carrier direction 45 degrees away from the side.
fn edit_first_side(inventory: &mut SketchInventory, flags: u32, skew: bool) {
    let index = inventory
        .entities
        .iter()
        .position(|entity| matches!(entity.kind, PmDcSketchEntityKind::Line { .. }))
        .expect("square side");
    crate::test_support::sketch_curves::edit_entity(&mut inventory.entities[index], |payload| {
        payload.entity_flags = flags;
        if skew {
            let PmDcSketchEntityKind::Line { direction, .. } = &mut payload.kind else {
                unreachable!("square side")
            };
            *direction = [real(1.0), real(1.0)];
        }
    });
}

/// Lists the relation at ordinal 11, a coincidence of the first side with a
/// corner point, in the sketch.
fn add_relation(inventory: &mut SketchInventory) {
    inventory.constraints.push(hinted_relation(0));
    let sketch = &inventory.sketches[0];
    let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
    references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
    let mut payload = (**sketch).clone();
    payload.entities =
        PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
    inventory.sketches[0] = Located::new(
        payload,
        crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        2,
    );
}

fn projection(inventory: &SketchInventory) -> SketchProjection {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project(&ctx, inventory, &[]).expect("sketch projection")
}

#[test]
fn an_inconsistent_construction_line_stays_native_and_the_sketch_closes() {
    let (mut inventory, _) = closed_square_inventory();
    edit_first_side(&mut inventory, CONSTRUCTION_FLAG, true);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.native_carrier_conflicts, 1);
    assert_eq!(projected.unresolved_entities, 1);
    // Four points and three sides; the line has no neutral geometry.
    assert_eq!(projected.entities.len(), 7);
    assert!(projected
        .entities
        .iter()
        .all(|entity| entity.native_ref.as_deref() != Some(FIRST_SIDE)));
    // The construction line bounds no profile, and three sides close none.
    assert!(projected.sketches[0].profiles.is_empty());
}

#[test]
fn a_relation_on_an_inconsistent_construction_line_stays_native() {
    let (mut inventory, _) = closed_square_inventory();
    edit_first_side(&mut inventory, CONSTRUCTION_FLAG, true);
    add_relation(&mut inventory);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert!(projected.constraints.is_empty());
    assert_eq!(projected.unresolved_constraints, 1);
    assert_eq!(projected.native_carrier_conflicts, 1);
}

#[test]
fn the_same_relation_projects_on_a_consistent_construction_line() {
    let (mut inventory, _) = closed_square_inventory();
    edit_first_side(&mut inventory, CONSTRUCTION_FLAG, false);
    add_relation(&mut inventory);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.constraints.len(), 1);
    assert_eq!(projected.unresolved_constraints, 0);
}

#[test]
fn an_inconsistent_line_without_a_construction_bit_keeps_the_sketch_open() {
    let (mut inventory, _) = closed_square_inventory();
    edit_first_side(&mut inventory, PROFILE_FLAG, true);
    let projected = projection(&inventory);
    assert!(projected.sketches.is_empty());
    assert!(projected.entities.is_empty());
    assert_eq!(projected.native_carrier_conflicts, 0);
}

#[test]
fn a_consistent_construction_line_projects_as_before() {
    let (mut inventory, _) = closed_square_inventory();
    edit_first_side(&mut inventory, CONSTRUCTION_FLAG, false);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.native_carrier_conflicts, 0);
    assert_eq!(projected.entities.len(), 8);
    assert_eq!(
        projected
            .entities
            .iter()
            .filter(|entity| entity.construction)
            .count(),
        1
    );
}

#[test]
fn carrier_conflicts_are_reported_with_a_pinned_loss() {
    assert!(crate::sketch::carrier_conflict_loss(0).is_none());
    let note = crate::sketch::carrier_conflict_loss(2).expect("loss for retained lines");
    assert_eq!(
        note.code.local_code(),
        "sketch.construction-carrier-conflict"
    );
    assert!(note.message.contains("2 Inventor construction line"));
}
