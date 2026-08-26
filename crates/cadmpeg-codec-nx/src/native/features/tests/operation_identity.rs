// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::native::om::{OmOperationStateJournalGroup, OmOperationStateJournalRow};
use crate::test_support::{
    composed_feature_history_payload, composed_feature_history_section, prt_with_named_payloads,
};
use std::collections::BTreeMap;

fn label(ordinal: u32, object_indices: [Option<u32>; 4]) -> FeatureOperationLabel {
    FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: "EXTRUDE".to_string(),
        object_indices,
        raw_object_indices: std::array::from_fn(|_| vec![0xff]),
        stable_identity: None,
        source_offset: u64::from(ordinal),
    }
}

#[test]
fn operation_header_identity_witness_survives_reordering() {
    let block_identities = BTreeMap::from([
        (55, Some("block-55".to_string())),
        (56, Some("block-56".to_string())),
        (61, Some("block-61".to_string())),
    ]);
    let mut original = vec![
        label(0, [Some(55), Some(56), None, None]),
        label(1, [None; 4]),
        label(2, [Some(61), None, None, None]),
    ];
    assign_operation_header_identities(&mut original, &block_identities);
    let identity = original[0]
        .stable_identity
        .clone()
        .expect("unique non-null header tuple has an identity witness");
    assert_eq!(
        identity,
        "nx:feature-history:operation-header-identity#content:block-55-block-56-null-null"
    );
    assert!(original[1].stable_identity.is_none());

    let mut reordered = vec![
        original[2].clone(),
        original[0].clone(),
        original[1].clone(),
    ];
    for label in &mut reordered {
        label.stable_identity = None;
    }
    assign_operation_header_identities(&mut reordered, &block_identities);
    assert_eq!(
        reordered[1].stable_identity.as_deref(),
        Some(identity.as_str())
    );
    assert!(reordered[2].stable_identity.is_none());
}

#[test]
fn feature_label_identity_retains_the_complete_header_ordinal() {
    const HEADER: &[u8] = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff";
    let mut section = composed_feature_history_section(&[
        (&[0xff; 4], "BLOCK", b"first".to_vec()),
        (&[0xff; 4], "SKETCH", b"second".to_vec()),
    ]);
    let second_header = section
        .windows(HEADER.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == HEADER).then_some(offset))
        .nth(1)
        .expect("second operation header");
    let mut unlabeled = HEADER.to_vec();
    unlabeled.extend_from_slice(&[0xff; 4]);
    unlabeled.extend_from_slice(b"unlabeled");
    section.splice(second_header..second_header, unlabeled);
    let payload_len = (section.len() - 16) as u32;
    section[8..12].copy_from_slice(&payload_len.to_be_bytes());
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&section);
    let container = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        payload,
    )]))
    .expect("feature-history fixture");

    let labels = super::feature_operation_labels(&container);
    assert_eq!(labels.len(), 2);
    assert!(labels[0].id.ends_with("-0000000000"));
    assert!(labels[1].id.ends_with("-0000000002"));
    let records = super::feature_operation_records(&container);
    assert_eq!(records[1].operation_label, labels[1].id);
}

#[test]
fn operation_header_identity_rejects_duplicate_tuples() {
    let block_identities = BTreeMap::from([
        (55, Some("block-55".to_string())),
        (56, Some("block-56".to_string())),
    ]);
    let mut labels = vec![
        label(0, [Some(55), Some(56), None, None]),
        label(1, [Some(55), Some(56), None, None]),
    ];
    assign_operation_header_identities(&mut labels, &block_identities);
    assert!(labels.iter().all(|label| label.stable_identity.is_none()));
}

#[test]
fn operation_header_identity_survives_offset_store_insertion() {
    let first_payload = composed_feature_history_payload(
        &[(&[1, 2, 0xff, 0xff], "EXTRUDE", Vec::new())],
        &[b"alpha".as_slice(), b"beta".as_slice()],
    );
    let second_payload = composed_feature_history_payload(
        &[(&[2, 3, 0xff, 0xff], "EXTRUDE", Vec::new())],
        &[
            b"inserted".as_slice(),
            b"alpha".as_slice(),
            b"beta".as_slice(),
        ],
    );
    let first = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        first_payload,
    )]))
    .expect("first synthetic container");
    let second = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        second_payload,
    )]))
    .expect("second synthetic container");

    let first_labels = super::feature_operation_labels(&first);
    let second_labels = super::feature_operation_labels(&second);
    assert_eq!(
        first_labels[0].object_indices,
        [Some(1), Some(2), None, None]
    );
    assert_eq!(
        second_labels[0].object_indices,
        [Some(2), Some(3), None, None]
    );
    assert_eq!(
        first_labels[0].stable_identity,
        second_labels[0].stable_identity
    );

    let first_records = super::feature_operation_records(&first);
    let second_records = super::feature_operation_records(&second);
    assert_eq!(
        first_records[0].stable_identity,
        second_records[0].stable_identity
    );
}

#[test]
fn operation_header_identity_requires_unique_resolved_blocks() {
    let block_identities = BTreeMap::from([(55, None), (56, Some("block-56".to_string()))]);
    let mut labels = vec![label(0, [Some(55), Some(56), None, None])];
    assign_operation_header_identities(&mut labels, &block_identities);
    assert!(labels[0].stable_identity.is_none());
}

#[test]
fn operation_body_write_retains_identity_group_and_image() {
    let body_writes = vec![
        0x01, 0x02, 0x11, 0x80, 0xa9, 0x97, 0x75, 0x01, 0x02, 0x10, 0x86, 0x93, 0xff, 0x01, 0x02,
        0x12, 0x80, 0xa9, 0x97, 0x75, 0x01, 0x02, 0x10, 0x86, 0x94, 0xff,
    ];
    let payload = composed_feature_history_payload(&[(&[0xff; 4], "EXTRUDE", body_writes)], &[]);
    let container = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        payload,
    )]))
    .expect("synthetic body-write container");
    let writes = super::feature_operation_body_writes(&container);
    let [first, second] = writes.as_slice() else {
        panic!("two body-write frames");
    };
    assert_eq!(first.body_identity, 0x11);
    assert_eq!(first.group_node, 0xa9);
    assert_eq!(first.raw_group_node, [0x80, 0xa9]);
    assert_eq!(first.body_image_object_index, 0x693);
    assert_eq!(first.raw_body_image_object_index, [0x86, 0x93]);
    assert_eq!(second.body_identity, 0x12);
    assert_eq!(second.group_node, first.group_node);
    assert_eq!(second.body_image_object_index, 0x694);
}

#[test]
fn operation_body_write_resolves_one_unique_image_block() {
    let body_write = vec![
        0x01, 0x02, 0x0b, 0x31, 0x97, 0x75, 0x01, 0x02, 0x10, 0x41, 0xff,
    ];
    let store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload =
        composed_feature_history_payload(&[(&[0xff; 4], "EXTRUDE", body_write)], &store_records);
    let container = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        payload,
    )]))
    .expect("synthetic body-image store");

    let writes = super::feature_operation_body_writes(&container);

    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].body_image_object_index, 65);
    assert_eq!(
        writes[0].body_image_data_block.as_deref(),
        Some("nx:om-data-blocks-0:block#65")
    );
}

#[test]
fn body_image_segment_use_requires_one_plain_alias() {
    let body_write = vec![
        0x01, 0x02, 0x0b, 0x31, 0x97, 0x75, 0x01, 0x02, 0x10, 0x41, 0xff,
    ];
    let store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload =
        composed_feature_history_payload(&[(&[0xff; 4], "EXTRUDE", body_write)], &store_records);
    let container = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        payload,
    )]))
    .expect("synthetic body-image store");
    let writes = super::feature_operation_body_writes(&container);
    let binding = |id: &str, stream_kind: &str| SegmentBodyBinding {
        id: id.to_string(),
        stream_link: format!("{id}:link"),
        stream_ordinal: 0,
        stream_kind: stream_kind.to_string(),
        body_object_index: 42,
        body_alias_object_index: 11,
        stream_role: 10,
        source_offset: 100,
    };

    let uses = super::feature_operation_body_image_segment_uses(
        &writes,
        &[binding("plain", "plain"), binding("partition", "partition")],
    );

    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].operation_body_write, writes[0].id);
    assert_eq!(
        uses[0].body_image_data_block,
        "nx:om-data-blocks-0:block#65"
    );
    assert_eq!(uses[0].segment_body_binding, "plain");
    assert!(super::feature_operation_body_image_segment_uses(
        &writes,
        &[binding("first", "plain"), binding("second", "plain")],
    )
    .is_empty());
}

#[test]
fn body_identity_segment_use_does_not_require_an_image_block() {
    let mut write = FeatureOperationBodyWrite {
        id: "nx:operation-body-write#0".into(),
        operation_label: "operation".into(),
        operation_record: "record".into(),
        ordinal: 0,
        body_identity: 11,
        group_node: 1,
        raw_group_node: vec![1],
        group_node_source_offset: 0,
        endpoint_tag: 0x12,
        body_image_object_index: 1519,
        body_image_data_block: None,
        raw_body_image_object_index: vec![0x95, 0xef],
        body_image_object_index_source_offset: 0,
        byte_len: 12,
        source_offset: 0,
    };
    let binding = |id: &str, stream_kind: &str| SegmentBodyBinding {
        id: id.into(),
        stream_link: format!("{id}:link"),
        stream_ordinal: 1,
        stream_kind: stream_kind.into(),
        body_object_index: 42,
        body_alias_object_index: 11,
        stream_role: 10,
        source_offset: 100,
    };

    let uses = super::feature_operation_body_identity_segment_uses(
        std::slice::from_ref(&write),
        &[binding("plain", "plain"), binding("partition", "partition")],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].operation_body_write, write.id);
    assert_eq!(uses[0].body_identity, 11);
    assert_eq!(uses[0].segment_body_binding, "plain");

    write.body_image_data_block = Some("irrelevant".into());
    assert!(super::feature_operation_body_identity_segment_uses(
        &[write],
        &[binding("first", "plain"), binding("second", "plain")],
    )
    .is_empty());
}

#[test]
fn body_partition_use_requires_a_complete_terminal_plain_run() {
    let body_write = vec![
        0x01, 0x02, 0x0b, 0x31, 0x97, 0x75, 0x01, 0x02, 0x10, 0x41, 0xff,
    ];
    let store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload =
        composed_feature_history_payload(&[(&[0xff; 4], "EXTRUDE", body_write)], &store_records);
    let container = crate::container::scan_bytes(prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        payload,
    )]))
    .expect("synthetic body-image store");
    let writes = super::feature_operation_body_writes(&container);
    let binding =
        |id: &str, stream_ordinal, body_alias_object_index, stream_role| SegmentBodyBinding {
            id: id.to_string(),
            stream_link: format!("{id}:link"),
            stream_ordinal,
            stream_kind: "plain".to_string(),
            body_object_index: 42,
            body_alias_object_index,
            stream_role,
            source_offset: 100,
        };
    let bindings = [binding("plain-0", 0, 11, 10), binding("plain-1", 1, 12, 16)];
    let image_uses = super::feature_operation_body_image_segment_uses(&writes, &bindings);
    let stream = |kind| crate::parasolid::Stream {
        file_offset: 0,
        consumed: 0,
        inflated: Vec::new(),
        kind,
        schema: Some("SCH_TEST".into()),
    };
    let streams = [
        stream(crate::parasolid::StreamKind::Plain),
        stream(crate::parasolid::StreamKind::Plain),
        stream(crate::parasolid::StreamKind::Partition),
        stream(crate::parasolid::StreamKind::Deltas),
        stream(crate::parasolid::StreamKind::Partition),
    ];
    let group =
        |id: &str, partition_stream_ordinal| crate::native::parasolid::ParasolidGroupRecord {
            id: id.into(),
            stream_ordinal: partition_stream_ordinal + 1,
            stream_kind: "deltas".into(),
            partition_stream_ordinal: Some(partition_stream_ordinal),
            xmt: 10,
            node_id: writes[0].group_node,
            references: vec![3, 4, 5, 6, 7],
            selector: 4,
            linked_reference_status: 0,
            byte_len: 20,
            inflated_offset: 0,
        };
    let groups = [group("owned", 2), group("collision", 4)];

    let uses = super::feature_operation_body_partition_uses(
        &writes,
        &image_uses,
        &bindings,
        &streams,
        &groups,
        &[],
    );

    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].partition_stream_ordinal, 2);
    assert_eq!(uses[0].parasolid_group_records, ["owned"]);

    let unterminated = [binding("plain-0", 0, 11, 10), binding("plain-1", 1, 12, 10)];
    assert!(super::feature_operation_body_partition_uses(
        &writes,
        &image_uses,
        &unterminated,
        &streams,
        &groups,
        &[],
    )
    .is_empty());

    let repeated_terminal = [binding("plain-0", 0, 11, 16), binding("plain-1", 1, 12, 16)];
    assert!(super::body_history_partition_stream(
        &repeated_terminal[1],
        &repeated_terminal,
        &streams,
    )
    .is_none());

    let interrupted_streams = [
        stream(crate::parasolid::StreamKind::Plain),
        stream(crate::parasolid::StreamKind::Deltas),
        stream(crate::parasolid::StreamKind::Partition),
    ];
    assert!(super::feature_operation_body_partition_uses(
        &writes,
        &image_uses,
        &bindings,
        &interrupted_streams,
        &groups,
        &[],
    )
    .is_empty());
}

#[test]
fn unlabeled_group_binds_a_body_identity_to_one_partition_namespace() {
    let unlabeled = FeatureUnlabeledOperationBodyWrite {
        id: "unlabeled-body-write".into(),
        operation_record: "unlabeled-record".into(),
        ordinal: 0,
        body_identity: 11,
        group_node: 99,
        raw_group_node: vec![99],
        group_node_source_offset: 10,
        endpoint_tag: 0x10,
        body_image_object_index: 20,
        body_image_data_block: Some("block".into()),
        raw_body_image_object_index: vec![20],
        body_image_object_index_source_offset: 11,
        byte_len: 12,
        source_offset: 9,
    };
    let group =
        |id: &str, partition_stream_ordinal| crate::native::parasolid::ParasolidGroupRecord {
            id: id.into(),
            stream_ordinal: partition_stream_ordinal + 1,
            stream_kind: "deltas".into(),
            partition_stream_ordinal: Some(partition_stream_ordinal),
            xmt: 10,
            node_id: 99,
            references: vec![3, 4, 5, 6, 7],
            selector: 4,
            linked_reference_status: 0,
            byte_len: 20,
            inflated_offset: 0,
        };

    let uses = super::feature_body_write_group_partition_uses(
        &[],
        std::slice::from_ref(&unlabeled),
        &[group("owned", 2)],
        &[],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].body_identity, 11);
    assert_eq!(uses[0].partition_stream_ordinal, 2);
    assert_eq!(uses[0].parasolid_group_records, ["owned"]);

    assert!(super::feature_body_write_group_partition_uses(
        &[],
        &[unlabeled],
        &[group("first", 2), group("collision", 4)],
        &[],
    )
    .is_empty());
}

fn journal_row(state_ordinal: u32, source_offset: u64) -> OmOperationStateJournalRow {
    OmOperationStateJournalRow {
        timestamp: 1_700_000_000,
        value_marker: 0xe0,
        value: state_ordinal,
        raw_value: vec![0xe0, 0, 0, 0, state_ordinal as u8],
        schema_id: 12,
        raw_schema_id: vec![12],
        state_ordinal,
        raw_state_ordinal: vec![state_ordinal as u8],
        source_offset,
        end_offset: source_offset + 16,
    }
}

fn journal_group(
    id: &str,
    section_link: &str,
    rows: Vec<OmOperationStateJournalRow>,
) -> OmOperationStateJournalGroup {
    OmOperationStateJournalGroup {
        id: id.to_string(),
        section_link: section_link.to_string(),
        ordinal: 0,
        selector: [4, 0],
        rows,
        source_entry: "/Root/UG_PART/UG_PART".to_string(),
        source_offset: 480,
        end_offset: 560,
    }
}

fn operation_record(id: &str, operation_label: &str) -> FeatureOperationRecord {
    FeatureOperationRecord {
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        ordinal: 0,
        byte_len: 32,
        sha256: "record-sha256".to_string(),
        payload_byte_len: 8,
        payload_sha256: "payload-sha256".to_string(),
        stable_identity: None,
        payload_source_offset: 404,
        source_offset: 400,
    }
}

fn terminal_frame(operation_record: &str, local_ordinal: u32) -> FeatureOperationTerminalFrame {
    FeatureOperationTerminalFrame {
        id: "nx:feature-history:operation-terminal-frame#0000000000-0000000000".to_string(),
        operation_record: operation_record.to_string(),
        immediate_common_frame: None,
        local_ordinal,
        raw_local_ordinal: vec![local_ordinal as u8],
        object_index: None,
        raw_object_index: vec![0xff],
        data_block: None,
        source_offset: 420,
        object_index_source_offset: 421,
    }
}

#[test]
fn operation_terminal_ordinal_joins_unique_section_journal_row() {
    let label = label(0, [None; 4]);
    let record = operation_record(
        "nx:feature-history:operation-record#0000000000-0000000000",
        &label.id,
    );
    let group = journal_group(
        "nx:feature-history:operation-state-journal-group#0000000000-0000000000",
        &label.section_link,
        vec![journal_row(6, 500), journal_row(7, 520)],
    );
    let frame = terminal_frame(&record.id, 7);

    let uses = feature_operation_state_journal_uses(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        std::slice::from_ref(&group),
    );

    let [relation] = uses.as_slice() else {
        panic!("one unique section-scoped journal row should join");
    };
    assert_eq!(
        relation.id,
        "nx:feature-history:operation-state-journal-use#0000000000-0000000000-0000000000-0000000000-0000000001"
    );
    assert_eq!(relation.operation_record, record.id);
    assert_eq!(relation.journal_row_ordinal, 1);
    assert_eq!(relation.operation_local_ordinal, 7);
    assert_eq!(relation.journal_state_ordinal, 7);
    assert_eq!(relation.operation_source_offset, 420);
    assert_eq!(relation.journal_source_offset, 520);
}

#[test]
fn operation_terminal_ordinal_rejects_wrong_section_and_ambiguous_rows() {
    let label = label(0, [None; 4]);
    let record = operation_record(
        "nx:feature-history:operation-record#0000000000-0000000000",
        &label.id,
    );
    let frame = terminal_frame(&record.id, 7);
    let wrong_section = journal_group(
        "nx:feature-history:operation-state-journal-group#wrong",
        "history#1",
        vec![journal_row(7, 600)],
    );
    let matching = journal_group(
        "nx:feature-history:operation-state-journal-group#matching",
        &label.section_link,
        vec![journal_row(7, 620)],
    );
    let duplicate = journal_group(
        "nx:feature-history:operation-state-journal-group#duplicate",
        &label.section_link,
        vec![journal_row(7, 640)],
    );

    let section_scoped = feature_operation_state_journal_uses(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        &[wrong_section, matching.clone()],
    );
    assert_eq!(section_scoped.len(), 1);
    assert_eq!(section_scoped[0].journal_source_offset, 620);

    let ambiguous = feature_operation_state_journal_uses(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        &[matching, duplicate],
    );
    assert!(ambiguous.is_empty());
}
