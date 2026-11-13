// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::extrude::DesignExtrudeStart;

#[test]
fn compact_distance_scope_walks_present_reference_slots() {
    for (present, first_offset, count_offset) in [(4, 115, 261), (5, 125, 271)] {
        let mut bytes = vec![0; count_offset + 4];
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[26..30].copy_from_slice(&4u32.to_le_bytes());
        bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
        bytes[34..38].copy_from_slice(&2u32.to_le_bytes());
        bytes[39] = 1;
        bytes[60..68].copy_from_slice(&1.0f64.to_le_bytes());
        let mut cursor = 68;
        let references: [u32; 5] = [10, 11, 12, 13, 14];
        for (ordinal, reference) in references.into_iter().enumerate() {
            if ordinal < present {
                bytes[cursor] = 1;
                bytes[cursor + 1..cursor + 5].copy_from_slice(&reference.to_le_bytes());
                cursor += 11;
            } else {
                cursor += 1;
            }
        }
        cursor += 2;
        assert_eq!(cursor, first_offset);
        bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
        let prologue = super::super::exact_compact_shifted_extrude_prologue(
            &bytes,
            0,
            count_offset,
            &references[..present],
        )
        .expect("compact distance prologue");
        assert_eq!(
            prologue.extent(),
            Some(DesignExtrudeExtent::OneSidedDistance)
        );
        assert_eq!(prologue.operation(), DesignExtrudeOperation::NewBody);
        assert_eq!(prologue.start(), DesignExtrudeStart::ProfilePlane);
        assert_eq!(prologue.side_extent_discriminators(), Some([1, 0]));
        bytes[69] = 99;
        assert!(super::super::exact_compact_shifted_extrude_prologue(
            &bytes,
            0,
            count_offset,
            &references[..present],
        )
        .is_none());
    }
}

#[test]
fn typed_extrude_prefix_retains_the_construction_group_reference() {
    let mut bytes = vec![0; 204];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[25] = 1;
    bytes[26..34].copy_from_slice(&25u64.to_le_bytes());
    bytes[34..38].copy_from_slice(&36u32.to_le_bytes());
    bytes[38..74].copy_from_slice(b"2CA5A1CD-C99B-4C9B-91AF-57989148E841");
    bytes[76..80].copy_from_slice(&2u32.to_le_bytes());
    bytes[80..84].copy_from_slice(&1u32.to_le_bytes());
    bytes[84..88].copy_from_slice(&2u32.to_le_bytes());
    bytes[89] = 1;
    bytes[90] = 2;
    bytes[110..118].copy_from_slice(&1.0f64.to_le_bytes());
    bytes[125..129].copy_from_slice(&1u32.to_le_bytes());
    let value = super::super::exact_typed_extrude_prologue(&bytes, 0, 200, &[25])
        .expect("typed Extrude prologue");
    assert_eq!(value.extent(), Some(DesignExtrudeExtent::OneSidedDistance));
    assert_eq!(value.operation(), DesignExtrudeOperation::Cut);
    assert_eq!(value.start(), DesignExtrudeStart::FromFace);
    let wire = serde_json::to_value(value).unwrap();
    assert_eq!(wire["reference"]["record_index"], 25);
    assert_eq!(wire["reference"]["record_index_offset"], 26);
    assert_eq!(
        wire["reference"]["type_guid"],
        "2CA5A1CD-C99B-4C9B-91AF-57989148E841"
    );
    assert_eq!(
        serde_json::from_value::<DesignExtrudePrologue>(wire).unwrap(),
        value
    );
    bytes[38] = b'3';
    assert!(super::super::exact_typed_extrude_prologue(&bytes, 0, 200, &[25]).is_none());
}

#[test]
fn reference_aware_two_sided_extents_keep_side_specific_references() {
    let make = |operation_offset: usize, second: u32| {
        let mut bytes = vec![0; 304];
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        if operation_offset == 28 {
            bytes[27] = 1;
        }
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&4u32.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8].copy_from_slice(&2u32.to_le_bytes());
        bytes[operation_offset + 13] = 1;
        bytes[operation_offset + 34..operation_offset + 42].copy_from_slice(&1.0f64.to_le_bytes());
        let mut cursor = operation_offset + 42;
        for reference in [10u32, 11, 12, 13] {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(&reference.to_le_bytes());
            cursor += 11;
        }
        cursor += 3;
        bytes[cursor..cursor + 4].copy_from_slice(&1u32.to_le_bytes());
        let second_offset = cursor + 13;
        bytes[second_offset..second_offset + 4].copy_from_slice(&second.to_le_bytes());
        if second == 2 {
            bytes[cursor + 9..cursor + 13].copy_from_slice(&1u32.to_le_bytes());
            bytes[second_offset + 5] = 1;
            bytes[second_offset + 6..second_offset + 10].copy_from_slice(&20u32.to_le_bytes());
            bytes[second_offset + 20] = 1;
            bytes[second_offset + 21..second_offset + 25].copy_from_slice(&21u32.to_le_bytes());
        }
        bytes
    };
    let references = [10, 11, 12, 13, 20, 21];
    let shifted = make(27, 1);
    let shifted_value =
        super::super::exact_extrude_prologue(&shifted, 0, 300, "300", "260", 300, &references)
            .expect("shifted two-sided distance prologue");
    assert_eq!(
        shifted_value.extent(),
        Some(DesignExtrudeExtent::TwoSidedDistance)
    );
    assert_eq!(shifted_value.side_extent_discriminators(), Some([1, 1]));
    let distance = make(26, 1);
    let value =
        super::super::exact_extrude_prologue(&distance, 0, 300, "300", "260", 300, &references)
            .expect("two-sided distance prologue");
    assert_eq!(value.extent(), Some(DesignExtrudeExtent::TwoSidedDistance));
    assert_eq!(value.side_extent_discriminators(), Some([1, 1]));
    let mut mixed = make(28, 2);
    let value =
        super::super::exact_extrude_prologue(&mixed, 0, 300, "300", "260", 300, &references)
            .expect("distance-to-face prologue");
    assert_eq!(
        value.extent(),
        Some(DesignExtrudeExtent::TwoSidedDistanceToFace)
    );
    assert_eq!(value.side_extent_discriminators(), Some([1, 2]));
    let face_first = |target_option: u32, second: u32| {
        let mut bytes = make(28, 1);
        let first = 117;
        bytes[first..200].fill(0);
        bytes[first..first + 4].copy_from_slice(&2u32.to_le_bytes());
        bytes[first + 4] = 1;
        bytes[first + 5..first + 9].copy_from_slice(&20u32.to_le_bytes());
        bytes[first + 19..first + 23].copy_from_slice(&target_option.to_le_bytes());
        bytes[first + 23..first + 27].copy_from_slice(&second.to_le_bytes());
        if second == 2 {
            bytes[first + 28] = 1;
            bytes[first + 29..first + 33].copy_from_slice(&20u32.to_le_bytes());
            bytes[first + 43] = 1;
            bytes[first + 44..first + 48].copy_from_slice(&21u32.to_le_bytes());
        }
        bytes
    };
    for (target, second, expected, wire_extent) in [
        (
            0,
            1,
            DesignExtrudeExtent::TwoSidedTargetToDistanceOptionZero,
            "two_sided_target_to_distance_option_zero",
        ),
        (
            0,
            2,
            DesignExtrudeExtent::TwoSidedToTargetsOptionZero,
            "two_sided_to_targets_option_zero",
        ),
        (
            2,
            2,
            DesignExtrudeExtent::TwoSidedToTargetsOptionTwo,
            "two_sided_to_targets_option_two",
        ),
    ] {
        let bytes = face_first(target, second);
        let value =
            super::super::exact_extrude_prologue(&bytes, 0, 300, "300", "260", 300, &references)
                .expect("face-first prologue");
        assert_eq!(value.extent(), Some(expected));
        let wire = serde_json::to_value(value).unwrap();
        assert_eq!(wire["extent"], wire_extent);
        assert_eq!(
            serde_json::from_value::<DesignExtrudePrologue>(wire).unwrap(),
            value
        );
    }
    let mut through_all_to_face = make(27, 2);
    through_all_to_face[116..120].copy_from_slice(&4u32.to_le_bytes());
    let value = super::super::exact_extrude_prologue(
        &through_all_to_face,
        0,
        300,
        "300",
        "260",
        300,
        &references,
    )
    .expect("through-all to face prologue");
    assert_eq!(
        value.extent(),
        Some(DesignExtrudeExtent::TwoSidedThroughAllToFace)
    );
    let mut controlled = mixed.clone();
    controlled[134] = 1;
    let value =
        super::super::exact_extrude_prologue(&controlled, 0, 300, "300", "260", 300, &references)
            .expect("distance-to-face control prologue");
    assert_eq!(
        value.extent(),
        Some(DesignExtrudeExtent::TwoSidedDistanceToFaceControlOne)
    );
    let wire = serde_json::to_value(value).unwrap();
    assert_eq!(wire["extent"], "two_sided_distance_to_face_control_one");
    assert_eq!(
        serde_json::from_value::<DesignExtrudePrologue>(wire).unwrap(),
        value
    );
    controlled[134] = 2;
    assert!(super::super::exact_extrude_prologue(
        &controlled,
        0,
        300,
        "300",
        "260",
        300,
        &references,
    )
    .is_none());
    let through_all = make(28, 4);
    let value =
        super::super::exact_extrude_prologue(&through_all, 0, 300, "300", "260", 300, &references)
            .expect("distance and through-all prologue");
    assert_eq!(
        value.extent(),
        Some(DesignExtrudeExtent::TwoSidedDistanceThroughAll)
    );
    assert_eq!(value.side_extent_discriminators(), Some([1, 4]));
    mixed[151] = 99;
    assert!(
        super::super::exact_extrude_prologue(&mixed, 0, 300, "300", "260", 300, &references,)
            .is_none()
    );
}

#[test]
fn slotted_prologue_reads_one_sided_and_symmetric_directions_at_shifted_offsets() {
    let make = |operation_offset: usize, direction: u32, first: u32, second: u32| {
        let mut bytes = vec![0; 304];
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&4u32.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8].copy_from_slice(&direction.to_le_bytes());
        bytes[operation_offset + 13] = 1;
        bytes[operation_offset + 34..operation_offset + 42].copy_from_slice(&1.0f64.to_le_bytes());
        let mut cursor = operation_offset + 42;
        for reference in [10u32, 11, 12, 13] {
            bytes[cursor] = 1;
            bytes[cursor + 1..cursor + 5].copy_from_slice(&reference.to_le_bytes());
            cursor += 11;
        }
        cursor += 3;
        bytes[cursor..cursor + 4].copy_from_slice(&first.to_le_bytes());
        if first == 2 {
            bytes[cursor + 4] = 1;
            bytes[cursor + 5..cursor + 9].copy_from_slice(&14u32.to_le_bytes());
        } else {
            bytes[cursor + 13..cursor + 17].copy_from_slice(&second.to_le_bytes());
        }
        if direction == 3 {
            bytes[operation_offset + 8..operation_offset + 12].copy_from_slice(&2u32.to_le_bytes());
            bytes[cursor + 25..cursor + 29].copy_from_slice(&2u32.to_le_bytes());
        }
        bytes
    };
    let references = [10, 11, 12, 13, 14];
    for (operation_offset, direction, first, second, extent) in [
        (26, 1, 1, 0, DesignExtrudeExtent::OneSidedDistance),
        (27, 1, 1, 0, DesignExtrudeExtent::OneSidedDistance),
        (26, 3, 1, 1, DesignExtrudeExtent::SymmetricDistance),
        (27, 3, 1, 1, DesignExtrudeExtent::SymmetricDistance),
        (26, 1, 2, 0, DesignExtrudeExtent::OneSidedToFace),
    ] {
        let bytes = make(operation_offset, direction, first, second);
        let value =
            super::super::exact_extrude_prologue(&bytes, 0, 300, "300", "260", 300, &references)
                .unwrap_or_else(|| {
                    panic!("slotted prologue {operation_offset} {direction} {first}")
                });
        assert_eq!(value.extent(), Some(extent));
        assert_eq!(value.operation(), DesignExtrudeOperation::NewBody);
        let DesignExtrudePrologue::ReferenceAware {
            operation_offset: actual,
            ..
        } = value
        else {
            panic!("reference-aware prologue");
        };
        assert_eq!(actual, u64::try_from(operation_offset).unwrap());
    }
}

#[test]
fn slotted_prologue_reads_two_face_targets_outside_the_dedicated_shifted_frame() {
    let mut bytes = vec![0; 304];
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[27..31].copy_from_slice(&2u32.to_le_bytes());
    bytes[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes[35..39].copy_from_slice(&1u32.to_le_bytes());
    bytes[40] = 1;
    bytes[61..69].copy_from_slice(&1.0f64.to_le_bytes());
    let mut cursor = 69;
    for reference in [10u32, 11, 12, 13] {
        bytes[cursor] = 1;
        bytes[cursor + 1..cursor + 5].copy_from_slice(&reference.to_le_bytes());
        cursor += 11;
    }
    cursor += 3;
    bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
    for (offset, reference) in [(4, 14u32), (28, 15), (43, 16)] {
        bytes[cursor + offset] = 1;
        bytes[cursor + offset + 1..cursor + offset + 5].copy_from_slice(&reference.to_le_bytes());
    }
    bytes[cursor + 19..cursor + 23].copy_from_slice(&1u32.to_le_bytes());
    bytes[cursor + 23..cursor + 27].copy_from_slice(&2u32.to_le_bytes());
    let references = [10, 11, 12, 13, 14, 15, 16];
    let value =
        super::super::exact_extrude_prologue(&bytes, 0, 300, "300", "260", 300, &references)
            .expect("two face targets");
    assert_eq!(value.extent(), Some(DesignExtrudeExtent::TwoSidedToFaces));
    assert_eq!(value.operation(), DesignExtrudeOperation::Cut);
    assert!(
        super::super::exact_extrude_prologue(&bytes, 0, 300, "300", "260", 292, &references)
            .is_none()
    );
}

#[test]
fn seventh_slot_record_is_read_when_the_slot_run_ends_at_the_first_side_extent() {
    use super::super::extrude_seventh_slot_record_index;
    use crate::layout::current_extrude_operation_fields as extrude_fields;
    const OPERATION: usize = 26;
    let slots_at = OPERATION + extrude_fields::PROFILE_NORMAL + 24;
    let build = |presence: [bool; 7]| {
        let mut bytes = vec![0; 200];
        let mut cursor = slots_at;
        for (ordinal, present) in presence.into_iter().enumerate() {
            if present {
                bytes[cursor] = 1;
                bytes[cursor + 1..cursor + 5]
                    .copy_from_slice(&(7_000 + u32::try_from(ordinal).unwrap()).to_le_bytes());
                cursor += 11;
            } else {
                cursor += 1;
            }
        }
        bytes[cursor..cursor + 4].copy_from_slice(&2u32.to_le_bytes());
        (bytes, cursor)
    };
    let prologue = |first_side_extent_offset: usize| DesignExtrudePrologue::LegacyShifted {
        operation_prefix_marker_offset: None,
        operation: DesignExtrudeOperation::Cut,
        operation_offset: u64::try_from(OPERATION).unwrap(),
        direction_face_extend_values: [1, 1],
        side_extent_discriminators: [2, 0],
        side_extent_discriminator_offsets: [u64::try_from(first_side_extent_offset).unwrap(), 180],
        extent: Some(DesignExtrudeExtent::OneSidedToFace),
        direction_face_extend_offsets: [30, 34],
        direction_reversed: false,
        direction_reversed_offset: 38,
        solid_operation: true,
        solid_operation_offset: 39,
        start: DesignExtrudeStart::FromFace,
        start_offset: 40,
    };

    let (bytes, extent_at) = build([true, false, false, true, false, true, true]);
    assert_eq!(
        extrude_seventh_slot_record_index(&bytes, prologue(extent_at)),
        Some(7_006)
    );

    let (bytes, extent_at) = build([true, false, false, true, false, true, false]);
    assert_eq!(
        extrude_seventh_slot_record_index(&bytes, prologue(extent_at)),
        None
    );

    let (bytes, extent_at) = build([true, false, false, true, false, true, true]);
    assert_eq!(
        extrude_seventh_slot_record_index(&bytes, prologue(extent_at + 5)),
        None
    );
}
