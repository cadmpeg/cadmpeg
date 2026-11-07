// SPDX-License-Identifier: Apache-2.0
//! Text member records of a sketch.

use std::collections::HashSet;

use super::closed_square_inventory;
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::Located;
use crate::sketch::curve::member_loss_counts;
use crate::sketch::text::{is_text_member_type, TextMember};
use crate::sketch::{project, SketchInventory, SketchProjection, SKETCH_TYPE};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Record type of a sketch text.
const TEXT: [u8; 16] = [
    0x01, 0x3c, 0xbb, 0x1f, 0xd2, 0x11, 0x4d, 0x68, 0xa0, 0x00, 0x9e, 0x9a, 0x3c, 0x3a, 0xa0, 0x76,
];
/// Record type of a text anchor.
const ANCHOR: [u8; 16] = [
    0x7f, 0x34, 0x10, 0x25, 0xae, 0x4a, 0x79, 0x96, 0x1d, 0x90, 0x47, 0x86, 0x9d, 0x5f, 0x4e, 0x48,
];
/// Record type of the group of box entities of a text.
const GROUP: [u8; 16] = [
    0x0e, 0xad, 0x98, 0x7a, 0xb9, 0x49, 0x5c, 0xee, 0xfc, 0x18, 0x67, 0x87, 0xa2, 0xa2, 0x06, 0x43,
];
/// Record type of a planar curve member, which is not a text member.
const CURVE_MEMBER: [u8; 16] = [
    0x2b, 0x11, 0x7b, 0x31, 0xd3, 0x11, 0x7c, 0x7a, 0x60, 0x00, 0x1c, 0xb3, 0xd1, 0xc1, 0xfb, 0xb0,
];
/// Ordinal of the listed text record; the square uses ordinals below it.
const TEXT_ORDINAL: u32 = 30;

/// Lists a record of `ordinal` in the sketch.
fn list_member(inventory: &mut SketchInventory, ordinal: u32) {
    let sketch = &inventory.sketches[0];
    let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
    references
        .push(PmDcReference::new(ordinal + 1, false).expect("test reference index fits 31 bits"));
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

fn text_member(ordinal: u32) -> TextMember {
    TextMember {
        segment_token: cadmpeg_ir::identity_key!("segment"),
        record_ordinal: ordinal,
    }
}

#[test]
fn a_listed_text_member_does_not_keep_its_sketch_open() {
    let (mut inventory, _) = closed_square_inventory();
    list_member(&mut inventory, TEXT_ORDINAL);
    inventory.text_members.push(text_member(TEXT_ORDINAL));
    let projected = projection(&inventory);
    assert_eq!(projected.sketches.len(), 1);
    assert_eq!(projected.entities.len(), 8);
    assert_eq!(projected.sketches[0].profiles.iter().count(), 1);
}

#[test]
fn an_unknown_listed_member_keeps_its_sketch_open() {
    let (mut inventory, _) = closed_square_inventory();
    list_member(&mut inventory, TEXT_ORDINAL);
    let projected = projection(&inventory);
    assert!(projected.sketches.is_empty());
    assert!(projected.entities.is_empty());
}

#[test]
fn a_text_member_at_another_ordinal_does_not_close_the_sketch() {
    let (mut inventory, _) = closed_square_inventory();
    list_member(&mut inventory, TEXT_ORDINAL);
    inventory.text_members.push(text_member(TEXT_ORDINAL + 1));
    let projected = projection(&inventory);
    assert!(projected.sketches.is_empty());
}

#[test]
fn only_the_three_text_member_types_are_text_members() {
    assert!(is_text_member_type(TEXT));
    assert!(is_text_member_type(ANCHOR));
    assert!(is_text_member_type(GROUP));
    assert!(!is_text_member_type(CURVE_MEMBER));
    assert!(!is_text_member_type(crate::sketch::LINE_TYPE));
}

#[test]
fn text_members_report_a_pinned_loss_and_other_members_keep_theirs() {
    let unresolved = HashSet::from([("generated", 0), ("generated", 1), ("generated", 2)]);
    let losses = member_loss_counts(
        &unresolved,
        [TEXT, TEXT, CURVE_MEMBER]
            .into_iter()
            .enumerate()
            .map(|(ordinal, id)| {
                (
                    "generated",
                    u32::try_from(ordinal).expect("synthetic ordinal"),
                    id,
                )
            }),
    );
    let text = losses
        .iter()
        .find(|loss| loss.code.local_code() == "sketch.text-member-native")
        .expect("text loss");
    assert!(text.message.contains("Retained 2"));
    assert!(text.message.contains("stores no curve"));
    assert!(losses
        .iter()
        .any(|loss| loss.code.local_code() == "sketch.member-native"
            && loss.message.contains("Retained 1")));
    assert_eq!(losses.len(), 2);
}
