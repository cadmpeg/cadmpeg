// SPDX-License-Identifier: Apache-2.0

use crate::native::features::assign_operation_header_identities;
use crate::native::features::body_history_partition_stream;
use crate::native::features::feature_body_write_group_partition_uses;
use crate::native::features::feature_operation_body_identity_segment_uses;
use crate::native::features::feature_operation_body_image_segment_uses;
use crate::native::features::feature_operation_body_partition_uses;
use crate::native::features::feature_operation_body_writes;
use crate::native::features::feature_operation_labels;
use crate::native::features::feature_operation_records;
use crate::native::features::feature_unlabeled_operation_records;
use crate::native::features::feature_operation_state_journal_uses;
use crate::native::features::operation_record::FeatureOperationRecord;
use crate::native::features::FeatureOperationBodyWrite;
use crate::native::features::FeatureOperationLabel;
use crate::native::features::FeatureOperationStateJournalUse;
use crate::native::features::FeatureOperationTerminalFrame;
use crate::native::om::journal_group::OmOperationStateJournalGroup;
use crate::native::segments::SegmentBodyBinding;
use crate::om::state_journal::JournalRow;
use crate::test_support::test_om::composed_feature_history_payload;
use crate::test_support::test_om::composed_feature_history_section;
use crate::test_support::test_prt::prt_with_named_payloads;
use std::collections::BTreeMap;

fn image_segment_uses_for_test(
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Vec<crate::native::features::FeatureOperationBodyImageSegmentUse> {
    crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_image_segment_uses(ctx, writes, bindings)
    }).expect("admitted body-image segment uses")
}

fn identity_segment_uses_for_test(
    writes: &[FeatureOperationBodyWrite],
    bindings: &[SegmentBodyBinding],
) -> Vec<crate::native::features::FeatureOperationBodyIdentitySegmentUse> {
    crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_identity_segment_uses(ctx, writes, bindings)
    }).expect("admitted body-identity segment uses")
}

fn body_partition_uses_for_test(
    writes: &[FeatureOperationBodyWrite],
    image_uses: &[crate::native::features::FeatureOperationBodyImageSegmentUse],
    bindings: &[SegmentBodyBinding],
    streams: &[crate::parasolid::Stream],
    groups: &[crate::native::parasolid::ParasolidGroupRecord],
    members: &[crate::native::parasolid::ParasolidGroupMember],
) -> Vec<crate::native::features::FeatureOperationBodyPartitionUse> {
    crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_partition_uses(
            ctx, writes, image_uses, bindings, streams, groups, members,
        )
    }).expect("admitted body partition uses")
}

fn body_segment_join_refusal<T>(
    route: for<'ctx> fn(
        &cadmpeg_core::decode::DecodeContext<'ctx>,
        &[FeatureOperationBodyWrite],
        &[SegmentBodyBinding],
    ) -> Result<Vec<T>, cadmpeg_core::CodecError>,
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
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
        ).expect("body-write frame"),
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
    let admitted = crate::test_support::with_decode_context(|ctx| {
        route(ctx, std::slice::from_ref(&write), std::slice::from_ref(&binding))
    }).expect("admitted body segment use");
    assert_eq!(admitted.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    route(&ctx, &[write], &[binding]).err()
        .expect("body segment use resource limit")
}

macro_rules! body_segment_join_limit_tests {
    ($collection:ident, $retained:ident, $work:ident, $route:path) => {
        #[test]
        fn $collection() {
            let error = body_segment_join_refusal($route,
                |policy| policy.limits.max_collection_items = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
        }

        #[test]
        fn $retained() {
            let error = body_segment_join_refusal($route,
                |policy| policy.limits.max_retained_bytes = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        }

        #[test]
        fn $work() {
            let error = body_segment_join_refusal($route,
                |policy| policy.limits.max_work_units = 0);
            assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
        }
    };
}

body_segment_join_limit_tests!(
    body_image_join_refuses_collection_limit,
    body_image_join_refuses_retained_limit,
    body_image_join_refuses_work_limit,
    feature_operation_body_image_segment_uses
);
body_segment_join_limit_tests!(
    body_identity_join_refuses_collection_limit,
    body_identity_join_refuses_retained_limit,
    body_identity_join_refuses_work_limit,
    feature_operation_body_identity_segment_uses
);

fn body_partition_join_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
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
        ).expect("body-write frame"),
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
            schema: Some(cadmpeg_parasolid::OwnedSchemaToken::try_from("SCH_TEST")
                .expect("schema token")),
        },
    };
    let streams = [
        stream(crate::parasolid::ParasolidSubtype::Plain),
        stream(crate::parasolid::ParasolidSubtype::Partition),
    ];
    let route = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        feature_operation_body_partition_uses(
            ctx, std::slice::from_ref(&write), std::slice::from_ref(&image_use),
            std::slice::from_ref(&binding), &streams, &[], &[],
        )
    };
    let admitted = crate::test_support::with_decode_context(|ctx| route(ctx))
        .expect("admitted body partition use");
    assert_eq!(admitted.len(), 1);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    route(&ctx).expect_err("body partition use resource limit")
}

#[test]
fn body_partition_join_refuses_collection_limit() {
    let error = body_partition_join_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn body_partition_join_refuses_retained_limit() {
    let error = body_partition_join_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn body_partition_join_refuses_work_limit() {
    let error = body_partition_join_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
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
    let payload_len = (section.len() - 16) as u32;
    section[8..12].copy_from_slice(&payload_len.to_be_bytes());
    let mut payload = Vec::new();
    for word in [32u32, 9, 11, 1, 1, 24] {
        payload.extend_from_slice(&word.to_le_bytes());
    }
    payload.resize(32, 0);
    payload.extend_from_slice(&section);
    crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx, prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]))
    })
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
    crate::test_support::with_decode_context(|ctx| {
        assign_operation_header_identities(ctx, &mut original, &block_identities)
    })
    .unwrap();
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
    crate::test_support::with_decode_context(|ctx| {
        assign_operation_header_identities(ctx, &mut reordered, &block_identities)
    })
    .unwrap();
    assert_eq!(
        reordered[1].stable_identity.as_deref(),
        Some(identity.as_str())
    );
    assert!(reordered[2].stable_identity.is_none());
}

#[test]
fn feature_label_identity_retains_the_complete_header_ordinal() {
    let container = unlabeled_history_fixture();

    let labels =
        crate::test_support::with_decode_context(|ctx| feature_operation_labels(ctx, &container))
            .unwrap();
    assert_eq!(labels.len(), 2);
    assert!(labels[0].id.ends_with("-0000000000"));
    assert!(labels[1].id.ends_with("-0000000002"));
    let records =
        crate::test_support::with_decode_context(|ctx| feature_operation_records(ctx, &container))
            .unwrap();
    assert_eq!(records[1].operation_label, labels[1].id);
}

fn feature_label_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = unlabeled_history_fixture();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_operation_labels(&ctx, &container).unwrap_err()
}

fn feature_operation_record_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = unlabeled_history_fixture();
    let admitted = crate::test_support::with_decode_context(|ctx| {
        feature_operation_records(ctx, &container)
    }).expect("admitted feature operation records");
    assert_eq!(admitted.len(), 2);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_operation_records(&ctx, &container)
        .expect_err("feature operation record resource limit")
}

#[test]
fn feature_operation_record_route_refuses_collection_limit() {
    let error = feature_operation_record_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn feature_operation_record_route_refuses_retained_limit() {
    let error = feature_operation_record_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn feature_operation_record_route_refuses_scoped_limit() {
    let error = feature_operation_record_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn feature_operation_record_route_refuses_work_limit() {
    let error = feature_operation_record_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn feature_label_route_refuses_collection_limit() {
    let error = feature_label_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn feature_label_route_refuses_retained_limit() {
    let error = feature_label_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn feature_label_route_refuses_scoped_limit() {
    let error = feature_label_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn feature_label_route_refuses_work_limit() {
    let error = feature_label_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn feature_label_identity_admits_full_ordinal_width() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    let id = crate::native::features::format_feature_history_id(
        &ctx, "operation-label", "0000000000", usize::MAX, None,
    ).expect("admitted label identity");
    assert_eq!(
        id,
        format!("nx:feature-history:operation-label#0000000000-{value:010}", value = usize::MAX),
    );
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(id.len() - 1);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    let error = crate::native::features::format_feature_history_id(
        &ctx, "operation-label", "0000000000", usize::MAX, None,
    ).expect_err("full identity exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

fn unlabeled_record_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = unlabeled_history_fixture();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_unlabeled_operation_records(&ctx, &container).unwrap_err()
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
fn unlabeled_record_route_refuses_collection_limit() {
    let error = unlabeled_record_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn unlabeled_record_route_refuses_retained_limit() {
    let error = unlabeled_record_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn unlabeled_record_route_refuses_work_limit() {
    let error = unlabeled_record_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
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
    crate::test_support::with_decode_context(|ctx| {
        assign_operation_header_identities(ctx, &mut labels, &block_identities)
    })
    .unwrap();
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
    let first = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", first_payload)]),
        )
    })
    .expect("first synthetic container");
    let second = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", second_payload)]),
        )
    })
    .expect("second synthetic container");

    let first_labels =
        crate::test_support::with_decode_context(|ctx| feature_operation_labels(ctx, &first))
            .unwrap();
    let second_labels =
        crate::test_support::with_decode_context(|ctx| feature_operation_labels(ctx, &second))
            .unwrap();
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
    crate::test_support::with_decode_context(|ctx| {
        assign_operation_header_identities(ctx, &mut labels, &block_identities)
    })
    .unwrap();
    assert!(labels[0].stable_identity.is_none());
}

#[test]
fn feature_operation_identity_refuses_scoped_keys_at_caller_limit() {
    let block_identities = BTreeMap::from([(55, Some("block-55".to_string()))]);
    let mut labels = vec![label(0, [Some(55), None, None, None])];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = assign_operation_header_identities(&ctx, &mut labels, &block_identities)
        .err()
        .expect("scoped key refusal");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes
                && limit.operation == "reserve NX operation header keys"
    ));
}

#[test]
fn operation_body_write_retains_identity_group_and_image() {
    let body_writes = vec![
        0x01, 0x02, 0x11, 0x80, 0xa9, 0x97, 0x75, 0x01, 0x02, 0x10, 0x86, 0x93, 0xff, 0x01, 0x02,
        0x12, 0x80, 0xa9, 0x97, 0x75, 0x01, 0x02, 0x10, 0x86, 0x94, 0xff,
    ];
    let payload = composed_feature_history_payload(&[(&[0xff; 4], "EXTRUDE", body_writes)], &[]);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]),
        )
    })
    .expect("synthetic body-write container");
    let writes = crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_writes(ctx, &container)
    })
    .unwrap();
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

fn operation_body_write_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let body_write = b"\x01\x02\x0b\x31\x97\x75\x01\x02\x10\x41\xff";
    let store = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload = composed_feature_history_payload(
        &[(&[0xff; 4], "EXTRUDE", body_write.to_vec())], &store,
    );
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx, prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]),
        )
    }).expect("synthetic body-write container");
    let admitted = crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_writes(ctx, &container)
    }).expect("admitted operation body writes");
    assert_eq!(admitted.len(), 1);
    assert!(admitted[0].body_image_data_block.is_some());
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    feature_operation_body_writes(&ctx, &container)
        .expect_err("operation body-write resource limit")
}

#[test]
fn operation_body_write_route_refuses_collection_limit() {
    let error = operation_body_write_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn operation_body_write_route_refuses_retained_limit() {
    let error = operation_body_write_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn operation_body_write_route_refuses_scoped_limit() {
    let error = operation_body_write_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn operation_body_write_route_refuses_work_limit() {
    let error = operation_body_write_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn operation_body_write_resolves_one_unique_image_block() {
    let body_write = vec![
        0x01, 0x02, 0x0b, 0x31, 0x97, 0x75, 0x01, 0x02, 0x10, 0x41, 0xff,
    ];
    let store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload =
        composed_feature_history_payload(&[(&[0xff; 4], "EXTRUDE", body_write)], &store_records);
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]),
        )
    })
    .expect("synthetic body-image store");

    let writes = crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_writes(ctx, &container)
    })
    .unwrap();

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
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]),
        )
    })
    .expect("synthetic body-image store");
    let writes = crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_writes(ctx, &container)
    })
    .unwrap();
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
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(
            ctx,
            prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]),
        )
    })
    .expect("synthetic body-image store");
    let writes = crate::test_support::with_decode_context(|ctx| {
        feature_operation_body_writes(ctx, &container)
    })
    .unwrap();
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

    let uses = body_partition_uses_for_test(
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
        crate::test_support::with_decode_context(|ctx| body_history_partition_stream(
            ctx, &repeated_terminal[1], &repeated_terminal, &streams,
        )).expect("admitted body-history partition")
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

    let uses = feature_body_write_group_partition_uses(
        &[],
        std::slice::from_ref(&unlabeled),
        &[group("owned", 2)],
        &[],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].body_identity, 11);
    assert_eq!(uses[0].partition_stream_ordinal, 2);
    assert_eq!(uses[0].parasolid_group_records, ["owned"]);

    assert!(feature_body_write_group_partition_uses(
        &[],
        &[unlabeled],
        &[group("first", 2), group("collision", 4)],
        &[],
    )
    .is_empty());
}

fn journal_row(state_ordinal: u32, source_offset: u64) -> JournalRow {
    JournalRow::new(
        source_offset,
        1_700_000_000,
        crate::om::state_tagged_value::StateTaggedValue::read_at(
            &[0xe0, 0, 0, 0, state_ordinal as u8],
            0,
        )
        .unwrap(),
        crate::om::state_index::StateIndexToken::read_at(&[12], 0).unwrap(),
        crate::om::state_index::StateIndexToken::read_at(&[state_ordinal as u8], 0).unwrap(),
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
        sha256: crate::native::hex::Sha256Hex::digest(b"record-sha256"),
        payload_sha256: crate::native::hex::Sha256Hex::digest(b"payload-sha256"),
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
                &[local_ordinal as u8],
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
