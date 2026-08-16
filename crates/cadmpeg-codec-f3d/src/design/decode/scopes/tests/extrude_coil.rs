// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

#[test]
fn extrude_scope_discriminators_follow_optional_indexed_reference() {
    let scope = |kind: &str,
                 operation: u32,
                 direction_face_extend: (u32, u32),
                 direction_reversed: u8,
                 structural_constant: u8,
                 start: u8,
                 reference_padding: Option<usize>,
                 reference_marker: bool,
                 current_side_extents: Option<((u32, u32), bool)>,
                 legacy_side_extents: Option<((u32, u32), bool)>,
                 legacy_reference_count_offset: Option<usize>| {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"301");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.resize(120, 0);
        bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
        let legacy_operation_marker = legacy_side_extents.is_some() && reference_marker;
        let legacy_field_shift = usize::from(legacy_operation_marker);
        let operation_offset = if legacy_side_extents.is_some() {
            27 + legacy_field_shift
        } else if let Some(reference_padding) = reference_padding {
            bytes[25] = 1;
            bytes[26..30].copy_from_slice(&77u32.to_le_bytes());
            30 + reference_padding
        } else {
            28
        };
        if reference_marker {
            if legacy_operation_marker {
                assert_eq!(reference_padding, None);
            } else {
                assert_eq!(reference_padding, Some(8));
            }
            bytes[operation_offset - 1] = 1;
        }
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8]
            .copy_from_slice(&direction_face_extend.0.to_le_bytes());
        bytes[operation_offset + 8..operation_offset + 12]
            .copy_from_slice(&direction_face_extend.1.to_le_bytes());
        bytes[operation_offset + 12] = direction_reversed;
        bytes[operation_offset + 13] = structural_constant;
        bytes[operation_offset + 14] = start;
        if let Some((side_extents, target_ordinal)) = current_side_extents {
            let profile_normal_offset = operation_offset + 18;
            bytes[profile_normal_offset + 16..profile_normal_offset + 24]
                .copy_from_slice(&1.0f64.to_le_bytes());
            let reference_slots_offset = profile_normal_offset + 24;
            let first_side_extent_offset = if target_ordinal {
                assert_eq!(side_extents.0, 2);
                let target_slot_offset = reference_slots_offset + 6;
                bytes[target_slot_offset] = 1;
                bytes[target_slot_offset + 1..target_slot_offset + 5]
                    .copy_from_slice(&reference_padding.map_or(55, |_| 77_u32).to_le_bytes());
                let target_ordinal_offset = target_slot_offset + 11;
                bytes[target_ordinal_offset..target_ordinal_offset + 4]
                    .copy_from_slice(&0u32.to_le_bytes());
                target_ordinal_offset + 5
            } else {
                reference_slots_offset + 7
            };
            let reference_count_offset = 180;
            bytes.resize(reference_count_offset, 0);
            let second_side_extent_offset = if side_extents.0 == 2 {
                reference_count_offset - 4
            } else {
                first_side_extent_offset + 13
            };
            bytes[first_side_extent_offset..first_side_extent_offset + 4]
                .copy_from_slice(&side_extents.0.to_le_bytes());
            bytes[second_side_extent_offset..second_side_extent_offset + 4]
                .copy_from_slice(&side_extents.1.to_le_bytes());
        }
        if legacy_side_extents.is_some() {
            let reference_count_offset = legacy_reference_count_offset.unwrap_or_else(|| {
                if legacy_side_extents.is_some_and(|(_, widened)| widened)
                    || direction_face_extend.0 == 2
                {
                    272
                } else {
                    252
                }
            }) + legacy_field_shift;
            bytes.resize(reference_count_offset, 0);
        }
        let compact_two_sided =
            legacy_reference_count_offset == Some(283) && direction_face_extend.0 == 2;
        if compact_two_sided {
            for reference_at in [139, 170, 185] {
                let reference_at = reference_at + legacy_field_shift;
                bytes[reference_at] = 1;
                bytes[reference_at + 1..reference_at + 5].copy_from_slice(&55u32.to_le_bytes());
            }
        } else if legacy_side_extents.is_some_and(|(_, widened)| widened)
            || legacy_side_extents.is_some() && direction_face_extend.0 == 2
        {
            for reference_at in [139, 159, 182] {
                let reference_at = reference_at + legacy_field_shift;
                bytes[reference_at] = 1;
                bytes[reference_at + 1..reference_at + 5].copy_from_slice(&55u32.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&reference_padding.map_or(55, |_| 77_u32).to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, kind);
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        if let Some((side_extents, widened)) = legacy_side_extents {
            if widened && direction_face_extend.0 != 2 {
                bytes[106..110].copy_from_slice(&1u32.to_le_bytes());
                bytes[110..114].copy_from_slice(&0u32.to_le_bytes());
            }
            let (first_extent_at, second_extent_at) = if compact_two_sided {
                (166, 181)
            } else if legacy_reference_count_offset == Some(294) {
                (116, 129)
            } else if direction_face_extend.0 == 2 {
                (155, 178)
            } else if widened {
                (116, if side_extents.0 == 2 { 268 } else { 130 })
            } else {
                (106, if side_extents.0 == 2 { 116 } else { 110 })
            };
            let first_extent_at = first_extent_at + legacy_field_shift;
            let second_extent_at = second_extent_at + legacy_field_shift;
            bytes[first_extent_at..first_extent_at + 4]
                .copy_from_slice(&side_extents.0.to_le_bytes());
            bytes[second_extent_at..second_extent_at + 4]
                .copy_from_slice(&side_extents.1.to_le_bytes());
        }
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: "301".into(),
            byte_offset: 0,
        };
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header).unwrap()
    };

    let direct = scope(
        "Extrude",
        1,
        (1, 2),
        0,
        1,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    assert_eq!(
        direct.extrude_prologue,
        Some(DesignExtrudePrologue::ReferenceAware {
            reference: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 28,
            direction_face_extend_values: [1, 2],
            side_extent_discriminators: [1, 0],
            side_extent_discriminator_offsets: [77, 90],
            first_side_target_ordinal: None,
            extent: DesignExtrudeExtent::OneSidedDistance,
            direction_face_extend_offsets: [32, 36],
            direction_reversed: false,
            direction_reversed_offset: 40,
            solid_operation: true,
            solid_operation_offset: 41,
            start: DesignExtrudeStart::ProfilePlane,
            start_offset: 42,
        })
    );
    let referenced = scope(
        "Extrude",
        3,
        (2, 0),
        0,
        1,
        1,
        Some(8),
        false,
        Some(((1, 1), false)),
        None,
        None,
    );
    assert_eq!(
        referenced.extrude_prologue,
        Some(DesignExtrudePrologue::ReferenceAware {
            reference: Some(crate::records::DesignExtrudePrologueReference {
                record_index: 77,
                record_index_offset: 26,
                trailing_zero_count: 8,
                operation_prefix_marker: None,
                operation_prefix_marker_offset: None,
            }),
            operation: DesignExtrudeOperation::Intersect,
            operation_offset: 38,
            direction_face_extend_values: [2, 0],
            side_extent_discriminators: [1, 1],
            side_extent_discriminator_offsets: [87, 100],
            first_side_target_ordinal: None,
            extent: DesignExtrudeExtent::TwoSidedDistance,
            direction_face_extend_offsets: [42, 46],
            direction_reversed: false,
            direction_reversed_offset: 50,
            solid_operation: true,
            solid_operation_offset: 51,
            start: DesignExtrudeStart::OffsetProfilePlane,
            start_offset: 52,
        })
    );
    let two_sided_to_faces = scope(
        "Extrude",
        1,
        (2, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((2, 0), false)),
        None,
        None,
    );
    assert_eq!(
        two_sided_to_faces
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::TwoSidedToFaces)
    );
    let compact_reference = scope(
        "Extrude",
        2,
        (1, 2),
        0,
        1,
        2,
        Some(7),
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    let Some(DesignExtrudePrologue::ReferenceAware {
        reference: Some(reference),
        operation_offset,
        ..
    }) = compact_reference.extrude_prologue
    else {
        panic!("compact referenced Extrude prologue");
    };
    assert_eq!(reference.trailing_zero_count, 7);
    assert_eq!(operation_offset, 37);

    let marked_reference = scope(
        "Extrude",
        1,
        (1, 2),
        0,
        1,
        0,
        Some(8),
        true,
        Some(((1, 0), false)),
        None,
        None,
    );
    let Some(DesignExtrudePrologue::ReferenceAware {
        reference: Some(reference),
        operation_offset,
        ..
    }) = marked_reference.extrude_prologue
    else {
        panic!("marked indexed-reference Extrude prologue");
    };
    assert_eq!(reference.operation_prefix_marker, Some(1));
    assert_eq!(reference.operation_prefix_marker_offset, Some(37));
    assert_eq!(operation_offset, 38);

    let to_face = scope(
        "Extrusion",
        2,
        (1, 1),
        1,
        1,
        2,
        None,
        false,
        Some(((2, 0), false)),
        None,
        None,
    );
    assert_eq!(to_face.kind, "Extrusion");
    let Some(prologue) = to_face.extrude_prologue else {
        panic!("to-face Extrude prologue");
    };
    assert_eq!(prologue.extent(), Some(DesignExtrudeExtent::OneSidedToFace));
    assert!(prologue.direction_reversed());
    assert_eq!(prologue.start(), DesignExtrudeStart::FromFace);

    let same_face_extend_blind = scope(
        "Extrude",
        2,
        (1, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    assert_eq!(
        same_face_extend_blind
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedDistance)
    );
    let same_face_extend_through_all = scope(
        "Extrude",
        2,
        (1, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((4, 0), false)),
        None,
        None,
    );
    assert_eq!(
        same_face_extend_through_all
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedThroughAll)
    );
    let target_ordinal = scope(
        "Extrude",
        2,
        (1, 1),
        0,
        1,
        0,
        None,
        false,
        Some(((2, 0), true)),
        None,
        None,
    );
    assert!(matches!(
        target_ordinal.extrude_prologue,
        Some(DesignExtrudePrologue::ReferenceAware {
            side_extent_discriminators: [2, 0],
            side_extent_discriminator_offsets: [92, 176],
            first_side_target_ordinal: Some(crate::records::DesignExtrudeTargetOrdinal {
                scope_reference_ordinal: 0,
                scope_reference_ordinal_offset: 87,
            }),
            extent: DesignExtrudeExtent::OneSidedToFace,
            ..
        })
    ));

    let shifted_distance = scope(
        "Extrude",
        4,
        (1, 2),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 0), false)),
        None,
    );
    assert_eq!(
        shifted_distance
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedDistance)
    );
    let shifted_symmetric = scope(
        "Extrude",
        4,
        (3, 2),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 0), true)),
        None,
    );
    assert_eq!(
        shifted_symmetric
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::SymmetricDistance)
    );
    assert!(matches!(
        shifted_symmetric.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [116, 130],
            ..
        })
    ));
    let shifted_compact_symmetric = scope(
        "Extrude",
        4,
        (3, 2),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 0), true)),
        Some(283),
    );
    assert!(matches!(
        shifted_compact_symmetric.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker: None,
            operation_prefix_marker_offset: None,
            side_extent_discriminator_offsets: [116, 130],
            extent: Some(DesignExtrudeExtent::SymmetricDistance),
            ..
        })
    ));
    let shifted_marked_symmetric = scope(
        "Extrude",
        4,
        (3, 2),
        0,
        1,
        0,
        None,
        true,
        None,
        Some(((1, 0), true)),
        Some(283),
    );
    assert!(matches!(
        shifted_marked_symmetric.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker: Some(1),
            operation_prefix_marker_offset: Some(27),
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 28,
            direction_face_extend_values: [3, 2],
            side_extent_discriminator_offsets: [117, 131],
            extent: Some(DesignExtrudeExtent::SymmetricDistance),
            direction_face_extend_offsets: [32, 36],
            direction_reversed_offset: 40,
            solid_operation_offset: 41,
            start_offset: 42,
            ..
        })
    ));
    let shifted_offset_profile = scope(
        "Extrude",
        2,
        (1, 2),
        0,
        1,
        1,
        None,
        false,
        None,
        Some(((1, 0), true)),
        Some(262),
    );
    assert!(matches!(
        shifted_offset_profile.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            operation_prefix_marker: None,
            operation_prefix_marker_offset: None,
            operation: DesignExtrudeOperation::Cut,
            operation_offset: 27,
            side_extent_discriminator_offsets: [116, 130],
            extent: Some(DesignExtrudeExtent::OneSidedDistance),
            start: DesignExtrudeStart::OffsetProfilePlane,
            start_offset: 41,
            ..
        })
    ));
    let shifted_two_sided = scope(
        "Extrude",
        2,
        (2, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 1), false)),
        None,
    );
    assert_eq!(
        shifted_two_sided
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::TwoSidedDistance)
    );
    let shifted_compact_two_sided = scope(
        "Extrude",
        2,
        (2, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((1, 1), false)),
        Some(283),
    );
    assert!(matches!(
        shifted_compact_two_sided.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [166, 181],
            extent: Some(DesignExtrudeExtent::TwoSidedDistance),
            ..
        })
    ));

    let shifted_through_all = scope(
        "Extrude",
        2,
        (1, 0),
        1,
        1,
        0,
        None,
        false,
        None,
        Some(((4, 0), false)),
        None,
    );
    assert_eq!(
        shifted_through_all
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedThroughAll)
    );
    let shifted_to_face = scope(
        "Extrude",
        2,
        (1, 1),
        1,
        1,
        0,
        None,
        false,
        None,
        Some(((2, 0), true)),
        None,
    );
    assert_eq!(
        shifted_to_face
            .extrude_prologue
            .and_then(DesignExtrudePrologue::extent),
        Some(DesignExtrudeExtent::OneSidedToFace)
    );
    assert!(matches!(
        shifted_to_face.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            side_extent_discriminator_offsets: [116, 268],
            ..
        })
    ));

    for reference_count_offset in [262, 263] {
        let shifted_compact_to_face = scope(
            "Extrude",
            2,
            (1, 1),
            1,
            1,
            0,
            None,
            false,
            None,
            Some(((2, 0), false)),
            Some(reference_count_offset),
        );
        assert!(matches!(
            shifted_compact_to_face.extrude_prologue,
            Some(DesignExtrudePrologue::LegacyShifted {
                extent: Some(DesignExtrudeExtent::OneSidedToFace),
                side_extent_discriminator_offsets: [106, offset],
                ..
            }) if offset == u64::try_from(reference_count_offset - 4).unwrap()
        ));
    }

    let shifted_symmetric_through_all = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((4, 4), true)),
        Some(294),
    );
    assert!(matches!(
        shifted_symmetric_through_all.extrude_prologue,
        Some(DesignExtrudePrologue::LegacyShifted {
            extent: Some(DesignExtrudeExtent::SymmetricThroughAll),
            side_extent_discriminator_offsets: [116, 129],
            ..
        })
    ));

    let invalid_absent_first_side = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        1,
        0,
        None,
        false,
        None,
        Some(((0, 0), false)),
        None,
    );
    assert_eq!(invalid_absent_first_side.extrude_prologue, None);

    let contradictory_direction_and_sides = scope(
        "Extrude",
        2,
        (2, 0),
        0,
        1,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    );
    assert_eq!(contradictory_direction_and_sides.extrude_prologue, None);

    let unrecognized = scope("Extrude", 2, (3, 0), 0, 1, 0, None, false, None, None, None);
    assert_eq!(unrecognized.kind, "Extrude");
    assert_eq!(unrecognized.extrude_prologue, None);
    assert_eq!(
        scope(
            "Extrude",
            2,
            (3, 0),
            2,
            1,
            0,
            None,
            false,
            Some(((1, 0), false)),
            None,
            None,
        )
        .extrude_prologue,
        None
    );
    let sheet = scope(
        "Extrude",
        2,
        (3, 0),
        0,
        0,
        0,
        None,
        false,
        Some(((1, 0), false)),
        None,
        None,
    )
    .extrude_prologue
    .expect("sheet Extrude prologue");
    assert!(!sheet.solid_operation());
    assert_eq!(
        scope(
            "Extrude",
            2,
            (3, 0),
            0,
            1,
            3,
            None,
            false,
            Some(((1, 0), false)),
            None,
            None,
        )
        .extrude_prologue,
        None
    );
}

#[test]
fn legacy_distance_extrude_scope_decodes_nullable_prefix_forms() {
    let scope = |prefix_present: bool, operation: u32, geometry_kind: u32| {
        let reference_count_offset = if prefix_present { 212 } else { 208 };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"376");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        bytes.resize(reference_count_offset, 0);
        let operation_offset = if prefix_present {
            bytes[20] = 1;
            bytes[21..25].copy_from_slice(&0u32.to_le_bytes());
            25
        } else {
            21
        };
        bytes[operation_offset..operation_offset + 4].copy_from_slice(&operation.to_le_bytes());
        bytes[operation_offset + 4..operation_offset + 8].copy_from_slice(&2u32.to_le_bytes());
        bytes[operation_offset + 8] = 1;
        bytes[operation_offset + 9..operation_offset + 13]
            .copy_from_slice(&geometry_kind.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&55u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        lp_utf16(&mut bytes, "Extrude");
        let mut tail = [0; 78];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&12u32.to_le_bytes());
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 12,
            class_tag: "376".into(),
            byte_offset: 0,
        };
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header).unwrap()
    };

    assert_eq!(
        scope(false, 1, 1).extrude_prologue,
        Some(DesignExtrudePrologue::LegacyDistance {
            prefix_value: None,
            prefix_value_offset: None,
            operation: DesignExtrudeOperation::Join,
            operation_offset: 21,
            extent_kind: 2,
            extent_kind_offset: 25,
            direction_reversed: true,
            direction_reversed_offset: 29,
            geometry_kind: 1,
            geometry_kind_offset: 30,
        })
    );
    assert_eq!(
        scope(true, 4, 0).extrude_prologue,
        Some(DesignExtrudePrologue::LegacyDistance {
            prefix_value: Some(0),
            prefix_value_offset: Some(21),
            operation: DesignExtrudeOperation::NewBody,
            operation_offset: 25,
            extent_kind: 2,
            extent_kind_offset: 29,
            direction_reversed: true,
            direction_reversed_offset: 33,
            geometry_kind: 0,
            geometry_kind_offset: 34,
        })
    );

    let mut invalid_extent_kind = scope(false, 1, 1).extrude_prologue.unwrap();
    let DesignExtrudePrologue::LegacyDistance { extent_kind, .. } = &mut invalid_extent_kind else {
        unreachable!("the fixture constructs the early distance-only layout");
    };
    *extent_kind = 1;
    assert_eq!(invalid_extent_kind.extent(), None);
}

#[test]
fn coil_scope_discriminators_use_the_fixed_scope_prologue() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.resize(120, 0);
    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    bytes[24] = 1;
    bytes[26..30].copy_from_slice(&2u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&3u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&2u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&4u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "SpirePrimitive");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "301".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("Coil scope");
    assert_eq!(scope.coil_operation, Some(DesignExtrudeOperation::Cut));
    assert_eq!(scope.coil_operation_offset, Some(20));
    assert_eq!(scope.coil_extent, Some(DesignCoilExtent::HeightPitch));
    assert_eq!(scope.coil_extent_offset, Some(30));
    assert_eq!(
        scope.coil_section,
        Some(DesignCoilSection::ExternalTriangle)
    );
    assert_eq!(scope.coil_section_offset, Some(92));
    assert_eq!(
        scope.coil_section_placement,
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_section_placement_offset, Some(107));
    assert_eq!(scope.coil_clockwise, Some(true));
    assert_eq!(scope.coil_clockwise_offset, Some(24));
}

#[test]
fn compact_coil_scope_uses_its_own_closed_discriminators() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"353");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    bytes.resize(120, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[26..30].copy_from_slice(&4u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&1u32.to_le_bytes());
    let references: [u32; 8] = [6645, 6650, 6653, 6656, 6659, 6662, 6665, 6668];
    bytes.extend_from_slice(&(references.len() as u32).to_le_bytes());
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&310u32.to_le_bytes());
    lp_utf16(&mut bytes, "CoilPrimitive");
    let mut tail = [0; 78];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    tail[31..35].copy_from_slice(&309u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 6644,
        class_tag: "353".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("compact Coil scope");
    assert_eq!(scope.coil_operation, Some(DesignExtrudeOperation::NewBody));
    assert_eq!(scope.coil_extent, Some(DesignCoilExtent::RevolutionsHeight));
    assert_eq!(scope.coil_section, Some(DesignCoilSection::Circular));
    assert_eq!(
        scope.coil_section_placement,
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_clockwise, Some(false));

    for (placement_code, placement) in [
        (1u32, DesignCoilSectionPlacement::Inside),
        (2u32, DesignCoilSectionPlacement::Center),
        (3u32, DesignCoilSectionPlacement::Outside),
    ] {
        for (section_code, section) in [
            (1u32, DesignCoilSection::Circular),
            (2u32, DesignCoilSection::Square),
            (3u32, DesignCoilSection::ExternalTriangle),
            (4u32, DesignCoilSection::InternalTriangle),
        ] {
            bytes[92..96].copy_from_slice(&placement_code.to_le_bytes());
            bytes[107..111].copy_from_slice(&section_code.to_le_bytes());
            let parsed =
                parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
                    .expect("compact Coil scope");
            assert_eq!(parsed.coil_section, Some(section));
            assert_eq!(parsed.coil_section_placement, Some(placement));
        }
    }

    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let unsupported = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("unsupported Coil operation remains a native scope");
    assert!(unsupported.coil_operation.is_none());
}

#[test]
fn compact_coil_new_body_scope_accepts_unlinked_state_trailer() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"338");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    bytes.resize(228, 0);
    bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
    bytes[24] = 0;
    bytes[26..30].copy_from_slice(&4u32.to_le_bytes());
    bytes[30..34].copy_from_slice(&1u32.to_le_bytes());
    bytes[92..96].copy_from_slice(&1u32.to_le_bytes());
    bytes[107..111].copy_from_slice(&1u32.to_le_bytes());
    let references: [u32; 8] = [6645, 6650, 6653, 6656, 6659, 6662, 6665, 6668];
    bytes.extend_from_slice(&(references.len() as u32).to_le_bytes());
    for reference in references {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    lp_utf16(&mut bytes, "CoilPrimitive");
    let mut tail = [0; 88];
    tail[0..4].copy_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&tail);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"259");
    bytes.extend_from_slice(&6644u32.to_le_bytes());
    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 6644,
        class_tag: "338".into(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("compact Coil new-body scope");
    assert_eq!(scope.frame_length, 442);
    assert_eq!(scope.kind, "CoilPrimitive");
    assert_eq!(scope.coil_operation, Some(DesignExtrudeOperation::NewBody));
    assert_eq!(scope.history_state_id, Some(3));
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, 0);
}

#[test]
fn long_coil_scope_discriminators_use_the_ten_reference_envelope() {
    let scope = |frame_length: usize, operation: u32| {
        let reference_members: [u32; 10] =
            [1001, 1002, 1003, 1004, 1005, 1006, 1007, 1008, 1009, 1010];
        let kind = "CoilPrimitive";
        let kind_length = 4 + kind.encode_utf16().count() * 2;
        let tail_length = if frame_length == 572 { 76 } else { 78 };
        let kind_at = frame_length - tail_length - kind_length;
        let reference_count_at = kind_at - 4 - 4 - reference_members.len() * 11;
        let mut bytes = vec![0; reference_count_at];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"345");
        bytes[7..11].copy_from_slice(&331u32.to_le_bytes());
        bytes[22..26].copy_from_slice(&operation.to_le_bytes());
        bytes[26..30].copy_from_slice(&1u32.to_le_bytes());
        for (offset, target) in [(30usize, 1005u32), (41, 1009)] {
            bytes[offset] = 1;
            bytes[offset + 1..offset + 5].copy_from_slice(&target.to_le_bytes());
        }
        if matches!(frame_length, 572 | 578) {
            let matrix: [f64; 16] = [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ];
            for (ordinal, value) in matrix.into_iter().enumerate() {
                bytes[77 + ordinal * 8..85 + ordinal * 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&(reference_members.len() as u32).to_le_bytes());
        for reference in reference_members {
            bytes.push(1);
            bytes.extend_from_slice(&reference.to_le_bytes());
            bytes.extend_from_slice(&[0; 6]);
        }
        bytes.extend_from_slice(&310u32.to_le_bytes());
        lp_utf16(&mut bytes, kind);
        let mut tail = vec![0; tail_length];
        tail[0..4].copy_from_slice(&1u32.to_le_bytes());
        tail[31..35].copy_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(&tail);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&331u32.to_le_bytes());
        assert_eq!(bytes.len(), frame_length + 11);
        let header = DesignRecordHeader {
            id: "generated:scope-header#0".into(),
            record_index: 331,
            class_tag: "345".into(),
            byte_offset: 0,
        };
        parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
            .expect("long Coil scope")
    };

    let boolean = scope(450, 1);
    assert_eq!(boolean.coil_operation, Some(DesignExtrudeOperation::Join));
    assert_eq!(boolean.coil_operation_offset, Some(22));
    assert_eq!(boolean.coil_extent, None);
    assert_eq!(boolean.coil_section, Some(DesignCoilSection::Circular));
    assert_eq!(boolean.coil_section_offset, None);
    assert_eq!(
        boolean.coil_section_placement,
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(boolean.coil_section_placement_offset, None);
    assert_eq!(boolean.coil_clockwise, Some(false));
    assert_eq!(boolean.coil_clockwise_offset, None);

    let new_body = scope(578, 2);
    assert_eq!(
        new_body.coil_operation,
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(new_body.coil_operation_offset, Some(22));
    let transform = new_body.coil_transform.expect("long Coil placement");
    assert_eq!(transform.transform_offset, 77);
    assert_eq!(
        transform.transform,
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );

    for (operation, expected) in [
        (1, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
        (3, DesignExtrudeOperation::Intersect),
    ] {
        let boolean = scope(572, operation);
        assert_eq!(boolean.coil_operation, Some(expected));
        let transform = boolean.coil_transform.expect("572-byte Coil placement");
        assert_eq!(transform.transform_offset, 77);
        assert_eq!(
            transform.transform,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
        );
    }
}
