// SPDX-License-Identifier: Apache-2.0
//! Lines whose endpoints share one position.

use super::{closed_square_inventory, hinted_relation, real};
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::Located;
use crate::sketch::{
    degenerate_line, degenerate_line_loss, project, PmDcSketchEntityKind, SketchInventory,
    SketchProjection, SKETCH_TYPE,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Native reference of the first square side.
const FIRST_SIDE: &str = "inventor:pmdc:sketch-entity#segment-7";

/// Index of the first square side in the entity list.
fn first_side(inventory: &SketchInventory) -> usize {
    inventory
        .entities
        .iter()
        .position(|entity| matches!(entity.kind, PmDcSketchEntityKind::Line { .. }))
        .expect("square side")
}

/// Makes the first square side end at its own start point.
fn collapse_first_side(inventory: &mut SketchInventory) {
    let index = first_side(inventory);
    crate::test_support::sketch_curves::edit_entity(&mut inventory.entities[index], |payload| {
        let PmDcSketchEntityKind::Line { points, .. } = &mut payload.kind else {
            unreachable!("square side")
        };
        let (marker, metadata, mut references) = points.clone().into_parts();
        references[1] = references[0];
        *points =
            PmDcReferenceList::new(marker, metadata, references).expect("collapsed point list");
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
fn a_zero_length_line_stays_native_and_the_sketch_closes() {
    let (mut inventory, _) = closed_square_inventory();
    collapse_first_side(&mut inventory);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.native_degenerate_lines, 1);
    assert_eq!(projected.native_carrier_conflicts, 0);
    assert_eq!(projected.unresolved_entities, 1);
    // Four points and three sides; the collapsed line has no neutral geometry.
    assert_eq!(projected.entities.len(), 7);
    assert!(projected
        .entities
        .iter()
        .all(|entity| entity.native_ref.as_deref() != Some(FIRST_SIDE)));
    // Three sides close no profile.
    assert!(projected.sketches[0].profiles.is_empty());
}

#[test]
fn a_relation_on_a_zero_length_line_stays_native() {
    let (mut inventory, _) = closed_square_inventory();
    collapse_first_side(&mut inventory);
    add_relation(&mut inventory);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert!(projected.constraints.is_empty());
    assert_eq!(projected.unresolved_constraints, 1);
}

#[test]
fn a_line_with_distinct_endpoints_projects_as_before() {
    let (mut inventory, _) = closed_square_inventory();
    add_relation(&mut inventory);
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.native_degenerate_lines, 0);
    assert_eq!(projected.entities.len(), 8);
    assert_eq!(projected.constraints.len(), 1);
}

#[test]
fn the_predicate_compares_the_resolved_positions_of_the_endpoints() {
    let (inventory, _) = closed_square_inventory();
    let line = &inventory.entities[first_side(&inventory)];
    let PmDcSketchEntityKind::Line { points, .. } = &line.kind else {
        unreachable!("square side")
    };
    assert_ne!(
        points.references()[0].index(),
        points.references()[1].index()
    );
    // Two point records at one position: a zero-length line.
    assert!(degenerate_line(line, |_, _| Some([real(2.0), real(0.0)])));
    // Different positions along the carrier: a segment.
    assert!(!degenerate_line(line, |_, reference| {
        Some([real(f64::from(reference)), real(0.0)])
    }));
    // An unresolved endpoint is not a degenerate line.
    assert!(!degenerate_line(line, |_, reference| {
        (reference == points.references()[0].index()).then(|| [real(0.0), real(0.0)])
    }));
}

#[test]
fn zero_length_lines_are_reported_with_a_pinned_loss() {
    assert!(degenerate_line_loss(0).is_none());
    let note = degenerate_line_loss(3).expect("loss for retained lines");
    assert_eq!(note.code.local_code(), "sketch.degenerate-line-native");
    assert!(note.message.contains("3 Inventor line"));
}

#[test]
fn endpoints_that_differ_by_rounding_are_one_position() {
    let (inventory, _) = closed_square_inventory();
    let line = &inventory.entities[first_side(&inventory)];
    let PmDcSketchEntityKind::Line { points, .. } = &line.kind else {
        unreachable!("square side")
    };
    let first = points.references()[0].index();
    let base = 3.0_f64;
    let rounded = |ulps: f64| {
        move |_: &str, reference: u32| {
            let x = if reference == first {
                base
            } else {
                base + ulps * f64::EPSILON * base
            };
            Some([real(x), real(base)])
        }
    };
    // A difference of four units of rounding is one position.
    assert!(degenerate_line(line, rounded(4.0)));
    // A difference of eight units of rounding is a segment.
    assert!(!degenerate_line(line, rounded(8.0)));
    // A zero coordinate has no rounding allowance.
    assert!(!degenerate_line(line, |_, reference| {
        Some([
            real(if reference == first { 0.0 } else { 1e-300 }),
            real(0.0),
        ])
    }));
}
