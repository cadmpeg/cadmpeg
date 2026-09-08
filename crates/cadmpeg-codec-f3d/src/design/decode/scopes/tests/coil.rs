// SPDX-License-Identifier: Apache-2.0

use super::prelude::{
    lp_utf16, parse_parameter_scope, DesignCoilExtent, DesignCoilSection,
    DesignCoilSectionPlacement, DesignExtrudeOperation, DesignRecordHeader, IndexedRecordOffsets,
};

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
        class_tag: crate::records::DesignClassTag::try_from("301".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .expect("Coil scope");
    assert_eq!(scope.coil_operation(), Some(DesignExtrudeOperation::Cut));
    assert_eq!(scope.coil_operation_offset(), Some(20));
    assert_eq!(scope.coil_extent(), Some(DesignCoilExtent::HeightPitch));
    assert_eq!(scope.coil_extent_offset(), Some(30));
    assert_eq!(
        scope.coil_section(),
        Some(DesignCoilSection::ExternalTriangle)
    );
    assert_eq!(scope.coil_section_offset(), Some(92));
    assert_eq!(
        scope.coil_section_placement(),
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_section_placement_offset(), Some(107));
    assert_eq!(scope.coil_clockwise(), Some(true));
    assert_eq!(scope.coil_clockwise_offset(), Some(24));
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
        class_tag: crate::records::DesignClassTag::try_from("353".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .expect("compact Coil scope");
    assert_eq!(
        scope.coil_operation(),
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(
        scope.coil_extent(),
        Some(DesignCoilExtent::RevolutionsHeight)
    );
    assert_eq!(scope.coil_section(), Some(DesignCoilSection::Circular));
    assert_eq!(
        scope.coil_section_placement(),
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(scope.coil_clockwise(), Some(false));

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
            let parsed = parse_parameter_scope(
                &bytes,
                &IndexedRecordOffsets::build(&bytes),
                header.record_index,
                &header.class_tag,
                header.byte_offset,
            )
            .expect("compact Coil scope");
            assert_eq!(parsed.coil_section(), Some(section));
            assert_eq!(parsed.coil_section_placement(), Some(placement));
        }
    }

    bytes[20..24].copy_from_slice(&2u32.to_le_bytes());
    let unsupported = parse_parameter_scope(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .expect("unsupported Coil operation remains a native scope");
    assert!(unsupported.coil_operation().is_none());
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
        class_tag: crate::records::DesignClassTag::try_from("338".to_owned()).unwrap(),
        byte_offset: 0,
    };

    let scope = parse_parameter_scope(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        header.record_index,
        &header.class_tag,
        header.byte_offset,
    )
    .expect("compact Coil new-body scope");
    assert_eq!(scope.frame_length, 442);
    assert_eq!(
        scope.kind(),
        crate::records::feature::DesignFeatureKind::CoilPrimitive
    );
    assert_eq!(
        scope.coil_operation(),
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(scope.history_state_id, Some(3));
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, None);
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
            class_tag: crate::records::DesignClassTag::try_from("345".to_owned()).unwrap(),
            byte_offset: 0,
        };
        parse_parameter_scope(
            &bytes,
            &IndexedRecordOffsets::build(&bytes),
            header.record_index,
            &header.class_tag,
            header.byte_offset,
        )
        .expect("long Coil scope")
    };

    let boolean = scope(450, 1);
    assert_eq!(boolean.coil_operation(), Some(DesignExtrudeOperation::Join));
    assert_eq!(boolean.coil_operation_offset(), Some(22));
    assert_eq!(boolean.coil_extent(), None);
    assert_eq!(boolean.coil_section(), Some(DesignCoilSection::Circular));
    assert_eq!(boolean.coil_section_offset(), None);
    assert_eq!(
        boolean.coil_section_placement(),
        Some(DesignCoilSectionPlacement::Inside)
    );
    assert_eq!(boolean.coil_section_placement_offset(), None);
    assert_eq!(boolean.coil_clockwise(), Some(false));
    assert_eq!(boolean.coil_clockwise_offset(), None);

    let new_body = scope(578, 2);
    assert_eq!(
        new_body.coil_operation(),
        Some(DesignExtrudeOperation::NewBody)
    );
    assert_eq!(new_body.coil_operation_offset(), Some(22));
    let transform = new_body.coil_transform().expect("long Coil placement");
    assert_eq!(transform.transform_offset, 77);
    assert_eq!(
        transform.transform,
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
        .try_into()
        .unwrap()
    );

    for (operation, expected) in [
        (1, DesignExtrudeOperation::Join),
        (2, DesignExtrudeOperation::Cut),
        (3, DesignExtrudeOperation::Intersect),
    ] {
        let boolean = scope(572, operation);
        assert_eq!(boolean.coil_operation(), Some(expected));
        let transform = boolean.coil_transform().expect("572-byte Coil placement");
        assert_eq!(transform.transform_offset, 77);
        assert_eq!(
            transform.transform,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]
            .try_into()
            .unwrap()
        );
    }
}
