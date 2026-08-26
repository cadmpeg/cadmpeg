// SPDX-License-Identifier: Apache-2.0
#![allow(unused_imports)]

use super::*;

#[test]
fn operation_source_properties_require_unique_owned_structures() {
    let record = crate::native::features::FeatureOperationRecord {
        id: "record".into(),
        operation_label: "operation".into(),
        ordinal: 3,
        byte_len: 20,
        sha256: "record-hash".into(),
        payload_byte_len: 10,
        payload_sha256: "payload-hash".into(),
        stable_identity: None,
        payload_source_offset: 110,
        source_offset: 100,
    };
    let common = crate::native::features::FeatureOperationCommonFrame {
        id: "common".into(),
        operation_record: record.id.clone(),
        ordinal: 0,
        indices: [0, 351, 171],
        raw_indices: [vec![0], vec![0x81, 0x5f], vec![0x80, 0xab]],
        marker: [1, 3, 2],
        state: [1, 2, 1, 1, 1, 0, 0, 0],
        legacy_inactive_modules: Some(true),
        modifies_parasolid_data: Some(true),
        split_tracking_data: [0, 0],
        group_count: 0,
        local_ordinal: 41,
        raw_local_ordinal: vec![0x29],
        object_index: Some(65),
        raw_object_index: vec![0x41],
        data_block: None,
        byte_len: 20,
        source_offset: 101,
        index_source_offsets: [101, 102, 104],
        state_source_offset: 109,
        local_ordinal_source_offset: 117,
        object_index_source_offset: 119,
    };
    let frame = crate::native::features::FeatureOperationTerminalFrame {
        id: "frame".into(),
        operation_record: record.id.clone(),
        immediate_common_frame: Some(common.id.clone()),
        local_ordinal: 41,
        raw_local_ordinal: vec![0x29],
        object_index: Some(65),
        raw_object_index: vec![0x41],
        data_block: None,
        source_offset: 117,
        object_index_source_offset: 119,
    };
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            std::slice::from_ref(&common),
            std::slice::from_ref(&frame),
        ),
        BTreeMap::from([
            ("operation_common_frame.0".into(), "common".into()),
            ("operation_record".into(), "record".into()),
            ("operation_terminal_frame".into(), "frame".into()),
        ])
    );
    assert!(super::operation_source_properties("missing", &[], &[], &[]).is_empty());
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            &[],
            &[],
        ),
        BTreeMap::from([("operation_record".into(), "record".into())])
    );
    let mut noncontiguous_common = common.clone();
    noncontiguous_common.ordinal = 1;
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            std::slice::from_ref(&noncontiguous_common),
            std::slice::from_ref(&frame),
        ),
        BTreeMap::from([
            ("operation_record".into(), "record".into()),
            ("operation_terminal_frame".into(), "frame".into()),
        ])
    );
    assert!(super::operation_source_properties(
        &record.operation_label,
        &[record.clone(), record.clone()],
        std::slice::from_ref(&common),
        std::slice::from_ref(&frame),
    )
    .is_empty());
    assert_eq!(
        super::operation_source_properties(
            &record.operation_label,
            std::slice::from_ref(&record),
            &[],
            &[frame.clone(), frame],
        ),
        BTreeMap::from([("operation_record".into(), "record".into())])
    );
}
