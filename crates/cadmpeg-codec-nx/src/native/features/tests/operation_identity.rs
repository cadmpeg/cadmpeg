// SPDX-License-Identifier: Apache-2.0

use crate::native::features::assign_operation_header_identities;
use crate::native::features::body_history_partition_stream;
use crate::native::features::feature_body_reference_occurrences;
use crate::native::features::feature_body_references;
use crate::native::features::feature_body_write_group_partition_uses;
use crate::native::features::feature_input_block_identity_groups;
use crate::native::features::feature_input_blocks;
use crate::native::features::feature_operation_body_identity_segment_uses;
use crate::native::features::feature_operation_body_image_segment_uses;
use crate::native::features::feature_operation_body_partition_uses;
use crate::native::features::feature_operation_body_writes;
use crate::native::features::feature_operation_common_frames;
use crate::native::features::feature_operation_labels;
use crate::native::features::feature_operation_object_references;
use crate::native::features::feature_operation_records;
use crate::native::features::feature_operation_state_journal_uses;
use crate::native::features::feature_operation_terminal_frames;
use crate::native::features::feature_payload_strings;
use crate::native::features::feature_unlabeled_operation_records;
use crate::native::features::operation_record::FeatureOperationRecord;
use crate::native::features::FeatureOperationBodyWrite;
use crate::native::features::FeatureOperationLabel;
use crate::native::features::FeatureOperationStateJournalUse;
use crate::native::features::FeatureOperationTerminalFrame;
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::native::segments::SegmentBodyBinding;
use crate::om::state_journal::JournalRow;
use crate::test_support::test_om::composed_feature_history_payload;
use crate::test_support::test_om::composed_feature_history_payload_over_sort_scratch;
use crate::test_support::test_om::composed_feature_history_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use std::collections::BTreeMap;

fn image_segment_uses_for_test(
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Vec<crate::native::features::FeatureOperationBodyImageSegmentUse> {
    feature_operation_body_image_segment_uses(writes, bindings)
}

fn identity_segment_uses_for_test(
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Vec<crate::native::features::FeatureOperationBodyIdentitySegmentUse> {
    feature_operation_body_identity_segment_uses(writes, bindings)
}

fn body_partition_uses_for_test(
    writes: &[FeatureOperationBodyWrite],
    image_uses: &[crate::native::features::FeatureOperationBodyImageSegmentUse],
    bindings: &[SegmentBodyBinding],
    streams: &[crate::parasolid::Stream],
    groups: &[crate::native::parasolid::ParasolidGroupRecord],
    members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Vec<crate::native::features::FeatureOperationBodyPartitionUse> {
    {
        feature_operation_body_partition_uses(
            writes, image_uses, bindings, streams, groups, members,
        )
    }
}

fn body_group_partition_uses_for_test(
    writes: &[FeatureOperationBodyWrite],
    unlabeled_writes: &[FeatureOperationBodyWrite],
    groups: &[crate::native::parasolid::ParasolidGroupRecord],
    members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Vec<crate::native::features::FeatureBodyWriteGroupPartitionUse> {
    feature_body_write_group_partition_uses(writes, unlabeled_writes, groups, members)
}

#[derive(Clone, Copy)]
enum BodySegmentJoinRoute {
    Image,
    Identity,
}

fn assert_body_segment_join_admits(route: BodySegmentJoinRoute) {
    let write = FeatureOperationBodyWrite {
        id: "nx:operation-body-write#0".to_string(),
        operation_label: Some("operation#0".to_string()),
        operation_record: "record#0".to_string(),
        ordinal: 0,
        frame: crate::om::body_write::BodyWriteFrame::<u64>::new(
            11,
            crate::om::body_write::BodyWriteIndex::from_wire(1, &[1]).expect("group token"),
            crate::om::body_write::BodyImageTag::Form12,
            crate::om::body_write::BodyWriteIndex::from_wire(2, &[2]).expect("image token"),
            0,
        )
        .expect("body-write frame"),
        body_image_data_block: Some("block#2".to_string()),
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Plain,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 16,
        source_offset: 0,
    };
    let call = |writes: &[FeatureOperationBodyWrite], bindings: &[SegmentBodyBinding]| match route {
        BodySegmentJoinRoute::Image => {
            feature_operation_body_image_segment_uses(writes, bindings).len()
        }
        BodySegmentJoinRoute::Identity => {
            feature_operation_body_identity_segment_uses(writes, bindings).len()
        }
    };
    let admitted = call(std::slice::from_ref(&write), std::slice::from_ref(&binding));
    assert_eq!(admitted, 1);
}

macro_rules! body_segment_join_admit_test {
    ($name:ident, $route:path) => {
        #[test]
        fn $name() {
            assert_body_segment_join_admits($route);
        }
    };
}

body_segment_join_admit_test!(body_image_join_admits_fixture, BodySegmentJoinRoute::Image);
body_segment_join_admit_test!(
    body_identity_join_admits_fixture,
    BodySegmentJoinRoute::Identity
);

#[test]
fn body_partition_join_admits_fixture() {
    let write = FeatureOperationBodyWrite {
        id: "nx:operation-body-write#0".to_string(),
        operation_label: Some("operation#0".to_string()),
        operation_record: "record#0".to_string(),
        ordinal: 0,
        frame: crate::om::body_write::BodyWriteFrame::<u64>::new(
            11,
            crate::om::body_write::BodyWriteIndex::from_wire(1, &[1]).expect("group token"),
            crate::om::body_write::BodyImageTag::Form12,
            crate::om::body_write::BodyWriteIndex::from_wire(2, &[2]).expect("image token"),
            0,
        )
        .expect("body-write frame"),
        body_image_data_block: Some("block#2".to_string()),
    };
    let image_use = crate::native::features::FeatureOperationBodyImageSegmentUse {
        id: "image-use#0".to_string(),
        operation_body_write: write.id.clone(),
        body_image_data_block: "block#2".to_string(),
        segment_body_binding: "binding#0".to_string(),
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Plain,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 16,
        source_offset: 0,
    };
    let stream = |subtype| crate::parasolid::Stream {
        file_offset: 0,
        consumed: 0,
        inflated: Vec::new(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype,
            schema: Some(
                cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST").expect("schema token"),
            ),
        },
    };
    let streams = [
        stream(crate::parasolid::ParasolidSubtype::Plain),
        stream(crate::parasolid::ParasolidSubtype::Partition),
    ];
    let route = || {
        feature_operation_body_partition_uses(
            std::slice::from_ref(&write),
            std::slice::from_ref(&image_use),
            std::slice::from_ref(&binding),
            &streams,
            &[],
            &[],
        )
    };
    let admitted = route();
    assert_eq!(admitted.len(), 1);
}

fn label(ordinal: u32, object_indices: [Option<u32>; 4]) -> FeatureOperationLabel {
    FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: "EXTRUDE".to_string(),
        objects: crate::om::header_references::HeaderReferences(object_indices.map(|value| {
            value.map(|value| {
                crate::om::reference_index::FeatureReferenceToken::from_wire(
                    value,
                    &[u8::try_from(value).unwrap()],
                )
                .unwrap()
            })
        })),
        stable_identity: None,
        source_offset: u64::from(ordinal),
    }
}

fn unlabeled_history_fixture() -> crate::container::Container<'static> {
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
    let payload_len = u32::try_from(section.len() - 16).expect("fixture value fits u32");
    section[8..12].copy_from_slice(&payload_len.to_be_bytes());
    let payload = crate::test_support::test_om::feature_history_section_over_sort_scratch(&section);
    {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("feature-history fixture")
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
    let container = unlabeled_history_fixture();

    let labels = feature_operation_labels(&container).unwrap();
    assert_eq!(labels.len(), 2);
    assert!(labels[0].id.ends_with("-0000000000"));
    assert!(labels[1].id.ends_with("-0000000002"));
    let records =
        crate::test_support::with_decode_context(|ctx| feature_operation_records(ctx, &container))
            .unwrap();
    assert_eq!(records[1].operation_label, labels[1].id);
}

#[test]
fn feature_operation_record_admits_fixture() {
    let container = unlabeled_history_fixture();
    let admitted =
        crate::test_support::with_decode_context(|ctx| feature_operation_records(ctx, &container))
            .expect("admitted feature operation records");
    assert_eq!(admitted.len(), 2);
}

#[test]
fn feature_label_identity_admits_full_ordinal_width() {
    let id = crate::native::features::format_feature_history_id(
        "operation-label",
        "0000000000",
        usize::MAX,
        None,
    );
    assert_eq!(
        id,
        format!(
            "nx:feature-history:operation-label#0000000000-{value:010}",
            value = usize::MAX
        ),
    );
}

#[test]
fn unlabeled_record_route_preserves_source_order() {
    let container = unlabeled_history_fixture();
    let records = crate::test_support::with_decode_context(|ctx| {
        feature_unlabeled_operation_records(ctx, &container)
    })
    .unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].id.ends_with("-0000000001"));
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
    let first = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            first_payload,
        )]))
    }
    .expect("first synthetic container");
    let second = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            second_payload,
        )]))
    }
    .expect("second synthetic container");

    let first_labels = feature_operation_labels(&first).unwrap();
    let second_labels = feature_operation_labels(&second).unwrap();
    assert_eq!(
        first_labels[0].objects.values(),
        [Some(1), Some(2), None, None]
    );
    assert_eq!(
        second_labels[0].objects.values(),
        [Some(2), Some(3), None, None]
    );
    assert_eq!(
        first_labels[0].stable_identity,
        second_labels[0].stable_identity
    );

    let first_records =
        crate::test_support::with_decode_context(|ctx| feature_operation_records(ctx, &first))
            .unwrap();
    let second_records =
        crate::test_support::with_decode_context(|ctx| feature_operation_records(ctx, &second))
            .unwrap();
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
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic body-write container");
    let writes = feature_operation_body_writes(&container).unwrap();
    let [first, second] = writes.as_slice() else {
        panic!("two body-write frames");
    };
    assert_eq!(first.frame.body_identity(), 0x11);
    assert_eq!(first.frame.group_node().value(), 0xa9);
    assert_eq!(first.frame.group_node().raw(), [0x80, 0xa9]);
    assert_eq!(first.frame.body_image().value(), 0x693);
    assert_eq!(first.frame.body_image().raw(), [0x86, 0x93]);
    assert_eq!(second.frame.body_identity(), 0x12);
    assert_eq!(
        second.frame.group_node().value(),
        first.frame.group_node().value()
    );
    assert_eq!(second.frame.body_image().value(), 0x694);
}

fn assert_operation_object_reference_admits(kind: crate::om::direct_reference::ReferenceFieldKind) {
    let field: &[u8] = match kind {
        crate::om::direct_reference::ReferenceFieldKind::DataBlock03 => {
            b"\x01\x02\x03\x07\x01\x00\x00\x00\x00\x00"
        }
        crate::om::direct_reference::ReferenceFieldKind::Tagged17 => {
            b"\x01\x02\x17\x07\xff\x80\x00\x00\x02"
        }
    };
    let payload = composed_feature_history_payload_over_sort_scratch(
        &[(&[0xff; 4], "EXTRUDE", field.to_vec())],
        &[],
    );
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic direct-reference container");
    let route = || feature_operation_object_references(&container, kind);
    let admitted = route().expect("admitted operation object reference");
    assert_eq!(admitted.len(), 1);
}

#[test]
fn operation_object_reference_route_admits_fixture() {
    for kind in [
        crate::om::direct_reference::ReferenceFieldKind::DataBlock03,
        crate::om::direct_reference::ReferenceFieldKind::Tagged17,
    ] {
        assert_operation_object_reference_admits(kind);
    }
}

fn assert_operation_common_frame_admits(terminal: bool) {
    let frame = [
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x81, 0x23, 0x81, 0x23, 0xff, 0x00,
    ];
    let payload = composed_feature_history_payload_over_sort_scratch(
        &[(&[0xff; 4], "FSET", frame.to_vec())],
        &[],
    );
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic common-frame container");
    let common =
        feature_operation_common_frames(&container).expect("admitted operation common frames");
    let route = || {
        if terminal {
            feature_operation_terminal_frames(&container, &common).map(|values| values.len())
        } else {
            feature_operation_common_frames(&container).map(|values| values.len())
        }
    };
    let admitted = route().expect("admitted operation frame route");
    assert_eq!(admitted, 1);
}

#[derive(Clone, Copy)]
enum FeaturePayloadReferenceRoute {
    Text,
    PrimaryBody,
    BodyOccurrences,
}

fn assert_feature_payload_reference_admits(route_kind: FeaturePayloadReferenceRoute) {
    let payload_bytes: &[u8] = match route_kind {
        FeaturePayloadReferenceRoute::Text => b"\x04\x07BLOCK\0",
        FeaturePayloadReferenceRoute::PrimaryBody
        | FeaturePayloadReferenceRoute::BodyOccurrences => b"\x01\x02\x10\x07\xff",
    };
    let payload = composed_feature_history_payload_over_sort_scratch(
        &[(&[0xff; 4], "EXTRUDE", payload_bytes.to_vec())],
        &[],
    );
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic feature payload container");
    let route = || match route_kind {
        FeaturePayloadReferenceRoute::Text => {
            feature_payload_strings(&container).map(|values| values.len())
        }
        FeaturePayloadReferenceRoute::PrimaryBody => {
            feature_body_references(&container).map(|values| values.len())
        }
        FeaturePayloadReferenceRoute::BodyOccurrences => {
            feature_body_reference_occurrences(&container).map(|values| values.len())
        }
    };
    let admitted = route().expect("admitted feature payload route");
    assert_eq!(admitted, 1);
}

#[test]
fn input_block_admits_fixture() {
    let store = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload = composed_feature_history_payload_over_sort_scratch(
        &[(
            &[1, 0xff, 0xff, 0xff],
            "EXTRUDE",
            b"\x01\x02\x10\x07\xff".to_vec(),
        )],
        &store,
    );
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic input-block container");
    let route = || feature_input_blocks(&container);
    let admitted = route().expect("admitted input block");
    assert_eq!(admitted.len(), 1);
}

#[test]
fn input_block_group_admits_fixture() {
    let input = |id: &str, operation: &str, offset| crate::native::features::FeatureInputBlock {
        id: id.to_string(),
        operation_label: operation.to_string(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(7, &[7])
            .expect("input object token"),
        data_block: "block#7".to_string(),
        source_offset: offset,
    };
    let inputs = [
        input("input#0", "operation#0", 20),
        input("input#1", "operation#1", 30),
    ];
    let route = || feature_input_block_identity_groups(&inputs);
    let admitted = route();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].members.len(), 2);
}

macro_rules! feature_payload_reference_admit_test {
    ($name:ident, $route:expr) => {
        #[test]
        fn $name() {
            assert_feature_payload_reference_admits($route);
        }
    };
}

feature_payload_reference_admit_test!(
    payload_string_route_admits_fixture,
    FeaturePayloadReferenceRoute::Text
);
feature_payload_reference_admit_test!(
    primary_body_route_admits_fixture,
    FeaturePayloadReferenceRoute::PrimaryBody
);
feature_payload_reference_admit_test!(
    body_occurrence_route_admits_fixture,
    FeaturePayloadReferenceRoute::BodyOccurrences
);

macro_rules! operation_frame_admit_test {
    ($name:ident, $terminal:expr) => {
        #[test]
        fn $name() {
            assert_operation_common_frame_admits($terminal);
        }
    };
}

operation_frame_admit_test!(common_frame_route_admits_fixture, false);
operation_frame_admit_test!(terminal_frame_route_admits_fixture, true);

#[test]
fn operation_body_write_admits_fixture() {
    let body_write = b"\x01\x02\x0b\x31\x97\x75\x01\x02\x10\x41\xff";
    let store = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload = composed_feature_history_payload_over_sort_scratch(
        &[(&[0xff; 4], "EXTRUDE", body_write.to_vec())],
        &store,
    );
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic body-write container");
    let admitted =
        feature_operation_body_writes(&container).expect("admitted operation body writes");
    assert_eq!(admitted.len(), 1);
    assert!(admitted[0].body_image_data_block.is_some());
}

#[test]
fn operation_body_write_resolves_one_unique_image_block() {
    let body_write = vec![
        0x01, 0x02, 0x0b, 0x31, 0x97, 0x75, 0x01, 0x02, 0x10, 0x41, 0xff,
    ];
    let store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload =
        composed_feature_history_payload(&[(&[0xff; 4], "EXTRUDE", body_write)], &store_records);
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic body-image store");

    let writes = feature_operation_body_writes(&container).unwrap();

    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].frame.body_image().value(), 65);
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
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic body-image store");
    let writes = feature_operation_body_writes(&container).unwrap();
    let binding = |id: &str, stream_kind: crate::parasolid::StreamKind| SegmentBodyBinding {
        id: id.to_string(),
        stream_link: format!("{id}:link"),
        stream_ordinal: 0,
        stream_kind,
        body_object_index: 42,
        body_alias_object_index: 11,
        stream_role: 10,
        source_offset: 100,
    };

    let uses = image_segment_uses_for_test(
        &writes,
        &[
            binding("plain", crate::parasolid::StreamKind::Plain),
            binding("partition", crate::parasolid::StreamKind::Partition),
        ],
    );

    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].operation_body_write, writes[0].id);
    assert_eq!(
        uses[0].body_image_data_block,
        "nx:om-data-blocks-0:block#65"
    );
    assert_eq!(uses[0].segment_body_binding, "plain");
    assert!(image_segment_uses_for_test(
        &writes,
        &[
            binding("first", crate::parasolid::StreamKind::Plain),
            binding("second", crate::parasolid::StreamKind::Plain)
        ],
    )
    .is_empty());
}

#[test]
fn body_identity_segment_use_does_not_require_an_image_block() {
    let mut write = FeatureOperationBodyWrite {
        id: "nx:operation-body-write#0".into(),
        operation_label: Some("operation".into()),
        operation_record: "record".into(),
        ordinal: 0,
        frame: crate::om::body_write::BodyWriteFrame::<u64>::new(
            11,
            crate::om::body_write::BodyWriteIndex::from_wire(1, &[1]).unwrap(),
            crate::om::body_write::BodyImageTag::Form12,
            crate::om::body_write::BodyWriteIndex::from_wire(1519, &[0x85, 0xef]).unwrap(),
            0,
        )
        .unwrap(),
        body_image_data_block: None,
    };
    let binding = |id: &str, stream_kind: crate::parasolid::StreamKind| SegmentBodyBinding {
        id: id.into(),
        stream_link: format!("{id}:link"),
        stream_ordinal: 1,
        stream_kind,
        body_object_index: 42,
        body_alias_object_index: 11,
        stream_role: 10,
        source_offset: 100,
    };

    let uses = identity_segment_uses_for_test(
        std::slice::from_ref(&write),
        &[
            binding("plain", crate::parasolid::StreamKind::Plain),
            binding("partition", crate::parasolid::StreamKind::Partition),
        ],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].operation_body_write, write.id);
    assert_eq!(uses[0].body_identity, 11);
    assert_eq!(uses[0].segment_body_binding, "plain");

    write.body_image_data_block = Some("irrelevant".into());
    assert!(identity_segment_uses_for_test(
        &[write],
        &[
            binding("first", crate::parasolid::StreamKind::Plain),
            binding("second", crate::parasolid::StreamKind::Plain)
        ],
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
    let container = {
        crate::container::scan_bytes(prt_with_named_payloads(&[(
            "/Root/UG_PART/UG_PART",
            payload,
        )]))
    }
    .expect("synthetic body-image store");
    let writes = feature_operation_body_writes(&container).unwrap();
    let binding =
        |id: &str, stream_ordinal, body_alias_object_index, stream_role| SegmentBodyBinding {
            id: id.to_string(),
            stream_link: format!("{id}:link"),
            stream_ordinal,
            stream_kind: crate::parasolid::StreamKind::Plain,
            body_object_index: 42,
            body_alias_object_index,
            stream_role,
            source_offset: 100,
        };
    let bindings = [binding("plain-0", 0, 11, 10), binding("plain-1", 1, 12, 16)];
    let image_uses = image_segment_uses_for_test(&writes, &bindings);
    let stream = |subtype| crate::parasolid::Stream {
        file_offset: 0,
        consumed: 0,
        inflated: Vec::new(),
        body: crate::parasolid::StreamBody::Parasolid {
            subtype,
            schema: Some(
                cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST")
                    .expect("the fixture text is a schema token"),
            ),
        },
    };
    let streams = [
        stream(crate::parasolid::ParasolidSubtype::Plain),
        stream(crate::parasolid::ParasolidSubtype::Plain),
        stream(crate::parasolid::ParasolidSubtype::Partition),
        stream(crate::parasolid::ParasolidSubtype::Deltas),
        stream(crate::parasolid::ParasolidSubtype::Partition),
    ];
    let group =
        |id: &str, partition_stream_ordinal| crate::native::parasolid::ParasolidGroupRecord {
            id: id.into(),
            origin: crate::native::parasolid::group_record::GroupOrigin::Deltas {
                stream_ordinal: partition_stream_ordinal + 1,
                partition_stream_ordinal: Some(partition_stream_ordinal),
            },
            xmt: 10,
            node_id: writes[0].frame.group_node().value(),
            references: [3, 4, 5, 6, 7],
            selector: crate::deltas::group::GroupSelector::Form4,
            linked_reference_status: crate::deltas::group::GroupReferenceStatus::Form0,
            byte_len: 20,
            inflated_offset: 0,
        };
    let groups = [group("owned", 2), group("collision", 4)];

    let uses =
        body_partition_uses_for_test(&writes, &image_uses, &bindings, &streams, &groups, &[]);

    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].partition_stream_ordinal, 2);
    assert_eq!(uses[0].parasolid_group_records, ["owned"]);

    let unterminated = [binding("plain-0", 0, 11, 10), binding("plain-1", 1, 12, 10)];
    assert!(body_partition_uses_for_test(
        &writes,
        &image_uses,
        &unterminated,
        &streams,
        &groups,
        &[],
    )
    .is_empty());

    let repeated_terminal = [binding("plain-0", 0, 11, 16), binding("plain-1", 1, 12, 16)];
    assert!(
        body_history_partition_stream(&repeated_terminal[1], &repeated_terminal, &streams,)
            .is_none()
    );

    let interrupted_streams = [
        stream(crate::parasolid::ParasolidSubtype::Plain),
        stream(crate::parasolid::ParasolidSubtype::Deltas),
        stream(crate::parasolid::ParasolidSubtype::Partition),
    ];
    assert!(body_partition_uses_for_test(
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
    let unlabeled = FeatureOperationBodyWrite {
        operation_label: None,
        id: "unlabeled-body-write".into(),
        operation_record: "unlabeled-record".into(),
        ordinal: 0,
        frame: crate::om::body_write::BodyWriteFrame::<u64>::new(
            11,
            crate::om::body_write::BodyWriteIndex::from_wire(99, &[99]).unwrap(),
            crate::om::body_write::BodyImageTag::Form10,
            crate::om::body_write::BodyWriteIndex::from_wire(20, &[20]).unwrap(),
            9,
        )
        .unwrap(),
        body_image_data_block: Some("block".into()),
    };
    let group =
        |id: &str, partition_stream_ordinal| crate::native::parasolid::ParasolidGroupRecord {
            id: id.into(),
            origin: crate::native::parasolid::group_record::GroupOrigin::Deltas {
                stream_ordinal: partition_stream_ordinal + 1,
                partition_stream_ordinal: Some(partition_stream_ordinal),
            },
            xmt: 10,
            node_id: 99,
            references: [3, 4, 5, 6, 7],
            selector: crate::deltas::group::GroupSelector::Form4,
            linked_reference_status: crate::deltas::group::GroupReferenceStatus::Form0,
            byte_len: 20,
            inflated_offset: 0,
        };

    let uses = body_group_partition_uses_for_test(
        &[],
        std::slice::from_ref(&unlabeled),
        &[group("owned", 2)],
        &[],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].body_identity, 11);
    assert_eq!(uses[0].partition_stream_ordinal, 2);
    assert_eq!(uses[0].parasolid_group_records, ["owned"]);

    assert!(body_group_partition_uses_for_test(
        &[],
        &[unlabeled],
        &[group("first", 2), group("collision", 4)],
        &[],
    )
    .is_empty());
}

#[test]
fn body_group_partition_admits_fixture() {
    let write = FeatureOperationBodyWrite {
        operation_label: None,
        id: "unlabeled-body-write".into(),
        operation_record: "unlabeled-record".into(),
        ordinal: 0,
        frame: crate::om::body_write::BodyWriteFrame::<u64>::new(
            11,
            crate::om::body_write::BodyWriteIndex::from_wire(99, &[99]).expect("group token"),
            crate::om::body_write::BodyImageTag::Form10,
            crate::om::body_write::BodyWriteIndex::from_wire(20, &[20]).expect("image token"),
            9,
        )
        .expect("body-write frame"),
        body_image_data_block: Some("block".into()),
    };
    let group = crate::native::parasolid::ParasolidGroupRecord {
        id: "owned".into(),
        origin: crate::native::parasolid::group_record::GroupOrigin::Deltas {
            stream_ordinal: 3,
            partition_stream_ordinal: Some(2),
        },
        xmt: 10,
        node_id: 99,
        references: [3, 4, 5, 6, 7],
        selector: crate::deltas::group::GroupSelector::Form4,
        linked_reference_status: crate::deltas::group::GroupReferenceStatus::Form0,
        byte_len: 20,
        inflated_offset: 0,
    };
    let route = || {
        feature_body_write_group_partition_uses(
            &[],
            std::slice::from_ref(&write),
            std::slice::from_ref(&group),
            &[],
        )
    };
    let admitted = route();
    assert_eq!(admitted.len(), 1);
}

fn journal_row(state_ordinal: u32, source_offset: u64) -> JournalRow {
    JournalRow::new(
        source_offset,
        1_700_000_000,
        crate::om::state_tagged_value::StateTaggedValue::read_at(
            &[
                0xe0,
                0,
                0,
                0,
                u8::try_from(state_ordinal).expect("fixture value fits u8"),
            ],
            0,
        )
        .unwrap(),
        crate::om::state_index::StateIndexToken::read_at(&[12], 0).unwrap(),
        crate::om::state_index::StateIndexToken::read_at(
            &[u8::try_from(state_ordinal).expect("fixture value fits u8")],
            0,
        )
        .unwrap(),
    )
    .unwrap()
}

fn journal_group(
    id: &str,
    section_link: &str,
    rows: Vec<JournalRow>,
) -> OmOperationStateJournalGroup {
    let source_offset = rows[0].offset() - 4;
    OmOperationStateJournalGroup {
        id: id.to_string(),
        section_link: section_link.to_string(),
        ordinal: 0,
        frame: crate::om::journal_group::JournalGroup::new([4, 0], source_offset, rows).unwrap(),
        source_entry: "/Root/UG_PART/UG_PART".to_string(),
    }
}

fn operation_record(id: &str, operation_label: &str) -> FeatureOperationRecord {
    FeatureOperationRecord {
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        ordinal: 0,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(b"record-sha256"),
        payload_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(b"payload-sha256"),
        stable_identity: None,
        span: crate::native::features::operation_record::OperationRecordSpan::new(400, 404, 8)
            .unwrap(),
    }
}

fn terminal_frame(operation_record: &str, local_ordinal: u32) -> FeatureOperationTerminalFrame {
    FeatureOperationTerminalFrame {
        id: "nx:feature-history:operation-terminal-frame#0000000000-0000000000".to_string(),
        operation_record: operation_record.to_string(),
        immediate_common_frame: None,
        frame: crate::om::common_frame::TerminalFrame::<u64, Option<String>>::new(
            crate::om::common_frame::CommonFrameSuffix::from_wire(
                local_ordinal,
                &[u8::try_from(local_ordinal).expect("fixture value fits u8")],
                None,
                &[0xff],
            )
            .unwrap()
            .with_target(None)
            .unwrap(),
            420,
        )
        .unwrap(),
    }
}

fn state_journal_uses_for_test(
    labels: &[FeatureOperationLabel],
    records: &[FeatureOperationRecord],
    terminal_frames: &[FeatureOperationTerminalFrame],
    groups: &[OmOperationStateJournalGroup],
) -> Vec<FeatureOperationStateJournalUse> {
    feature_operation_state_journal_uses(labels, records, terminal_frames, groups)
}

#[test]
fn state_journal_use_admits_fixture() {
    let label = label(0, [None; 4]);
    let record = operation_record(
        "nx:feature-history:operation-record#0000000000-0000000000",
        &label.id,
    );
    let group = journal_group(
        "nx:feature-history:operation-state-journal-group#0000000000-0000000000",
        &label.section_link,
        vec![journal_row(7, 520)],
    );
    let frame = terminal_frame(&record.id, 7);
    let route = || {
        feature_operation_state_journal_uses(
            std::slice::from_ref(&label),
            std::slice::from_ref(&record),
            std::slice::from_ref(&frame),
            std::slice::from_ref(&group),
        )
    };
    let admitted = route();
    assert_eq!(admitted.len(), 1);
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
        vec![journal_row(6, 507), journal_row(7, 520)],
    );
    let frame = terminal_frame(&record.id, 7);

    let uses = state_journal_uses_for_test(
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
    assert_eq!(relation.state_ordinal, 7);
    assert_eq!(relation.operation_source_offset, 420);
    assert_eq!(relation.journal_source_offset, 520);
    let wire = serde_json::to_value(relation).unwrap();
    assert_eq!(wire["state_ordinal"], 7);
    let admitted: FeatureOperationStateJournalUse = serde_json::from_value(wire).unwrap();
    assert_eq!(&admitted, relation);
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

    let section_scoped = state_journal_uses_for_test(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        &[wrong_section, matching.clone()],
    );
    assert_eq!(section_scoped.len(), 1);
    assert_eq!(section_scoped[0].journal_source_offset, 620);

    let ambiguous = state_journal_uses_for_test(
        std::slice::from_ref(&label),
        std::slice::from_ref(&record),
        std::slice::from_ref(&frame),
        &[matching, duplicate],
    );
    assert!(ambiguous.is_empty());
}
