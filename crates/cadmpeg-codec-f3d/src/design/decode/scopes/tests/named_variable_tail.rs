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
fn parameter_scope_parses_named_variable_tail() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"378");
    bytes.extend_from_slice(&12u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&55u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&7u32.to_le_bytes());
    lp_utf16(&mut bytes, "Draft");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    lp_utf16(&mut bytes, "draft-name");
    bytes.extend_from_slice(&[0; 7]);

    bytes.push(1);
    bytes.push(0x4e);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.extend_from_slice(&0.25f64.to_le_bytes());
    bytes.extend_from_slice(&0xfcu32.to_le_bytes());
    bytes.push(1);
    bytes.push(0x4d);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1, 0, 0]);
    bytes.push(1);
    bytes.push(0x4c);
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&[0; 3]);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"261");
    bytes.extend_from_slice(&12u32.to_le_bytes());

    let header = DesignRecordHeader {
        id: "generated:scope-header#0".into(),
        record_index: 12,
        class_tag: "378".into(),
        byte_offset: 0,
    };
    let scope = parse_parameter_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &header)
        .expect("named variable-tail scope");
    assert_eq!(scope.kind(), crate::records::DesignFeatureKind::Draft);
    assert_eq!(scope.feature_ordinal.get(), 1);
    assert_eq!(scope.history_state_id, Some(7));
    assert_eq!(scope.previous_history_state_id, None);
    assert_eq!(scope.previous_history_state_id_offset, None);
    assert_eq!(scope.reference_members.values().copied().collect::<Vec<_>>(), [55]);
    assert_eq!(scope.frame_length, paired_at as u64);

    let mut owner_scope = scope.clone();
    owner_scope.reference_members = crate::records::ReferenceRun::Unlocated(vec![327, 330, 55, 56, 57, 58]);
    let owners = vec![
        DesignParameterOwner {
            id: "f3d:test:owner#327".into(),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "272".into(),
            record_index: 327,
            scope_record_index: 12,
            local_ordinal: 0,
            evaluated_value: 0.0,
            evaluated_value_offset: 111,
            parameter_record_index: 326,
            owned_ordinal: 3,
            variant: Some(0),
            companion_record_index: 328,
        },
        DesignParameterOwner {
            id: "f3d:test:owner#330".into(),
            byte_offset: 0,
            frame_length: 104,
            class_tag: "272".into(),
            record_index: 330,
            scope_record_index: 12,
            local_ordinal: 1,
            evaluated_value: 0.0,
            evaluated_value_offset: 222,
            parameter_record_index: 329,
            owned_ordinal: 4,
            variant: Some(0),
            companion_record_index: 331,
        },
    ];
    let operation = exact_draft_operation_with_owners(
        &bytes,
        &IndexedRecordOffsets::build(&bytes),
        &owner_scope,
        &owners,
    )
    .expect("owner-lane Draft operation");
    assert_eq!(operation.angle, 0.0);
    assert_eq!(operation.angle_record_index, 327);
    assert_eq!(operation.opposite_angle_record_index, 330);
    assert_eq!(operation.angle_offset, 111);
    assert_eq!(operation.opposite_angle_offset, 222);
}
