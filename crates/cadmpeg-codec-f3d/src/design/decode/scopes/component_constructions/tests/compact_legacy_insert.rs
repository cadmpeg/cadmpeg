// SPDX-License-Identifier: Apache-2.0
//! The 393- and 399-byte `Component Insert` frames whose 57-byte relation
//! names a compact legacy typed occurrence placement.

use cadmpeg_core::decode::u64_from_index;

use crate::records::feature::scope::DesignParameterScope;

const ROLE: &str = "b2231f72-46dc-40fa-b8e8-10cd208d7df8";
const NULL_GUID: &str = "00000000-0000-0000-0000-000000000000";
const TRANSFORM: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 2.5],
    [0.0, 0.0, 0.0, 1.0],
];

fn header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

fn legacy_target(bytes: &mut Vec<u8>) {
    bytes.push(1);
    bytes.extend_from_slice(&159_u64.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0x0102_0304_0506_0708_u64.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"E03784ED-5E19-4E14-B9F2-3B07017018CD");
    bytes.push(0);
}

/// A compact legacy typed placement with identity flag `0`.
fn compact_carrier(ordinal: u32, matrix: [[f64; 4]; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    header(&mut bytes, *b"298", 10);
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    legacy_target(&mut bytes);
    bytes.extend_from_slice(&11_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&ordinal.to_le_bytes());
    for guid in [
        "11111111-2222-3333-4444-555555555555",
        "66666666-7777-8888-9999-aaaaaaaaaaaa",
    ] {
        bytes.extend(crate::bytes::lp_utf16_bytes(guid).unwrap());
    }
    bytes.push(0);
    legacy_target(&mut bytes);
    bytes.push(0);
    for value in matrix.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(crate::bytes::lp_utf16_bytes(ROLE).unwrap());
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

/// The carrier, its 57-byte relation, and one scope frame of `frame_length`
/// bytes with the 404-byte (393) or 400-byte (399) prefix.
fn fixture(
    frame_length: usize,
    carrier: &[u8],
    scope_transform: [[f64; 4]; 4],
) -> (Vec<u8>, DesignParameterScope, usize) {
    let mut bytes = carrier.to_vec();
    let relation_at = bytes.len();
    header(&mut bytes, *b"304", 20);
    bytes.extend_from_slice(&[0; 10]);
    for (ordinal, reference) in [10_u32, 11, 30].into_iter().enumerate() {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0, [8, 7, 6][ordinal]));
    }
    assert_eq!(bytes.len(), relation_at + 57);
    header(&mut bytes, *b"260", 20);
    let scope_at = bytes.len();
    let shift = if frame_length == 393 { 4 } else { 0 };
    bytes.resize(scope_at + frame_length, 0);
    let mut prefix = Vec::new();
    header(&mut prefix, *b"388", 30);
    if shift != 0 {
        prefix.extend_from_slice(&[0; 9]);
        prefix.extend_from_slice(&[1, 0, 0, 0, 0]);
        prefix.extend_from_slice(&[0; 4]);
    } else {
        prefix.extend_from_slice(&[0; 14]);
    }
    prefix.extend_from_slice(&0x1122_3344_5566_7788_u64.to_le_bytes());
    prefix.extend_from_slice(&[0; 4]);
    prefix.push(1);
    prefix.extend_from_slice(&20_u32.to_le_bytes());
    prefix.extend_from_slice(&[0; 6]);
    prefix.extend_from_slice(&[1, 0]);
    for value in scope_transform.into_iter().flatten() {
        prefix.extend_from_slice(&value.to_le_bytes());
    }
    prefix.extend(crate::bytes::lp_utf16_bytes(NULL_GUID).unwrap());
    assert_eq!(prefix.len(), 254 + shift);
    bytes[scope_at..scope_at + prefix.len()].copy_from_slice(&prefix);
    header(&mut bytes, *b"260", 30);
    let reference_at = scope_at + 38 + shift;
    let scope = DesignParameterScope::try_new(
        crate::records::feature::scope::DesignParameterScopeDraft {
            id: "f3d:Design/BulkStream.dat:design-parameter-scope#30".into(),
            byte_offset: u64_from_index(scope_at),
            class_tag: crate::records::references::DesignClassTag::try_from("388".to_owned())
                .unwrap(),
            record_index: 30,
            frame_length: u64_from_index(frame_length),
            kind_offset: 0,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            label: None,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: u64_from_index(scope_at) + 9,
            reference_members: crate::records::identity::ReferenceRun::from_columns(
                vec![20],
                vec![u64_from_index(reference_at)],
                "reference_members",
            )
            .unwrap(),
            payload: crate::records::feature::scope::DesignFeatureKind::ComponentInsert
                .try_into()
                .unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            deleted_members: None,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "260".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: u64_from_index(scope_at + frame_length),
        }
        .with_fixture_layout(),
    )
    .unwrap();
    (bytes, scope, scope_at)
}

fn construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<crate::records::feature::assembly_features::DesignComponentInsertConstruction> {
    super::super::exact_component_insert_construction(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
    )
    .unwrap()
}

#[test]
fn compact_legacy_insert_frames_join_their_carrier_role_and_transform() {
    let carrier = compact_carrier(2, TRANSFORM);
    assert_eq!(carrier.len(), 527);
    for (frame_length, transform_delta) in [(393, 54), (399, 50)] {
        let (bytes, scope, scope_at) = fixture(frame_length, &carrier, TRANSFORM);
        let construction = construction(&bytes, &scope).expect("compact legacy insert");
        assert_eq!(construction.relation_record_index, 20);
        assert_eq!(construction.carrier_record_index, 10);
        assert_eq!(
            construction.occurrence_identity,
            Some(0x1122_3344_5566_7788)
        );
        assert_eq!(construction.neutron_role, ROLE);
        assert_eq!(*construction.transform(), TRANSFORM.try_into().unwrap());
        assert_eq!(
            construction.transform_offset(),
            Some(u64_from_index(scope_at + transform_delta))
        );
        assert_eq!(construction.carrier_transform_offset(), Some(307));
        assert!(construction.neutron_role_offset > 307);
    }
}

#[test]
fn compact_legacy_insert_frames_refuse_other_carriers_and_prefixes() {
    let moved = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 2.25],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for frame_length in [393, 399] {
        // The carrier matrix is not the scope transform.
        let (bytes, scope, _) = fixture(frame_length, &compact_carrier(1, moved), TRANSFORM);
        assert!(construction(&bytes, &scope).is_none());
        // A zero ordinal does not close the compact envelope.
        let (bytes, scope, _) = fixture(frame_length, &compact_carrier(0, TRANSFORM), TRANSFORM);
        assert!(construction(&bytes, &scope).is_none());
        // The relation names another scope.
        let (mut bytes, scope, _) =
            fixture(frame_length, &compact_carrier(1, TRANSFORM), TRANSFORM);
        bytes[527 + 47] = 31;
        assert!(construction(&bytes, &scope).is_none());
    }
    // The 393-byte frame has the shifted prefix and the 399-byte frame the
    // fourteen-zero prefix; the other prefix does not close either frame.
    let carrier = compact_carrier(1, TRANSFORM);
    let (mut bytes, scope, scope_at) = fixture(393, &carrier, TRANSFORM);
    bytes[scope_at + 20] = 0;
    assert!(construction(&bytes, &scope).is_none());
    let (mut bytes, scope, scope_at) = fixture(399, &carrier, TRANSFORM);
    bytes[scope_at + 11] = 1;
    assert!(construction(&bytes, &scope).is_none());
}
