// SPDX-License-Identifier: Apache-2.0
//! The compact legacy typed occurrence placement and the precedence of a
//! scope transform over the placement that its carrier stores.

const ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
const MATRIX: [[f64; 4]; 4] = [
    [0.0, -1.0, 0.0, 2.0],
    [1.0, 0.0, 0.0, 3.0],
    [0.0, 0.0, 1.0, 4.0],
    [0.0, 0.0, 0.0, 1.0],
];
/// Offset of the instance discriminator in the legacy envelope.
const DISCRIMINATOR: usize = 84;
/// Offset of the two GUIDs in the wide legacy envelope.
const WIDE_GUIDS: usize = 97;

/// The compact envelope: the wide envelope with `u8 0` and `ordinal` after the
/// instance discriminator in place of `u32 1`, `u8 0`, and `u32 1`.
fn compact_record(entity_id: u64, ordinal: u32, transform: Option<[[f64; 4]; 4]>) -> Vec<u8> {
    let wide = super::legacy_occurrence_record(ROLE, entity_id, transform);
    let mut bytes = wide[..DISCRIMINATOR + 4].to_vec();
    bytes.push(0);
    bytes.extend_from_slice(&ordinal.to_le_bytes());
    bytes.extend_from_slice(&wide[WIDE_GUIDS..]);
    bytes
}

#[test]
fn compact_legacy_placements_decode_identity_and_matrix_forms() {
    let identity = compact_record(10, 1, None);
    let matrix = compact_record(11, 2, Some(MATRIX));
    assert_eq!(identity.len(), 399);
    assert_eq!(matrix.len(), 527);
    assert_eq!(matrix[DISCRIMINATOR + 4], 0);
    let mut bytes = identity;
    bytes.extend(matrix);
    bytes.extend(super::legacy_occurrence_record(ROLE, 12, Some(MATRIX)));

    let placements =
        super::super::occurrence_placements(&bytes, &super::super::indexed_records(&bytes), None);

    assert_eq!(
        placements
            .iter()
            .map(|placement| placement.carrier_record_index)
            .collect::<Vec<_>>(),
        [10, 11, 12]
    );
    assert_eq!(
        super::super::occurrence_transforms(&placements, ROLE),
        vec![None, Some(MATRIX), Some(MATRIX)]
    );
}

#[test]
fn compact_legacy_placements_refuse_a_zero_ordinal_and_a_wide_mix() {
    for ordinal in [0, 1] {
        let record = compact_record(10, ordinal, Some(MATRIX));
        assert_eq!(
            super::super::occurrence_placement(&record, None)
                .unwrap()
                .is_some(),
            ordinal == 1
        );
    }
    // The terminator of the compact path is zero; `01` starts the wide form,
    // which then lacks its path terminator and second `u32 1`.
    let mut wide_marker = compact_record(10, 1, Some(MATRIX));
    wide_marker[DISCRIMINATOR + 4] = 1;
    assert!(super::super::occurrence_placement(&wide_marker, None)
        .unwrap()
        .is_none());
    // The insertion-carrier envelope keeps the wide path.
    let compact = compact_record(10, 1, None);
    assert!(
        super::super::identity_component_insert_carrier(&compact, 0, compact.len(), 10).is_none()
    );
}

#[test]
fn compact_legacy_insert_carriers_bind_by_record_and_equal_placement() {
    let mut bytes = vec![0xee; 3];
    let carrier_at = bytes.len();
    bytes.extend(compact_record(10, 2, Some(MATRIX)));
    let relation_at = bytes.len();
    let (role, role_offset, transform_offset) =
        super::super::legacy_typed_component_insert_carrier(
            &bytes,
            carrier_at,
            relation_at,
            10,
            MATRIX,
        )
        .expect("compact carrier with the scope placement");
    assert_eq!(role, ROLE);
    assert_eq!(transform_offset, Some(carrier_at + 307));
    assert!(transform_offset.is_some_and(|offset| offset < role_offset));
    let identity = cadmpeg_ir::transform::Transform::identity().rows();
    assert!(super::super::legacy_typed_component_insert_carrier(
        &bytes,
        carrier_at,
        relation_at,
        10,
        identity
    )
    .is_none());
    assert!(super::super::legacy_typed_component_insert_carrier(
        &bytes,
        carrier_at,
        relation_at,
        11,
        MATRIX
    )
    .is_none());
}

fn placement(
    carrier_record_index: u64,
    transform: Option<[[f64; 4]; 4]>,
) -> super::super::OccurrencePlacement {
    super::super::OccurrencePlacement {
        frame_bytes: Vec::new(),
        occurrence_guid: None,
        carrier_record_index,
        owner: None,
        link_names: vec![ROLE.to_owned()],
        transform,
    }
}

#[test]
fn a_carrier_keeps_a_stored_placement_other_than_the_insertion_frame() {
    let insertion = [
        [1.0, 0.0, 0.0, -2.5],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let identity = cadmpeg_ir::transform::Transform::identity().rows();
    let placements = [
        placement(10, Some(MATRIX)),
        placement(11, Some(insertion)),
        placement(12, None),
    ];
    let direct = vec![
        (10, insertion),
        (11, insertion),
        (12, identity),
        (13, insertion),
    ];

    let selected =
        super::super::occurrence_transforms_with_precedence(direct.clone(), &placements, ROLE);

    assert_eq!(
        selected
            .iter()
            .map(|placement| (placement.carrier_record_index, placement.transform))
            .collect::<Vec<_>>(),
        [
            (10, Some(MATRIX)),
            (11, Some(insertion)),
            (12, Some(identity)),
            (13, Some(insertion)),
        ]
    );
    assert_eq!(
        super::super::superseded_placement_count(&direct, &placements, ROLE),
        2
    );
}

#[test]
fn a_repeated_target_insert_carrier_reports_its_own_placement() {
    let carrier = super::repeated_target_occurrence_record(ROLE, 15, 6, Some(MATRIX));
    let (role, _, placement) =
        super::super::repeated_target_component_insert_carrier(&carrier, 0, carrier.len(), 15)
            .unwrap()
            .expect("repeated-target carrier");
    assert_eq!(role, ROLE);
    let (offset, matrix) = placement.expect("stored matrix");
    assert_eq!(matrix, MATRIX);
    assert_eq!(carrier[offset - 1], 0);
    let other = [
        [1.0, 0.0, 0.0, -2.5],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    assert!(
        super::super::repeated_target_component_insert(&carrier, 0, carrier.len(), 15, other)
            .unwrap()
            .is_none()
    );
    assert!(
        super::super::repeated_target_component_insert_carrier(&carrier, 0, carrier.len(), 16)
            .unwrap()
            .is_none()
    );
}

#[test]
fn compact_legacy_placements_without_an_operation_table_stay_at_the_insertion_frame() {
    let mut bytes = compact_record(10, 1, Some(MATRIX));
    bytes.extend(super::legacy_occurrence_record(ROLE, 12, Some(MATRIX)));
    let records = super::super::indexed_records(&bytes);
    for (posed, unstated) in [
        (std::collections::HashSet::new(), true),
        (std::collections::HashSet::from([12]), true),
        (std::collections::HashSet::from([10]), false),
    ] {
        let (placements, failures) = super::super::occurrence_placements_with_failures(
            &bytes,
            &records,
            None,
            None,
            Some(&posed),
        )
        .unwrap();
        // The placement is admitted at its stored matrix either way; a missing
        // operation table only reports that the pose is the insertion frame.
        assert_eq!(
            placements
                .iter()
                .map(|placement| (placement.carrier_record_index, placement.transform))
                .collect::<Vec<_>>(),
            [(10, Some(MATRIX)), (12, Some(MATRIX))]
        );
        assert_eq!(failures.len(), usize::from(unstated));
        if let Some(failure) = failures.first() {
            assert_eq!(failure.carrier_record_index, Some(10));
            assert_eq!(failure.link_names, [ROLE]);
            assert!(failure.pose_unstated);
        }
    }
    assert!(super::super::compact_legacy_placement(&bytes[..399 + 128]));
    assert!(!super::super::compact_legacy_placement(&bytes[399 + 128..]));
}

#[test]
fn a_component_path_carrier_states_the_pose_of_a_compact_placement() {
    const ROOT_COMPONENT: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let current = [
        [1.0, 0.0, 0.0, 0.5],
        [0.0, 1.0, 0.0, 1.0],
        [0.0, 0.0, 1.0, 0.4],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let bytes = compact_record(10, 1, Some(MATRIX));
    let records = super::super::indexed_records(&bytes);
    let posed = std::collections::HashSet::new();
    let (placements, failures) = super::super::occurrence_placements_with_failures(
        &bytes,
        &records,
        None,
        None,
        Some(&posed),
    )
    .unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(failures.len(), 1);

    // A component path that names placement record 10 states its pose.
    let carrier = super::legacy_path_pose::legacy_root_component_path_carrier(
        ROOT_COMPONENT,
        0x0102_0304_0506_0708,
        current,
    );
    let pose = super::super::component_path_pose(&carrier, magic).expect("component path pose");
    assert_eq!(pose.placement_record_index, 10);
    let mut stated = failures.clone();
    super::super::retain_unstated_poses(&mut stated, &[pose]);
    assert!(stated.is_empty());
    // Without one, the report stays.
    let mut unstated = failures;
    super::super::retain_unstated_poses(&mut unstated, &[]);
    assert_eq!(unstated.len(), 1);
}

fn component_targets(mut bytes: Vec<u8>) -> Vec<u8> {
    let placeholder = b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let offsets: Vec<_> = bytes
        .windows(placeholder.len())
        .enumerate()
        .filter_map(|(offset, value)| (value == placeholder).then_some(offset))
        .collect();
    assert_eq!(offsets.len(), 2);
    for offset in offsets {
        bytes[offset..offset + placeholder.len()].copy_from_slice(
            crate::records::feature::occurrence_transfer::COMPONENT_TYPE.as_bytes(),
        );
    }
    bytes
}

#[test]
fn a_wide_legacy_record_keeps_the_wide_reading_and_a_compact_one_binds_its_owner() {
    let wide_bytes = component_targets(super::legacy_occurrence_record(ROLE, 10, Some(MATRIX)));
    assert!(!super::super::compact_legacy_placement(&wide_bytes));
    assert!(super::super::legacy_occurrence_placement(&wide_bytes).is_some());
    assert!(super::super::compact_legacy_placement_details(&wide_bytes).is_some());
    let wide = super::super::occurrence_placement(&wide_bytes, None)
        .unwrap()
        .expect("wide placement");
    assert!(wide.owner.is_none());
    assert!(wide.frame_bytes.is_empty());
    assert_eq!(wide.transform, Some(MATRIX));
    assert_eq!(wide.link_names, [ROLE]);
    let compact_bytes = component_targets(compact_record(11, 1, Some(MATRIX)));
    assert!(super::super::compact_legacy_placement(&compact_bytes));
    let compact = super::super::occurrence_placement(&compact_bytes, None)
        .unwrap()
        .expect("compact placement");
    assert_eq!(compact.owner.expect("local owner").target, 4);
    assert_eq!(compact.frame_bytes, compact_bytes);
    assert_eq!(compact.transform, Some(MATRIX));
    assert_eq!(compact.link_names, [ROLE]);
}
