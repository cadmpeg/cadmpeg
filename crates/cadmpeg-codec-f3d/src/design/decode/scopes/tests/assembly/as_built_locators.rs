// SPDX-License-Identifier: Apache-2.0

use super::*;

/// The placement-ordered `As-built` form stores a u32 additional-owner count at
/// offset 43 and that many owner references after it, then the locator count
/// and two locator references; its locators are wide operand-path locators.
/// Its owners are the three offsets, the angle, and the additional owners. The
/// 378-byte form has no additional owners, the 422-byte form two, and the 444-byte
/// form three.
#[test]
fn as_built_378_alignment_reads_wide_locators() {
    use crate::records::feature::assembly::TYPED_OCCURRENCE_PATH_TYPE_GUID;
    use compact_wide::exact_assembly_alignment_with_types;
    use compact_wide::{push_header, push_wide_operand_path};
    let scope_record_index = 10_u32;
    let scope_of = |kind, count: u32| {
        let frame_length = 378 + 22 * u64::from(count);
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#0",
            kind,
            scope_record_index,
        );
        scope.class_tag = DesignClassTag::try_from("328".to_owned()).unwrap();
        scope
            .try_edit(|draft| {
                draft.frame_length = frame_length;
                draft.reference_members = ReferenceRun::unlocated(
                    (if count == 0 {
                        [80, 81, 82, 83, 84, 50, 51, 52, 53]
                    } else {
                        [80, 50, 51, 52, 53, 81, 82, 83, 84]
                    })
                    .into_iter()
                    .chain(54..54 + count)
                    .collect::<Vec<_>>(),
                );
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope.paired_class_tag = DesignClassTag::try_from("264".to_owned()).unwrap();
        scope
    };
    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: evaluated_value_offset - 40,
            frame_length: 103,
            class_tag: DesignClassTag::try_from("321".to_owned()).unwrap(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        })
        .unwrap()
    };
    let owners_of = |count: u32| {
        [(1.0, 501), (2.0, 502), (3.0, 503), (0.25, 504)]
            .into_iter()
            .chain((0..count).map(|ordinal| (-0.5 + f64::from(ordinal), 505 + u64::from(ordinal))))
            .zip(0_u32..)
            .map(|((value, at), ordinal)| owner(50 + ordinal, ordinal, value, at))
            .collect::<Vec<_>>()
    };
    let bytes_of = |count: u32| {
        let extra = 11 * usize::try_from(count).unwrap();
        let frame_length = 378 + 2 * extra;
        let mut bytes = vec![0_u8; frame_length];
        bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"328");
        bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
        bytes[43..47].copy_from_slice(&count.to_le_bytes());
        for ordinal in 0..count {
            let at = 47 + 11 * usize::try_from(ordinal).unwrap();
            bytes[at] = 1;
            bytes[at + 1..at + 5].copy_from_slice(&(54 + ordinal).to_le_bytes());
        }
        bytes[47 + extra..51 + extra].copy_from_slice(&2_u32.to_le_bytes());
        for (at, locator_record_index) in [(51 + extra, 64_u32), (62 + extra, 67_u32)] {
            bytes[at] = 1;
            bytes[at + 1..at + 5].copy_from_slice(&locator_record_index.to_le_bytes());
        }
        push_header(&mut bytes, b"264", scope_record_index);
        bytes.resize(frame_length + 30, 0);
        push_wide_operand_path(
            &mut bytes,
            64,
            scope_record_index,
            "11111111-1111-1111-1111-111111111111",
        );
        let second = bytes.len();
        push_wide_operand_path(
            &mut bytes,
            67,
            scope_record_index,
            "66666666-6666-6666-6666-666666666666",
        );
        bytes[second + 57..second + 61].copy_from_slice(&901_u32.to_le_bytes());
        push_header(&mut bytes, b"300", 71);
        bytes
    };

    let locator = "2E3AC990-483D-4A55-8662-C064B976A957";
    let types = std::collections::HashMap::from([
        (64, (locator, 5)),
        (65, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
        (67, (locator, 5)),
        (68, (TYPED_OCCURRENCE_PATH_TYPE_GUID, 4)),
    ]);
    for count in [0, 2, 3] {
        let bytes = bytes_of(count);
        let owners = owners_of(count);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let read = |bytes: &[u8], kind| {
            exact_assembly_alignment_with_types(
                bytes,
                &records,
                &scope_of(kind, count),
                &owners,
                &types,
            )
            .unwrap()
        };
        let alignment = read(&bytes, DesignFeatureKind::AsBuilt)
            .unwrap_or_else(|| panic!("As-built alignment with {count} additional owners"));
        assert_eq!(alignment.angle(), 0.25);
        assert_eq!(alignment.offset(), [1.0, 2.0, 3.0]);
        assert_eq!(alignment.owners.len(), 4);
        let serialized = serde_json::to_value(&alignment).unwrap();
        let restored: crate::records::feature::assembly::DesignAssemblyAlignment =
            serde_json::from_value(serialized).unwrap();
        assert_eq!(restored, alignment);
        let frames = alignment.operand_frames().expect("wide locator frames");
        assert!(frames
            .iter()
            .all(|frame| frame.transform.rows()
                == cadmpeg_ir::transform::Transform::identity().rows()));
        let paths = alignment.operand_path_refs().expect("both operand paths");
        assert_eq!(
            paths.map(|path| path.occurrence_guids()[0].as_str().to_owned()),
            [
                "11111111-1111-1111-1111-111111111111".to_owned(),
                "66666666-6666-6666-6666-666666666666".to_owned()
            ]
        );

        // An `Assemble` scope of the same length has no wide locators.
        let assemble = read(&bytes, DesignFeatureKind::Assemble);
        assert!(assemble.is_none_or(|alignment| alignment.operand_path_refs().is_none()));

        // A stored count other than the frame's has no alignment.
        let mut miscounted = bytes.clone();
        miscounted[43..47].copy_from_slice(&(count + 1).to_le_bytes());
        assert!(read(&miscounted, DesignFeatureKind::AsBuilt).is_none());

        if count != 0 {
            let mut wrong_owner = bytes.clone();
            wrong_owner[48..52].copy_from_slice(&99_u32.to_le_bytes());
            assert!(read(&wrong_owner, DesignFeatureKind::AsBuilt).is_none());
        } else {
            for references in [
                vec![50, 51, 52, 53],
                vec![80, 50, 51, 52, 53, 81, 82, 83, 84],
            ] {
                let mut wrong_table = scope_of(DesignFeatureKind::AsBuilt, count);
                wrong_table
                    .try_edit(|draft| {
                        draft.reference_members = ReferenceRun::unlocated(references);
                        draft.layout_fixture_references();
                        draft.layout_fixture_tail();
                    })
                    .unwrap();
                assert!(exact_assembly_alignment_with_types(
                    &bytes,
                    &records,
                    &wrong_table,
                    &owners,
                    &types
                )
                .unwrap()
                .is_none());
            }
        }
    }
}

#[test]
fn as_built_alignment_uses_locator_frames_and_parameter_owner_lanes() {
    let scope_record_index = 10_u32;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#0",
        DesignFeatureKind::AsBuilt,
        scope_record_index,
    );
    scope.class_tag = DesignClassTag::try_from("439".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 399;
            draft.reference_members = ReferenceRun::unlocated(vec![50, 51, 52, 53]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope.paired_class_tag = DesignClassTag::try_from("262".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.paired_byte_offset = 399;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();

    let owner = |record_index, local_ordinal, evaluated_value, evaluated_value_offset| {
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
            byte_offset: (evaluated_value_offset) - 40,
            frame_length: 103,
            class_tag: DesignClassTag::try_from("321".to_owned()).unwrap(),
            record_index,
            scope_record_index,
            local_ordinal,
            evaluated_value,
            evaluated_value_offset,
            parameter_record_index: record_index + 1,
            owned_ordinal: local_ordinal,
            variant: None,
            companion_record_index: record_index + 2,
        })
        .unwrap()
    };
    let owners = [
        owner(50, 0, 0.25, 501),
        owner(51, 1, 1.0, 502),
        owner(52, 2, 2.0, 503),
        owner(53, 3, 3.0, 504),
    ];

    let mut bytes = vec![0_u8; 399];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"439");
    bytes[7..11].copy_from_slice(&scope_record_index.to_le_bytes());
    bytes[47..51].copy_from_slice(&2_u32.to_le_bytes());
    for (at, locator_record_index) in [(51, 64_u32), (62, 67_u32)] {
        bytes[at] = 1;
        bytes[at + 1..at + 5].copy_from_slice(&locator_record_index.to_le_bytes());
    }
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"262");
    bytes.extend_from_slice(&scope_record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 17]);
    let first_locator_at = bytes.len();
    append_as_built_path_envelope(
        &mut bytes,
        scope_record_index,
        64,
        70,
        "11111111-1111-1111-1111-111111111111",
        [1.0, 2.0, 3.0],
    );
    let second_locator_at = bytes.len();
    append_as_built_path_envelope(
        &mut bytes,
        scope_record_index,
        67,
        80,
        "22222222-2222-2222-2222-222222222222",
        [4.0, 5.0, 6.0],
    );
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"396");
    bytes.extend_from_slice(&90_u32.to_le_bytes());

    let alignment = exact_assembly_alignment(
        &bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("exact As-built alignment");
    assert_eq!(alignment.angle(), 0.25);
    assert_eq!(alignment.offset(), [1.0, 2.0, 3.0]);
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.value)
            .collect::<Vec<_>>(),
        [50, 51, 52, 53]
    );
    assert_eq!(
        alignment
            .owners
            .iter()
            .map(|owner| owner.offset)
            .collect::<Vec<_>>(),
        [Some(501), Some(502), Some(503), Some(504)]
    );
    let frames = alignment.operand_frames().expect("locator transforms");
    assert_eq!(
        frames
            .each_ref()
            .map(|frame| (frame.reference_record_index, frame.transform[0][3])),
        [(70, 1.0), (80, 4.0)]
    );
    assert_eq!(
        frames.each_ref().map(|frame| frame.transform_offset),
        [
            Some(u64::try_from(first_locator_at + 33).expect("fixture offset fits u64")),
            Some(u64::try_from(second_locator_at + 33).expect("fixture offset fits u64")),
        ]
    );
    let paths = alignment.operand_paths().expect("locator occurrence paths");
    assert_eq!(
        paths.each_ref().map(|path| (
            path.link().locator_record_index,
            path.occurrence_guids()[0].as_str()
        )),
        [
            (64, "11111111-1111-1111-1111-111111111111"),
            (67, "22222222-2222-2222-2222-222222222222"),
        ]
    );

    let mut invalid_transform = bytes.clone();
    invalid_transform[first_locator_at + 33..first_locator_at + 41]
        .copy_from_slice(&2.0_f64.to_le_bytes());
    let incomplete = exact_assembly_alignment(
        &invalid_transform,
        &crate::design::test_support::indexed_record_offsets_for_test(&invalid_transform),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("alignment scalars remain exact");
    assert_eq!(incomplete.operand_frames(), None);
    assert_eq!(incomplete.operand_paths(), None);

    let mut duplicate_reference = bytes;
    duplicate_reference[63..67].copy_from_slice(&64_u32.to_le_bytes());
    let incomplete = exact_assembly_alignment(
        &duplicate_reference,
        &crate::design::test_support::indexed_record_offsets_for_test(&duplicate_reference),
        &scope,
        &owners,
    )
    .unwrap()
    .expect("alignment scalars remain exact");
    assert_eq!(incomplete.operand_frames(), None);
    assert_eq!(incomplete.operand_paths(), None);
}
