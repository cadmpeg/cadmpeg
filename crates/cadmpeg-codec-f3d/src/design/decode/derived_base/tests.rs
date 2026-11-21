// SPDX-License-Identifier: Apache-2.0

use super::{
    companion, derived_base_owner, mirror, mirror_owner, owner, DesignPrimaryFrame, View,
    MIRROR_DERIVED_BASE, PRESENTATION,
};
use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData, DESIGN_MODULE_FUSION};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;
use crate::records::references::DesignClassTag;
use std::collections::HashSet;

fn source_frame() -> Vec<u8> {
    let mut bytes = vec![0; 562];
    for at in [19, 24, 39, 54, 59, 74, 85] {
        bytes[at] = 1;
    }
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[70..74].copy_from_slice(&1u32.to_le_bytes());
    for (at, value) in [(25, 31u64), (40, 41), (60, 51), (75, 61), (86, 41)] {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

#[test]
fn source_pair_names_the_source_result_and_timeline_owner() {
    let source = super::derived_base_source(&source_frame(), 1000).unwrap();
    assert_eq!(source.parameter_body.value, 31);
    assert_eq!(source.result_body.value, 41);
    assert_eq!(source.owner_record.value, 51);
    assert_eq!(source.source_body.value, 61);
    assert_eq!(source.source_body.offset, 1075);
    assert_eq!(source.result_repeat_offset, 1086);
    assert!(source.frame_holds(1000, 1444));
    assert!(source.snapshot.is_none());
    assert!(super::derived_base_owner(&source_frame(), 1000).is_none());
}

#[test]
fn source_pair_requires_its_counts_local_references_and_repeated_result() {
    for at in [
        19, 20, 24, 33, 39, 48, 54, 55, 59, 68, 70, 74, 83, 85, 86, 94, 96,
    ] {
        let mut bytes = source_frame();
        bytes[at] ^= 3;
        assert!(super::derived_base_source(&bytes, 0).is_none(), "byte {at}");
    }
    assert!(super::derived_base_source(&source_frame()[..96], 0).is_none());
}

#[test]
fn source_pair_preserves_wide_body_references_and_checks_location_arithmetic() {
    let mut bytes = source_frame();
    bytes[75..83].copy_from_slice(&u64::MAX.to_le_bytes());
    assert_eq!(
        super::derived_base_source(&bytes, 0)
            .unwrap()
            .source_body
            .value,
        u64::MAX
    );
    assert!(super::derived_base_source(&bytes, usize::MAX).is_none());
    let mut source = super::derived_base_source(&bytes, 1000).unwrap();
    source.source_body.offset += 1;
    assert!(!source.frame_holds(1000, 1444));
}

/// A derived base frame of `len` bytes that names `owner` at offset 59 and
/// again twelve bytes before its end, with a zero last byte.
fn frame(len: usize, owner: u64) -> Vec<u8> {
    let mut frame = vec![0; len];
    for at in [59, len - 12] {
        frame[at] = 1;
        frame[at + 1..at + 9].copy_from_slice(&owner.to_le_bytes());
    }
    frame
}

#[test]
fn a_derived_base_frame_names_its_owner_twice() {
    let owner = derived_base_owner(&frame(405, 8269), 1000).expect("owner");
    assert_eq!(owner.record_index.value, 8269);
    assert_eq!(owner.record_index.offset, 1060);
    assert_eq!(owner.repeat_offset, 1393);
}

#[test]
fn a_derived_base_owner_needs_two_equal_well_formed_references() {
    let mut differs = frame(405, 8269);
    differs[394] = 2;
    let mut unmarked = frame(405, 8269);
    unmarked[59] = 0;
    unmarked[393] = 0;
    let mut trailing = frame(405, 8269);
    trailing[404] = 1;
    let mut unclosed = frame(405, 8269);
    unclosed[68] = 1;
    unclosed[402] = 1;
    for (frame, reason) in [
        (differs, "the repeated reference names another record"),
        (unmarked, "the references lack their marker"),
        (trailing, "the last byte is not zero"),
        (unclosed, "the references lack their two zero bytes"),
        (
            frame(405, u64::from(u32::MAX) + 1),
            "the owner exceeds a record index",
        ),
        (
            frame(75, 8269)[..70].to_vec(),
            "the repeat overlaps the first reference",
        ),
        (
            frame(405, 8269)[..65].to_vec(),
            "the frame ends inside the first reference",
        ),
    ] {
        assert_eq!(derived_base_owner(&frame, 0), None, "{reason}");
    }
}

#[test]
fn mirror_derived_base_owner_requires_registered_reciprocal_references() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#6",
        DesignFeatureKind::native("Derived Base Feature"),
        6,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(3);
            draft.previous_history_state_id = Some(2);
            draft.paired_byte_offset = 634;
            draft.frame_length = 634;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let type_row = |guid: &str, version| SegmentTypeData {
        byte_offset: 0,
        type_guid: guid.to_owned().try_into().unwrap(),
        type_guid_offset: 0,
        base_type_guid: BaseTypeGuid::Absent,
        version,
        version_offset: 0,
        module: DESIGN_MODULE_FUSION.into(),
        entities: ReferenceRun::unlocated(Vec::new()),
    };
    let scope_type = type_row(MIRROR_DERIVED_BASE, 10);
    let companion_type = type_row(PRESENTATION, 1);
    let tag = DesignClassTag::try_from("256".to_owned()).unwrap();
    let frames = [
        DesignPrimaryFrame {
            entity_id: 6,
            class_tag: tag.clone(),
            start: 0,
            member_end: 634,
            end: 664,
            design_type: &scope_type,
        },
        DesignPrimaryFrame {
            entity_id: 7,
            class_tag: tag,
            start: 664,
            member_end: 779,
            end: 779,
            design_type: &companion_type,
        },
    ];
    let mut bytes = [0; 779];
    bytes[20..24].copy_from_slice(&2_u32.to_le_bytes());
    let mut body_rows = Vec::new();
    for target in [9, 10] {
        crate::test_support::push_marked_reference(&mut body_rows, target);
        body_rows.extend_from_slice(&[0; 4]);
    }
    bytes[24..54].copy_from_slice(&body_rows);
    let reference = |bytes: &mut [u8], at: usize, index: u64| {
        bytes[at] = 1;
        bytes[at + 1..at + 9].copy_from_slice(&index.to_le_bytes());
    };
    for (at, index) in [
        (owner::MARKER, 4),
        (mirror::COMPANION_REFERENCE, 7),
        (664 + companion::OWNER_REFERENCE, 4),
        (664 + companion::SCOPE_REFERENCE, 6),
        (664 + companion::REPEATED_SCOPE_REFERENCE, 6),
    ] {
        reference(&mut bytes, at, index);
    }
    let owners = HashSet::from([4]);
    let read = |bytes: &[u8], frames: &[DesignPrimaryFrame<'_>], owners: &HashSet<u64>| {
        mirror_owner(View::over_retained(bytes), &scope, frames, owners)
    };
    let binding = read(&bytes, &frames, &owners).unwrap();
    assert_eq!(binding.record_index.value, 4);
    assert_eq!(binding.record_index.offset, 60);
    assert_eq!(binding.repeat_offset, 695);
    let result = binding.result_body.unwrap();
    assert_eq!(result.value, 10);
    assert_eq!(result.offset, 40);
    let mut bound_scope = scope.clone();
    bound_scope.derived_base_owner = Some(binding);
    let wire = serde_json::to_value(&bound_scope).unwrap();
    let decoded: DesignParameterScope = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.derived_base_owner, Some(binding));
    let mut missing_result = bytes;
    missing_result[39] = 0;
    let owner_only = read(&missing_result, &frames, &owners).unwrap();
    assert_eq!(owner_only.record_index, binding.record_index);
    assert_eq!(owner_only.result_body, None);

    let mut wrong_type = frames.clone();
    wrong_type[1].design_type = &scope_type;
    assert!(read(&bytes, &wrong_type, &owners).is_none());
    assert!(read(&bytes, &frames, &HashSet::new()).is_none());
    assert!(read(&bytes[..778], &frames, &owners).is_none());
    for at in [
        owner::MARKER + 9,
        mirror::COMPANION_REFERENCE + 1,
        mirror::CLOSING_ZEROES,
        664 + companion::OWNER_REFERENCE + 1,
        664 + companion::SCOPE_REFERENCE + 1,
        664 + companion::REPEATED_SCOPE_REFERENCE + 1,
    ] {
        let mut malformed = bytes;
        malformed[at] ^= 1;
        assert!(read(&malformed, &frames, &owners).is_none(), "{at}");
    }
}
