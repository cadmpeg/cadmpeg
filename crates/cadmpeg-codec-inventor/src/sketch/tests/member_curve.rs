// SPDX-License-Identifier: Apache-2.0
//! Planar curve members of a sketch.

use super::closed_square_inventory;
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::{Located, RecordTypeId};
use crate::sketch::member_curve::tests::{curve_fixture, record, reference_fixture, Auxiliary};
use crate::sketch::{project, PmDcSketchEntity, SketchInventory, SKETCH_TYPE};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::sketches::SketchGeometryDefinition;

fn member_entity(ordinal: u32, bytes: &[u8]) -> PmDcSketchEntity {
    Located::new(
        record(bytes).expect("synthetic member curve"),
        RecordTypeId::from_bytes(crate::sketch::member_curve::TYPE),
        cadmpeg_ir::identity_key!("segment"),
        ordinal,
    )
}

/// The closed square with a member listed in its sketch and, when `member`
/// holds the member's bytes, present as an entity record.
fn square_with_member(member: Option<Vec<u8>>) -> SketchInventory {
    let (mut inventory, _) = closed_square_inventory();
    let sketch = &inventory.sketches[0];
    let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
    references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
    let mut payload = (**sketch).clone();
    payload.entities =
        PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
    inventory.sketches[0] = Located::new(
        payload,
        RecordTypeId::from_bytes(SKETCH_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        2,
    );
    if let Some(bytes) = member {
        inventory.entities.push(member_entity(11, &bytes));
    }
    inventory
}

fn projected(inventory: &SketchInventory) -> crate::sketch::SketchProjection {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project(&ctx, inventory, &[]).expect("sketch projection")
}

fn members() -> Vec<Vec<u8>> {
    vec![
        reference_fixture(4, 5, Auxiliary::List),
        reference_fixture(4, 5, Auxiliary::None),
        curve_fixture(4, 5, &[]),
        curve_fixture(4, 5, &[1., 2., 1.]),
    ]
}

#[test]
fn a_parsed_member_stays_native_and_the_sketch_closes() {
    for bytes in members() {
        let inventory = square_with_member(Some(bytes));
        let projection = projected(&inventory);
        assert_eq!(projection.sketches.len(), 1);
        // The square profile is unaffected; the member adds no neutral geometry.
        assert_eq!(projection.sketches[0].profiles.len(), 1);
        assert_eq!(projection.entities.len(), 9);
        let native = projection
            .entities
            .iter()
            .filter(|entity| {
                matches!(
                    entity.geometry.definition(),
                    SketchGeometryDefinition::Native { .. }
                )
            })
            .count();
        assert_eq!(native, 1);
    }
}

#[test]
fn an_unparsed_member_keeps_the_sketch_open() {
    let projection = projected(&square_with_member(None));
    assert!(projection.sketches.is_empty());
}

#[test]
fn each_body_is_reported_with_its_own_reason_under_the_pinned_loss() {
    for (bytes, reason) in members().into_iter().zip([
        "reference value",
        "reference value",
        "exact curve rule",
        "exact curve rule",
    ]) {
        let inventory = square_with_member(Some(bytes));
        let losses = crate::sketch::curve::native_losses(&inventory.entities);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code.local_code(), "sketch.member-curve-native");
        assert!(losses[0].message.contains("Retained 1"));
        assert!(losses[0].message.contains(reason));
    }
    // A square alone reports nothing.
    assert!(crate::sketch::curve::native_losses(&square_with_member(None).entities).is_empty());
}
