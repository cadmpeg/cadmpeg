// SPDX-License-Identifier: Apache-2.0

use crate::native::features::feature_operation_chronological_labels;
use crate::native::features::payload_name::FeaturePayloadName;
use crate::native::features::unique_feature_body_references;
use crate::native::features::FeatureBodyReference;
use crate::native::features::FeatureInputBlock;
use crate::native::features::FeatureOperationCommonFrame;
use crate::native::features::FeatureOperationLabel;
use crate::native::features::FeatureOperationTerminalFrame;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_om::composed_feature_history_payload;
use crate::test_support::test_prt::prt_with_named_payloads;
use crate::NxCodec;

const SYNTHETIC_BOOLEAN_PAYLOAD: &[u8] = b"\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x01\x00\x01\x03\x02\x03\x00";

fn boolean_native_route_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let part = composed_feature_history_payload(
        &[(&[0xff; 4], "SUBTRACT", SYNTHETIC_BOOLEAN_PAYLOAD.to_vec())],
        &[],
    );
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)]);
    let container = crate::test_support::with_decode_context(move |ctx| {
        crate::container::scan_bytes(ctx, file)
    })
    .expect("Boolean native container");
    let admitted = crate::test_support::with_decode_context(|ctx| {
        crate::native::features::feature_boolean_operations(ctx, &container)
    })
    .expect("admitted native Boolean operations");
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].tools.len(), 2);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            crate::native::features::feature_boolean_operations(ctx, &container)
                .expect_err("native Boolean resource limit")
        },
    )
}

#[test]
fn native_boolean_route_refuses_collection_limit() {
    let error = boolean_native_route_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn native_boolean_route_refuses_retained_limit() {
    let error = boolean_native_route_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn native_boolean_route_refuses_scoped_limit() {
    let error = boolean_native_route_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn native_boolean_route_refuses_work_limit() {
    let error = boolean_native_route_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn feature_body_segment_uses(
    references: &[FeatureBodyReference],
    data_block_uses: &[crate::native::features::FeatureBodyDataBlockUse],
    inputs: &[FeatureInputBlock],
    blocks: &[crate::native::om::DataBlock],
    bindings: &[crate::native::segments::SegmentBodyBinding],
    object_frames: &[crate::native::features::object_frame::DataBlockObjectFrame],
) -> Vec<crate::native::features::FeatureBodySegmentUse> {
    crate::test_support::with_decode_context(|ctx| {
        crate::native::features::feature_body_segment_uses(
            ctx,
            references,
            data_block_uses,
            inputs,
            blocks,
            bindings,
            object_frames,
        )
    })
    .expect("admitted feature body segment uses")
}

fn body_segment_use_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    use crate::native::om::{DataBlock, DataBlockRole};
    let reference = FeatureBodyReference {
        ordinal: None,
        id: "nx:feature-history:body-reference#0".to_string(),
        operation_label: "operation#0".to_string(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(11, &[11])
            .expect("reference token"),
        source_offset: 0,
    };
    let binding = crate::native::segments::SegmentBodyBinding {
        id: "binding#0".to_string(),
        stream_link: "stream#0".to_string(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 0,
    };
    let input = FeatureInputBlock {
        id: "input#0".to_string(),
        operation_label: "other-operation".to_string(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(1, &[1])
            .expect("input token"),
        data_block: "block#1".to_string(),
        source_offset: 0,
    };
    let block = DataBlock {
        id: "block#1".to_string(),
        section_ordinal: 0,
        block_ordinal: 1,
        role: DataBlockRole::Column,
        section_offset: 0,
        byte_len: 0,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        stable_identity: None,
        source_entry: String::new(),
        source_offset: 0,
    };
    let route = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        crate::native::features::feature_body_segment_uses(
            ctx,
            std::slice::from_ref(&reference),
            &[],
            std::slice::from_ref(&input),
            std::slice::from_ref(&block),
            std::slice::from_ref(&binding),
            &[],
        )
    };
    let admitted = crate::test_support::with_decode_context(|ctx| route(ctx))
        .expect("admitted body segment uses");
    assert_eq!(admitted.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| route(ctx).expect_err("body segment use resource limit"),
    )
}

#[test]
fn body_segment_uses_refuse_collection_limit() {
    let error = body_segment_use_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn body_segment_uses_refuse_retained_limit() {
    let error = body_segment_use_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn body_segment_uses_refuse_scoped_limit() {
    let error = body_segment_use_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn body_segment_uses_refuse_work_limit() {
    let error = body_segment_use_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn chronological_label_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let labels = [FeatureOperationLabel {
        id: "operation#0".to_string(),
        section_link: "section#0".to_string(),
        ordinal: 0,
        value: "EXTRUDE".to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 10,
    }];
    let admitted = crate::test_support::with_decode_context(|ctx| {
        feature_operation_chronological_labels(ctx, &labels)
    })
    .expect("admitted chronological labels");
    assert_eq!(admitted.len(), 1);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            configure(policy);
        },
        |ctx| {
            feature_operation_chronological_labels(ctx, &labels)
                .expect_err("chronological label resource limit")
        },
    )
}

#[test]
fn chronological_labels_refuse_collection_limit() {
    let error = chronological_label_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn chronological_labels_refuse_retained_limit() {
    let error = chronological_label_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn chronological_labels_refuse_scoped_limit() {
    let error = chronological_label_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes)
    );
}

#[test]
fn chronological_labels_refuse_work_limit() {
    let error = chronological_label_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

#[test]
fn segment_body_lineage_statuses_cover_every_bound_image() {
    use crate::native::features::FeatureBodyReference;
    use crate::native::features::FeatureBooleanKind;
    use crate::native::features::FeatureBooleanOperation;
    use crate::native::features::FeatureOperationLabel;
    use crate::native::segments::{segment_body_lineage_statuses, SegmentBodyBinding};
    let labels = [
        FeatureOperationLabel {
            id: "operation#0".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 0,
            value: "EXTRUDE".to_string(),
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 0,
        },
        FeatureOperationLabel {
            id: "operation#1".to_string(),
            section_link: "history#0".to_string(),
            ordinal: 1,
            value: "UNITE".to_string(),
            objects: crate::om::header_references::HeaderReferences([None; 4]),
            stable_identity: None,
            source_offset: 1,
        },
    ];
    let references = [FeatureBodyReference {
        ordinal: None,
        id: "reference#0".to_string(),
        operation_label: "operation#0".to_string(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(10, &[10]).unwrap(),
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target: crate::test_support::native_references::boolean_reference(10, 1),
        tools: vec![crate::test_support::native_references::boolean_reference(
            21, 1,
        )],
        source_offset: 1,
    }];
    let binding =
        |id: &str, stream_ordinal: u32, stream_kind: crate::parasolid::StreamKind, body, alias| {
            SegmentBodyBinding {
                id: id.to_string(),
                stream_link: format!("stream#{stream_ordinal}"),
                stream_ordinal,
                stream_kind,
                body_object_index: body,
                body_alias_object_index: alias,
                stream_role: 19,
                source_offset: u64::from(stream_ordinal),
            }
        };
    let statuses = crate::test_support::with_decode_context(|ctx| {
        segment_body_lineage_statuses(
            ctx,
            &crate::native::segments::BodyLineageInputs {
                labels: &labels,
                references: &references,
                data_block_uses: &[],
                data_blocks: &[],
                booleans: &booleans,
                operands: &[],
                bindings: &[
                    binding(
                        "binding#0",
                        0,
                        crate::parasolid::StreamKind::Partition,
                        10,
                        11,
                    ),
                    binding("binding#1", 1, crate::parasolid::StreamKind::Plain, 20, 21),
                ],
                inputs: &[],
            },
        )
    })
    .expect("admitted segment lineage statuses")
    .expect("required invariant");
    assert_eq!(statuses.len(), 2);
    assert!(statuses[0].terminal);
    assert!(!statuses[1].terminal);
}

#[test]
fn unique_feature_body_references_require_one_field_per_operation() {
    let reference = |id: &str, operation_label: &str, body_object_index| FeatureBodyReference {
        ordinal: None,
        id: id.to_string(),
        operation_label: operation_label.to_string(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(
            body_object_index,
            &[u8::try_from(body_object_index).expect("fixture value fits u8")],
        )
        .unwrap(),
        source_offset: 0,
    };
    let references = [
        reference("reference#0", "operation#0", 10),
        reference("reference#1", "operation#0", 11),
        reference("reference#2", "operation#1", 12),
    ];
    let unique = crate::test_support::with_decode_context(|ctx| {
        unique_feature_body_references(ctx, &references)
    })
    .expect("admitted unique body references");
    assert!(!unique.contains_key("operation#0"));
    assert_eq!(unique["operation#1"].id, "reference#2");
}

#[test]
fn feature_body_segment_uses_require_one_alias_pair() {
    use crate::native::features::FeatureBodyReference;
    use crate::native::segments::SegmentBodyBinding;
    let reference = FeatureBodyReference {
        ordinal: None,
        id: "nx:feature-history:body-reference#0".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(11, &[11]).unwrap(),
        source_offset: 90,
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#3".into(),
        stream_ordinal: 3,
        stream_kind: crate::parasolid::StreamKind::Plain,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 40,
    };
    let uses = feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[],
        &[],
        &[],
        std::slice::from_ref(&binding),
        &[],
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].feature_body_reference, reference.id);
    assert_eq!(uses[0].segment_body_binding, binding.id);
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[],
        &[],
        &[],
        &[binding.clone(), binding.clone()],
        &[]
    )
    .is_empty());
    let duplicate_reference = FeatureBodyReference {
        ordinal: None,
        id: "nx:feature-history:body-reference#1".into(),
        operation_label: reference.operation_label.clone(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(12, &[12]).unwrap(),
        source_offset: 91,
    };
    assert!(feature_body_segment_uses(
        &[reference, duplicate_reference],
        &[],
        &[],
        &[],
        std::slice::from_ref(&binding),
        &[],
    )
    .is_empty());
}

#[test]
fn feature_body_segment_uses_bridge_unique_offset_store_aliases() {
    use crate::native::features::object_frame::DataBlockObjectFrame;
    use crate::native::features::FeatureBodyDataBlockUse;
    use crate::native::features::FeatureBodyReference;
    use crate::native::om::{DataBlock, DataBlockRole};
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        ordinal: None,
        id: "reference#0".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(11, &[11]).unwrap(),
        source_offset: 90,
    };
    let data_block_use = FeatureBodyDataBlockUse {
        id: "data-block-use#0".into(),
        feature_body_reference: reference.id.clone(),
        data_block: "block#11".into(),
    };
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: reference.operation_label.clone(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(3, &[3]).unwrap(),
        data_block: "block#3".into(),
        source_offset: 80,
    };
    let blocks = [
        DataBlock {
            id: "block#3".into(),
            section_ordinal: 2,
            block_ordinal: 3,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 1,
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
        DataBlock {
            id: "block#11".into(),
            section_ordinal: 2,
            block_ordinal: 11,
            role: DataBlockRole::Column,
            section_offset: 0,
            byte_len: 1,
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
            stable_identity: None,
            source_entry: String::new(),
            source_offset: 0,
        },
    ];
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: 10,
        body_alias_object_index: 11,
        stream_role: 19,
        source_offset: 40,
    };
    let discriminator = [
        0x00, 0x72, 0x01, 0xc0, 0x20, 0x02, 0x01, 0xc0, 0x45, 0x04, 0x00, 0x80, 0x86, 0x02, 0x01,
        0x02, 0x80, 0xa4,
    ];
    let mut block_bytes = Vec::new();
    let object_id_offset = block_bytes.len();
    block_bytes.push(u8::try_from(reference.body.value()).expect("fixture value fits u8"));
    block_bytes.extend_from_slice(&discriminator);
    let parsed_frames = crate::test_support::with_decode_context(|ctx| {
        crate::om::data_block_object_frames(ctx, &block_bytes)
    })
    .unwrap();
    assert_eq!(parsed_frames.len(), 1);
    assert_eq!(parsed_frames[0].offset, object_id_offset);
    let object_frame = DataBlockObjectFrame {
        id: "frame#0".into(),
        data_block: "block#11".into(),
        ordinal: 0,
        object: crate::om::compact::LocatedCompactIndex {
            atom: parsed_frames[0].atom,
            offset: 100,
        },
    };
    let uses = feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
        std::slice::from_ref(&object_frame),
    );
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].segment_body_binding, "binding#0");

    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
        &[],
    )
    .is_empty());

    let mut mismatched_frame = object_frame.clone();
    mismatched_frame.object.atom = crate::om::compact::CompactIndexAtom::read(&[12]).unwrap();
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
        std::slice::from_ref(&mismatched_frame),
    )
    .is_empty());

    let mut wrong_block_frame = object_frame.clone();
    wrong_block_frame.data_block = "block#other".into();
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
        std::slice::from_ref(&wrong_block_frame),
    )
    .is_empty());

    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
        &[object_frame.clone(), object_frame.clone()],
    )
    .is_empty());

    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[data_block_use.clone(), data_block_use.clone()],
        std::slice::from_ref(&input),
        &blocks,
        std::slice::from_ref(&binding),
        std::slice::from_ref(&object_frame),
    )
    .is_empty());

    let second_input = FeatureInputBlock {
        id: "input#1".into(),
        operation_label: reference.operation_label.clone(),
        input_slot: crate::om::header_references::HeaderSlot::One,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(4, &[4]).unwrap(),
        data_block: "block#4".into(),
        source_offset: 81,
    };
    let second_block = DataBlock {
        id: "block#4".into(),
        section_ordinal: 3,
        block_ordinal: 4,
        role: DataBlockRole::Column,
        section_offset: 0,
        byte_len: 1,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        stable_identity: None,
        source_entry: String::new(),
        source_offset: 0,
    };
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        &[input.clone(), second_input],
        &[blocks[0].clone(), blocks[1].clone(), second_block],
        std::slice::from_ref(&binding),
        std::slice::from_ref(&object_frame),
    )
    .is_empty());

    let mut duplicate_alias = binding.clone();
    duplicate_alias.id = "binding#1".into();
    duplicate_alias.body_object_index = 20;
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        &[binding.clone(), duplicate_alias],
        std::slice::from_ref(&object_frame),
    )
    .is_empty());

    let mut primary_collision = binding.clone();
    primary_collision.id = "binding#2".into();
    primary_collision.body_object_index = 11;
    primary_collision.body_alias_object_index = 12;
    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        std::slice::from_ref(&data_block_use),
        std::slice::from_ref(&input),
        &blocks,
        &[binding, primary_collision],
        std::slice::from_ref(&object_frame),
    )
    .is_empty());
}

#[test]
fn feature_body_segment_uses_reject_primary_index_offset_collision() {
    use crate::native::features::FeatureBodyDataBlockUse;
    use crate::native::features::FeatureBodyReference;
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        ordinal: None,
        id: "reference#0".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(11, &[11]).unwrap(),
        source_offset: 90,
    };
    let data_block_use = FeatureBodyDataBlockUse {
        id: "data-block-use#0".into(),
        feature_body_reference: reference.id.clone(),
        data_block: "block#11".into(),
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: 11,
        body_alias_object_index: 12,
        stream_role: 19,
        source_offset: 40,
    };
    assert!(
        feature_body_segment_uses(&[reference], &[data_block_use], &[], &[], &[binding], &[])
            .is_empty()
    );
}

#[test]
fn feature_body_segment_uses_exclude_missing_offset_store_ordinals() {
    use crate::native::features::FeatureBodyReference;
    use crate::native::features::FeatureInputBlock;
    use crate::native::om::{DataBlock, DataBlockRole};
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        ordinal: None,
        id: "reference#99".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(99, &[99]).unwrap(),
        source_offset: 90,
    };
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: reference.operation_label.clone(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(3, &[3]).unwrap(),
        data_block: "block#3".into(),
        source_offset: 80,
    };
    let block = DataBlock {
        id: "block#3".into(),
        section_ordinal: 2,
        block_ordinal: 3,
        role: DataBlockRole::Column,
        section_offset: 10,
        byte_len: 19,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0]),
        stable_identity: None,
        source_entry: "part".into(),
        source_offset: 20,
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Plain,
        body_object_index: 99,
        body_alias_object_index: 100,
        stream_role: 19,
        source_offset: 40,
    };

    assert!(
        feature_body_segment_uses(&[reference], &[], &[input], &[block], &[binding], &[])
            .is_empty()
    );
}

#[test]
fn feature_body_segment_uses_exclude_ambiguous_offset_store_namespaces() {
    use crate::native::features::FeatureBodyReference;
    use crate::native::features::FeatureInputBlock;
    use crate::native::om::{DataBlock, DataBlockRole};
    use crate::native::segments::SegmentBodyBinding;

    let reference = FeatureBodyReference {
        ordinal: None,
        id: "reference#99".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(99, &[99]).unwrap(),
        source_offset: 90,
    };
    let input = |slot: u8, object_index: u32, data_block: &str| FeatureInputBlock {
        id: format!("input#{slot}"),
        operation_label: reference.operation_label.clone(),
        input_slot: crate::om::header_references::HeaderSlot::try_from(slot).unwrap(),
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(
            object_index,
            &[u8::try_from(object_index).expect("fixture value fits u8")],
        )
        .unwrap(),
        data_block: data_block.into(),
        source_offset: 80 + u64::from(slot),
    };
    let block = |id: &str, section_ordinal: u32, block_ordinal: u32| DataBlock {
        id: id.into(),
        section_ordinal,
        block_ordinal,
        role: DataBlockRole::Column,
        section_offset: 10,
        byte_len: 19,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0]),
        stable_identity: None,
        source_entry: "part".into(),
        source_offset: 20,
    };
    let binding = SegmentBodyBinding {
        id: "binding#0".into(),
        stream_link: "stream#0".into(),
        stream_ordinal: 0,
        stream_kind: crate::parasolid::StreamKind::Plain,
        body_object_index: 99,
        body_alias_object_index: 100,
        stream_role: 19,
        source_offset: 40,
    };

    assert!(feature_body_segment_uses(
        std::slice::from_ref(&reference),
        &[],
        &[input(0, 3, "block#3"), input(1, 4, "block#4"),],
        &[block("block#3", 2, 3), block("block#4", 3, 4)],
        &[binding],
        &[],
    )
    .is_empty());
}

#[test]
fn feature_body_data_block_uses_inherit_the_operation_input_store() {
    use crate::native::features::feature_body_data_block_uses;
    use crate::native::features::FeatureBodyReference;
    use crate::native::features::FeatureInputBlock;
    use crate::native::om::{DataBlock, DataBlockRole};

    let reference = FeatureBodyReference {
        ordinal: None,
        id: "nx:feature-history:body-reference#0".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(72, &[72]).unwrap(),
        source_offset: 90,
    };
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: "operation#0".into(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(3, &[3]).unwrap(),
        data_block: "nx:om-data-blocks-2:block#3".into(),
        source_offset: 80,
    };
    let block = |id: &str, section_ordinal, block_ordinal| DataBlock {
        id: id.into(),
        section_ordinal,
        block_ordinal,
        role: DataBlockRole::Column,
        section_offset: 10,
        byte_len: 19,
        sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0]),
        stable_identity: None,
        source_entry: "part".into(),
        source_offset: 20,
    };
    let blocks = [
        block("nx:om-data-blocks-2:block#3", 2, 3),
        block("nx:om-data-blocks-1:block#72", 1, 72),
        block("nx:om-data-blocks-2:block#72", 2, 72),
    ];
    let uses = crate::test_support::with_decode_context(|ctx| {
        feature_body_data_block_uses(
            ctx,
            std::slice::from_ref(&reference),
            std::slice::from_ref(&input),
            &blocks,
        )
    })
    .unwrap();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].data_block, blocks[2].id);
    let duplicate_reference = FeatureBodyReference {
        ordinal: None,
        id: "nx:feature-history:body-reference#1".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(73, &[73]).unwrap(),
        source_offset: 91,
    };
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_body_data_block_uses(ctx, &[reference, duplicate_reference], &[input], &blocks)
    })
    .unwrap()
    .is_empty());
}

#[test]
fn feature_body_data_block_uses_refuse_collection_at_caller_limit() {
    use crate::native::features::{
        feature_body_data_block_uses, FeatureBodyReference, FeatureInputBlock,
    };
    use crate::native::om::{DataBlock, DataBlockRole};

    let reference = FeatureBodyReference {
        ordinal: None,
        id: "nx:feature-history:body-reference#0".into(),
        operation_label: "operation#0".into(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(72, &[72]).unwrap(),
        source_offset: 90,
    };
    let input = FeatureInputBlock {
        id: "input#0".into(),
        operation_label: "operation#0".into(),
        input_slot: crate::om::header_references::HeaderSlot::Zero,
        object: crate::om::reference_index::FeatureReferenceToken::from_wire(3, &[3]).unwrap(),
        data_block: "nx:om-data-blocks-2:block#3".into(),
        source_offset: 80,
    };
    let blocks = [
        DataBlock {
            id: "nx:om-data-blocks-2:block#3".into(),
            section_ordinal: 2,
            block_ordinal: 3,
            role: DataBlockRole::Column,
            section_offset: 10,
            byte_len: 19,
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0]),
            stable_identity: None,
            source_entry: "part".into(),
            source_offset: 20,
        },
        DataBlock {
            id: "nx:om-data-blocks-2:block#72".into(),
            section_ordinal: 2,
            block_ordinal: 72,
            role: DataBlockRole::Column,
            section_offset: 10,
            byte_len: 19,
            sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[0]),
            stable_identity: None,
            source_entry: "part".into(),
            source_offset: 20,
        },
    ];

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_collection_items = 0;
        },
        |ctx| {
            let error = feature_body_data_block_uses(ctx, &[reference], &[input], &blocks)
                .expect_err("body block use needs one collection item");
            assert!(matches!(
                error,
                cadmpeg_core::CodecError::ResourceLimit(limit)
                    if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                        && limit.operation == "NX feature body block uses"
            ));
        },
    );
}

#[test]
fn feature_body_lineage_closes_overlapping_alias_pairs_transitively() {
    use crate::native::features::FeatureBodyReference;
    use crate::native::features::FeatureBooleanKind;
    use crate::native::features::FeatureBooleanOperation;
    use crate::native::features::FeatureOperationLabel;
    use crate::native::segments::{segment_body_lineage_statuses, SegmentBodyBinding};

    let label = |ordinal: u32, value: &str| FeatureOperationLabel {
        id: format!("operation#{ordinal}"),
        section_link: "history#0".to_string(),
        ordinal,
        value: value.to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: 1 - u64::from(ordinal),
    };
    let labels = [label(1, "UNITE"), label(0, "EXTRUDE")];
    let references = [FeatureBodyReference {
        ordinal: None,
        id: "reference#30".to_string(),
        operation_label: "operation#0".to_string(),
        body: crate::om::reference_index::FeatureReferenceToken::from_wire(30, &[30]).unwrap(),
        source_offset: 0,
    }];
    let booleans = [FeatureBooleanOperation {
        id: "boolean#0".to_string(),
        operation_label: "operation#1".to_string(),
        kind: FeatureBooleanKind::Unite,
        target: crate::test_support::native_references::boolean_reference(99, 1),
        tools: vec![crate::test_support::native_references::boolean_reference(
            10, 1,
        )],
        source_offset: 1,
    }];
    let binding = |id: &str, stream_ordinal, body, alias| SegmentBodyBinding {
        id: id.to_string(),
        stream_link: format!("stream#{stream_ordinal}"),
        stream_ordinal,
        stream_kind: crate::parasolid::StreamKind::Partition,
        body_object_index: body,
        body_alias_object_index: alias,
        stream_role: 19,
        source_offset: u64::from(stream_ordinal),
    };
    let bindings = [
        binding("binding#0", 0, 10, 20),
        binding("binding#1", 1, 30, 20),
        binding("binding#2", 2, 40, 20),
    ];

    let statuses = crate::test_support::with_decode_context(|ctx| {
        segment_body_lineage_statuses(
            ctx,
            &crate::native::segments::BodyLineageInputs {
                labels: &labels,
                references: &references,
                data_block_uses: &[],
                data_blocks: &[],
                booleans: &booleans,
                operands: &[],
                bindings: &bindings,
                inputs: &[],
            },
        )
    })
    .expect("admitted segment lineage statuses")
    .expect("required invariant");
    assert_eq!(statuses.len(), 3);
    assert!(statuses.iter().all(|status| !status.terminal));
}

#[test]
fn nx_block_payload_points_require_exactly_two_named_scalars() {
    use crate::native::features::construction_records::feature_block_payload_point_groups;
    use crate::native::features::construction_records::feature_block_payload_points;
    use crate::native::features::FeatureBlockPayloadNamedRecord;
    use crate::native::features::FeaturePayloadScalar;

    let operation_label = "operation".to_string();
    let construction_payload = "payload".to_string();
    let name = FeaturePayloadName {
        id: "name".to_string(),
        operation_label: operation_label.clone(),
        construction_payload: construction_payload.clone(),
        ordinal: 0,
        frame: crate::om::name_field::NameField::new(
            "Point7".to_string(),
            10,
            Some(crate::om::compact::CompactIndexTarget {
                atom: crate::om::compact::CompactIndexAtom::from_wire(131, &[0x80, 0x83]).unwrap(),
                target: Some(101),
            }),
        )
        .unwrap(),
        source_offset: 100,
    };
    let scalar = |id: &str, ordinal: u32, value: f64| {
        let mut raw_value = value.to_be_bytes();
        raw_value[0] -= 0x10;
        FeaturePayloadScalar {
            id: id.to_string(),
            operation_label: operation_label.clone(),
            payload: crate::native::features::FeatureScalarPayload::Construction {
                construction_payload: construction_payload.clone(),
            },
            ordinal,
            field_code: 100,
            scalar: crate::om::scalar::ShiftedBinary64::try_from(raw_value).unwrap(),
            payload_offset: 20 + u64::from(ordinal) * 13,
            source_offset: 110 + u64::from(ordinal) * 13,
        }
    };
    let scalars = [scalar("first", 0, 1.25), scalar("second", 1, -2.5)];
    let record = FeatureBlockPayloadNamedRecord {
        id: "record".to_string(),
        operation_label,
        construction_payload,
        name_field: name.id.clone(),
        scalar_fields: scalars.iter().map(|scalar| scalar.id.clone()).collect(),
        payload_start_offset: 10,
        payload_end_offset: 50,
    };

    let points = crate::test_support::with_decode_context(|ctx| {
        feature_block_payload_points(
            ctx,
            std::slice::from_ref(&record),
            std::slice::from_ref(&name),
            &scalars,
        )
    })
    .expect("admitted block points");
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].name, "Point7");
    assert_eq!(points[0].coordinates, [1.25, -2.5]);

    let mut duplicate = points[0].clone();
    duplicate.id = "point-2".to_string();
    let groups = crate::test_support::with_decode_context(|ctx| {
        feature_block_payload_point_groups(ctx, &[points[0].clone(), duplicate])
    })
    .expect("admitted block point group");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].points.len(), 2);
    assert_eq!(groups[0].coordinates, [1.25, -2.5]);

    let mut conflicting = points[0].clone();
    conflicting.id = "conflicting".to_string();
    conflicting.coordinates = cadmpeg_ir::units::FiniteVector::new([
        conflicting.coordinates[0],
        f64::from_bits((-2.5_f64).to_bits() + 1),
    ])
    .expect("finite coordinates");
    assert!(crate::test_support::with_decode_context(|ctx| {
        feature_block_payload_point_groups(ctx, &[points[0].clone(), conflicting])
    })
    .expect("conflicting block point group")
    .is_empty());

    let mut incomplete = record.clone();
    incomplete.scalar_fields.pop();
    assert!(
        crate::test_support::with_decode_context(|ctx| feature_block_payload_points(
            ctx,
            &[incomplete],
            std::slice::from_ref(&name),
            &scalars,
        ))
        .expect("incomplete block point")
        .is_empty()
    );
    let mut malformed = name;
    malformed.frame = crate::om::name_field::NameField::new(
        "Point0".to_string(),
        10,
        Some(crate::om::compact::CompactIndexTarget {
            atom: crate::om::compact::CompactIndexAtom::from_wire(131, &[0x80, 0x83]).unwrap(),
            target: Some(101),
        }),
    )
    .unwrap();
    assert!(
        crate::test_support::with_decode_context(|ctx| feature_block_payload_points(
            ctx,
            &[record],
            &[malformed],
            &scalars,
        ))
        .expect("malformed block point")
        .is_empty()
    );
}

#[test]
fn operation_history_reverses_source_order_within_each_section() {
    let label = |section: &str, ordinal, value: &str| FeatureOperationLabel {
        id: format!("{section}-{ordinal}"),
        section_link: section.to_string(),
        ordinal,
        value: value.to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: u64::from(ordinal),
    };
    let labels = [
        label("first", 0, "newest-first"),
        label("first", 1, "oldest-first"),
        label("second", 0, "newest-second"),
        label("second", 1, "oldest-second"),
    ];

    let values = crate::test_support::with_decode_context(|ctx| {
        feature_operation_chronological_labels(ctx, &labels)
    })
    .expect("admitted chronological labels")
    .into_iter()
    .map(|label| label.value.as_str())
    .collect::<Vec<_>>();

    assert_eq!(
        values,
        [
            "oldest-first",
            "newest-first",
            "oldest-second",
            "newest-second"
        ]
    );
}

#[test]
fn operation_history_groups_interleaved_sections_before_reversing() {
    let label = |section: &str, ordinal, value: &str| FeatureOperationLabel {
        id: format!("{section}-{ordinal}"),
        section_link: section.to_string(),
        ordinal,
        value: value.to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset: u64::from(ordinal),
    };
    let labels = [
        label("first", 0, "newest-first"),
        label("second", 0, "newest-second"),
        label("first", 1, "oldest-first"),
        label("second", 1, "oldest-second"),
    ];

    let values = crate::test_support::with_decode_context(|ctx| {
        feature_operation_chronological_labels(ctx, &labels)
    })
    .expect("admitted chronological labels")
    .into_iter()
    .map(|label| label.value.as_str())
    .collect::<Vec<_>>();

    assert_eq!(
        values,
        [
            "oldest-first",
            "newest-first",
            "oldest-second",
            "newest-second"
        ]
    );
}

#[test]
fn operation_history_uses_serialized_offsets_for_section_and_member_order() {
    let label = |section: &str, ordinal, value: &str, source_offset| FeatureOperationLabel {
        id: format!("{section}-{ordinal}"),
        section_link: section.to_string(),
        ordinal,
        value: value.to_string(),
        objects: crate::om::header_references::HeaderReferences([None; 4]),
        stable_identity: None,
        source_offset,
    };
    let labels = [
        label("first", 1, "oldest-first", 210),
        label("second", 1, "oldest-second", 110),
        label("first", 0, "newest-first", 200),
        label("second", 0, "newest-second", 100),
    ];

    let values = crate::test_support::with_decode_context(|ctx| {
        feature_operation_chronological_labels(ctx, &labels)
    })
    .expect("admitted chronological labels")
    .into_iter()
    .map(|label| label.value.as_str())
    .collect::<Vec<_>>();

    assert_eq!(
        values,
        [
            "oldest-second",
            "newest-second",
            "oldest-first",
            "newest-first"
        ]
    );
}

#[test]
fn decoded_operation_frames_resolve_unique_offset_store_targets() {
    let input_slots: &'static [u8] = &[1, 0xff, 0xff, 0xff];
    let common_and_terminal = vec![
        0x00, 0x81, 0x5f, 0x80, 0xab, 0x01, 0x03, 0x02, 0x01, 0x02, 0x01, 0x01, 0x01, 0x00, 0x00,
        0x00, 0x29, 0x29, 0x41, 0x00,
    ];
    let store_records = (0..65).map(|_| b"\0".as_slice()).collect::<Vec<_>>();
    let payload = composed_feature_history_payload(
        &[(input_slots, "FSET", common_and_terminal)],
        &store_records,
    );
    let file = prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", payload)]);
    let result = NxCodec
        .decode(&mut Cursor::new(file), &DecodeOptions::default())
        .expect("required invariant");
    let namespace = result
        .ir()
        .native
        .namespace("nx")
        .expect("required invariant");
    let common_frames = namespace
        .arena_as::<FeatureOperationCommonFrame>("feature_operation_common_frames")
        .expect("required invariant");
    assert_eq!(common_frames.len(), 1);
    assert_eq!(common_frames[0].frame.suffix().object_index(), Some(65));
    assert_eq!(
        common_frames[0]
            .frame
            .suffix()
            .target()
            .and_then(Option::as_deref),
        Some("nx:om-data-blocks-0:block#65")
    );
    let terminal_frames = namespace
        .arena_as::<FeatureOperationTerminalFrame>("feature_operation_terminal_frames")
        .expect("required invariant");
    assert_eq!(terminal_frames.len(), 1);
    assert_eq!(terminal_frames[0].frame.suffix().object_index(), Some(65));
    assert_eq!(
        terminal_frames[0]
            .frame
            .suffix()
            .target()
            .and_then(Option::as_deref),
        Some("nx:om-data-blocks-0:block#65")
    );
}
