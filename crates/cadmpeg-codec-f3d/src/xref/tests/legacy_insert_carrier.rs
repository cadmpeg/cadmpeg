// SPDX-License-Identifier: Apache-2.0
#[test]
fn legacy_typed_insert_carriers_bind_by_record_and_equal_placement() {
    let matrix = [
        [0.0, -1.0, 0.0, 2.0],
        [1.0, 0.0, 0.0, 3.0],
        [0.0, 0.0, 1.0, 4.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let identity = cadmpeg_ir::transform::Transform::identity().rows();
    let role = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
    let encoded_role = crate::bytes::lp_utf16_bytes(role).unwrap();
    for (transform, expected) in [(None, identity), (Some(matrix), matrix)] {
        let mut bytes = vec![0xee; 5];
        let carrier_at = bytes.len();
        bytes.extend(crate::test_support::legacy_occurrence_record(
            role, 10, transform,
        ));
        let relation_at = bytes.len();
        let (decoded, role_offset, transform_offset) =
            super::super::legacy_typed_component_insert_carrier(
                &bytes,
                carrier_at,
                relation_at,
                10,
                expected,
            )
            .expect("carrier with the scope placement");
        assert_eq!(decoded, role);
        assert_eq!(
            &bytes[role_offset - 4..role_offset - 4 + encoded_role.len()],
            encoded_role
        );
        assert_eq!(transform_offset.is_some(), transform.is_some());
        if let Some(offset) = transform_offset {
            assert_eq!(bytes[offset - 1], 0);
            assert_eq!(&bytes[offset + 8..offset + 16], &matrix[0][1].to_le_bytes());
        }
        assert!(super::super::legacy_typed_component_insert_carrier(
            &bytes,
            carrier_at,
            relation_at,
            11,
            expected
        )
        .is_none());
        let other = if transform.is_some() {
            identity
        } else {
            matrix
        };
        assert!(super::super::legacy_typed_component_insert_carrier(
            &bytes,
            carrier_at,
            relation_at,
            10,
            other
        )
        .is_none());
        assert!(super::super::legacy_typed_component_insert_carrier(
            &bytes,
            carrier_at,
            relation_at - 1,
            10,
            expected
        )
        .is_none());
    }
}
