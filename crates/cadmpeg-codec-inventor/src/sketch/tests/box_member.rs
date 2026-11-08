// SPDX-License-Identifier: Apache-2.0
//! Box frame and box group member records of a sketch.

use std::collections::HashSet;

use crate::container::InventorContainer;
use crate::rse::{RecordFrameState, SegmentBulkState, SegmentKind};
use crate::sketch::box_member::{admit, is_box_member_type, member_loss};
use crate::sketch::curve::member_loss_counts;
use crate::sketch::{inventory, SketchInventory};
use crate::test_support::test_fixtures::content;
use crate::test_support::test_fixtures::primary_envelope_fixture;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, View};
use cadmpeg_core::CodecError;

/// Record type of a box frame.
const FRAME: [u8; 16] = [
    0x08, 0xdb, 0x93, 0xdc, 0xc1, 0x49, 0xf2, 0x6f, 0xf2, 0x29, 0xec, 0xb1, 0x80, 0x4d, 0x97, 0x84,
];
/// Record type of a box group.
const GROUP: [u8; 16] = [
    0x5f, 0x33, 0xec, 0x8f, 0x6c, 0x4f, 0xb6, 0xbe, 0x9f, 0xcc, 0x71, 0x96, 0x3a, 0xd1, 0xb9, 0xdf,
];
/// Record type of the group of box entities of a text.
const TEXT_GROUP: [u8; 16] = [
    0x0e, 0xad, 0x98, 0x7a, 0xb9, 0x49, 0x5c, 0xee, 0xfc, 0x18, 0x67, 0x87, 0xa2, 0xa2, 0x06, 0x43,
];
/// The segment major version of the admitted body grammar.
const VERSION: u8 = 19;

/// The inventory of one record placed in the first segment of the envelope
/// fixture, with the segment major version replaced by `version`.
fn inventory_with_record_at(
    type_id: [u8; 16],
    payload: &[u8],
    policy: DecodePolicy,
    version: Option<u8>,
) -> Result<SketchInventory, CodecError> {
    let bytes = primary_envelope_fixture();
    let payload = payload.to_vec();
    let arena = DecodeArena::new();
    let (setup_ctx, source) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
            .expect("envelope view");
    let mut container = InventorContainer::open(&setup_ctx, source).expect("framed envelope");
    let segment = &mut container.rse.segments[0];
    segment.kind = SegmentKind::PmDc;
    if let Some(version) = version {
        segment
            .registry
            .as_mut()
            .expect("registry join fixture")
            .version_major = version;
    }
    let SegmentBulkState::Framed(bulk) = &mut segment.bulk else {
        panic!("framed bulk fixture");
    };
    let RecordFrameState::Framed(table) = &mut bulk.records else {
        panic!("framed record fixture");
    };
    table.records[0].type_id = type_id;
    table.records[0].payload = View::over_retained(&payload);
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input view");
    inventory(&ctx, &container.rse)
}

fn reference(index: u32) -> [u8; 4] {
    (0x8000_0000 | index).to_le_bytes()
}

/// A box frame body: sketch reference, width, height, corner, a u32 value, a
/// placement matrix with explicit cells 3 and 7, and the fixed tail.
fn frame(width: f64, height: f64, sketch: u32) -> Vec<u8> {
    let mut bytes = content(0);
    bytes.extend_from_slice(&reference(sketch));
    for value in [width, height, -1.0, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0x8421u16.to_le_bytes());
    bytes.extend_from_slice(&0x7b56u16.to_le_bytes());
    for value in [-1.0f64, 3.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0x0100u16.to_le_bytes());
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.push(1);
    bytes
}

/// A box group body: a u32 value and `count` references.
fn group(count: u32) -> Vec<u8> {
    let mut bytes = content(0);
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    for index in 1..=count {
        bytes.extend_from_slice(&reference(index));
    }
    bytes
}

fn admitted(type_id: [u8; 16], bytes: &[u8], version: u8) -> bool {
    admit(type_id, View::over_retained(bytes), version).is_ok()
}

#[test]
fn a_frame_and_a_group_of_the_stated_grammar_are_admitted() {
    assert!(admitted(FRAME, &frame(4.0, 2.0, 1), VERSION));
    assert!(admitted(GROUP, &group(7), VERSION));
}

#[test]
fn a_frame_with_masked_diagonal_and_no_explicit_cells_is_admitted() {
    let mut bytes = content(0);
    bytes.extend_from_slice(&reference(1));
    for value in [4.0f64, 2.0, 0.0, 0.0] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0x8421u16.to_le_bytes());
    bytes.extend_from_slice(&0x7bdeu16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&0x0100u16.to_le_bytes());
    bytes.extend_from_slice(&1.0f64.to_le_bytes());
    bytes.push(1);
    assert!(admitted(FRAME, &bytes, VERSION));
}

#[test]
fn a_frame_that_breaks_the_grammar_is_refused() {
    let mut trailing = frame(4.0, 2.0, 1);
    trailing.push(0);
    let truncated = frame(4.0, 2.0, 1)[..frame(4.0, 2.0, 1).len() - 1].to_vec();
    assert!(!admitted(FRAME, &trailing, VERSION));
    assert!(!admitted(FRAME, &truncated, VERSION));
    assert!(!admitted(FRAME, &frame(0.0, 2.0, 1), VERSION));
    assert!(!admitted(FRAME, &frame(4.0, -2.0, 1), VERSION));
    assert!(!admitted(FRAME, &frame(4.0, f64::NAN, 1), VERSION));
    assert!(!admitted(FRAME, &frame(4.0, 2.0, 0), VERSION));
}

#[test]
fn a_group_with_another_reference_count_is_refused() {
    assert!(!admitted(GROUP, &group(6), VERSION));
    assert!(!admitted(GROUP, &group(8), VERSION));
    assert!(!admitted(GROUP, &group(0), VERSION));
}

#[test]
fn only_version_19_bodies_are_admitted() {
    for version in [18, 20, 22, 25, 31] {
        assert!(!admitted(FRAME, &frame(4.0, 2.0, 1), version));
        assert!(!admitted(GROUP, &group(7), version));
    }
}

#[test]
fn only_the_frame_and_group_types_are_box_members() {
    assert!(is_box_member_type(FRAME));
    assert!(is_box_member_type(GROUP));
    assert!(!is_box_member_type(TEXT_GROUP));
    assert!(!is_box_member_type(crate::sketch::LINE_TYPE));
    assert!(!admitted(TEXT_GROUP, &group(7), VERSION));
}

#[test]
fn the_inventory_admits_a_box_member_as_a_member_that_closes_its_sketch() {
    for (type_id, bytes) in [(FRAME, frame(4.0, 2.0, 1)), (GROUP, group(7))] {
        let inventory =
            inventory_with_record_at(type_id, &bytes, DecodePolicy::service(), Some(VERSION))
                .expect("inventory");
        assert_eq!(inventory.text_members.len(), 1);
        assert!(inventory.issues.is_empty());
        assert!(inventory.entities.is_empty());
    }
}

#[test]
fn the_inventory_keeps_a_malformed_or_other_version_box_member_unresolved() {
    let mut trailing = frame(4.0, 2.0, 1);
    trailing.push(0);
    for (type_id, bytes, version) in [
        (FRAME, trailing, VERSION),
        (FRAME, frame(4.0, 2.0, 1), 22),
        (GROUP, group(6), VERSION),
    ] {
        let inventory =
            inventory_with_record_at(type_id, &bytes, DecodePolicy::service(), Some(version))
                .expect("inventory");
        assert!(inventory.text_members.is_empty());
    }
}

#[test]
fn admitted_box_members_report_a_pinned_loss_and_unadmitted_ones_keep_theirs() {
    let unresolved = HashSet::from([("generated", 0), ("generated", 1), ("generated", 2)]);
    let admitted = HashSet::from([("generated", 0), ("generated", 1)]);
    let losses = member_loss_counts(
        &unresolved,
        &admitted,
        [FRAME, GROUP, FRAME]
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
    let boxed = losses
        .iter()
        .find(|loss| loss.code.local_code() == "sketch.box-member-native")
        .expect("box loss");
    assert!(boxed.message.contains("Retained 2"));
    assert!(boxed.message.contains("stores no curve"));
    assert!(losses
        .iter()
        .any(|loss| loss.code.local_code() == "sketch.member-native"
            && loss.message.contains("Retained 1")));
    assert_eq!(losses.len(), 2);
    assert_eq!(member_loss(2).message, boxed.message);
}
