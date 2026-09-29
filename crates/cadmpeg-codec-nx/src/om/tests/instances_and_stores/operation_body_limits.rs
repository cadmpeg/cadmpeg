// SPDX-License-Identifier: Apache-2.0
//! Resource refusals for operation body lanes.

use super::{
    operation_body_11_continuations_for_test, operation_body_members_for_test,
    operation_body_reference_lanes_for_test,
};
use crate::om::OperationBodyReferenceLaneValues;

fn body_continuation_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    
    
    
    crate::test_support::with_decode_context_over(bytes, |policy| { configure(policy); }, |ctx| {

    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, "TRIM BODY").unwrap();
    crate::om::operation_body_11_continuations(ctx, record).unwrap_err()

})
}

#[test]
fn body_continuations_refuse_collection_limit() {
    let error = body_continuation_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn body_continuations_refuse_retained_limit() {
    let error = body_continuation_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn body_continuations_refuse_work_limit() {
    let error = body_continuation_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}

fn body_reference_lane_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let bytes = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    
    
    
    crate::test_support::with_decode_context_over(bytes, |policy| { configure(policy); }, |ctx| {

    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, "OFFSET").unwrap();
    crate::om::operation_body_reference_lanes(ctx, record).unwrap_err()

})
}

#[test]
fn body_reference_lanes_refuse_collection_limit() {
    let error = body_reference_lane_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems)
    );
}

#[test]
fn body_reference_lanes_refuse_retained_limit() {
    let error = body_reference_lane_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes)
    );
}

#[test]
fn body_reference_lanes_refuse_work_limit() {
    let error = body_reference_lane_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits)
    );
}
#[test]
fn om_operation_body_branch_11_decodes_wrapped_member_lane_atomically() {
    let label = "SEW";
    let bytes = b"\x01\x02\x10\x42\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x03\x2e\x7f\x00\x2e\x80\x01\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, label).unwrap();
    let members = operation_body_members_for_test(record);
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].members.len(), 2);
    assert_eq!(members[0].body_reference_ordinal, 0);
    assert_eq!(members[0].body_object_index, 66);
    assert_eq!(members[0].members[0].atom.value(), 127);
    assert_eq!(members[0].members[0].atom.raw(), [0x7f]);
    assert_eq!(members[0].members[0].offset, 122);
    assert_eq!(members[0].members[1].atom.value(), 1);
    assert_eq!(members[0].members[1].atom.raw(), [0x80, 0x01]);

    let truncated = &bytes[..bytes.len() - 1];
    assert!(operation_body_members_for_test(
        crate::om::operation_record::OperationBodyInput::new(
            truncated,
            record.offset(),
            record.payload_start(),
            record.name()
        )
        .unwrap()
    )
    .is_empty());
}

#[test]
fn om_trim_body_branch_11_decodes_terminal_continuation_atomically() {
    let label = "TRIM BODY";
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, label).unwrap();
    let continuations = operation_body_11_continuations_for_test(record);
    assert_eq!(continuations.len(), 1);
    let continuation = &continuations[0];
    assert_eq!(continuation.body_reference_ordinal, 0);
    assert_eq!(continuation.body_object_index, 114);
    assert_eq!(continuation.continuation.atom.value(), 67);
    assert_eq!(continuation.continuation.atom.raw(), [0x80, 0x43]);
    assert_eq!(continuation.continuation.offset, 126);
    assert_eq!(continuation.terminal.token.value(), 114);
    assert_eq!(continuation.terminal.token.raw(), [0x72]);
    assert_eq!(continuation.terminal.offset, 131);

    let mut distinct_terminal = bytes.to_vec();
    distinct_terminal[31] = 0x71;
    assert_eq!(
        operation_body_11_continuations_for_test(
            crate::om::operation_record::OperationBodyInput::new(
                &distinct_terminal,
                record.offset(),
                record.payload_start(),
                record.name()
            )
            .unwrap()
        )[0]
        .terminal
        .token
        .value(),
        113
    );

    let truncated = &bytes[..bytes.len() - 1];
    assert!(operation_body_11_continuations_for_test(
        crate::om::operation_record::OperationBodyInput::new(
            truncated,
            record.offset(),
            record.payload_start(),
            record.name()
        )
        .unwrap()
    )
    .is_empty());
}

#[test]
fn om_operation_body_decodes_homogeneous_unwrapped_reference_lanes() {
    let label = "OFFSET";
    let compact = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let record =
        crate::om::operation_record::OperationBodyInput::new(compact, 100, 0, label).unwrap();
    let lanes = operation_body_reference_lanes_for_test(record);
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes[0].body_object_index, 110);
    let OperationBodyReferenceLaneValues::CompactIndex(values) = &lanes[0].values else {
        panic!("expected CompactIndex lane")
    };
    assert_eq!(
        values
            .iter()
            .map(|value| (value.atom.value(), value.offset))
            .collect::<Vec<_>>(),
        [(13, 111), (105, 113)]
    );
    assert_eq!(
        values
            .iter()
            .map(|value| value.atom.raw())
            .collect::<Vec<_>>(),
        [b"\x80\x0d".as_slice(), b"\x69".as_slice()]
    );

    let objects =
        b"\x01\x02\x10\x70\xff\x1c\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let object_record = crate::om::operation_record::OperationBodyInput::new(
        objects,
        record.offset(),
        record.payload_start(),
        record.name(),
    )
    .unwrap();
    let lanes = operation_body_reference_lanes_for_test(object_record);
    let OperationBodyReferenceLaneValues::PayloadObjectIndex(values) = &lanes[0].values else {
        panic!("expected PayloadObjectIndex lane")
    };
    assert_eq!(
        values
            .iter()
            .map(|value| value.token.value())
            .collect::<Vec<_>>(),
        [670, 68]
    );
    assert_eq!(
        values
            .iter()
            .map(|value| value.token.raw())
            .collect::<Vec<_>>(),
        [b"\xf1\x02\x9e".as_slice(), b"\xf0\x44".as_slice()]
    );

    let truncated = &objects[..objects.len() - 1];
    assert!(operation_body_reference_lanes_for_test(
        crate::om::operation_record::OperationBodyInput::new(
            truncated,
            object_record.offset(),
            object_record.payload_start(),
            object_record.name()
        )
        .unwrap()
    )
    .is_empty());

    let branch_11 =
        b"\x01\x02\x10\x70\xff\x11\x00\x00\x00\x01\x03\xf1\x02\x9e\xf0\x44\x00\x00\x0b\x00";
    let lanes = operation_body_reference_lanes_for_test(
        crate::om::operation_record::OperationBodyInput::new(
            branch_11,
            record.offset(),
            record.payload_start(),
            record.name(),
        )
        .unwrap(),
    );
    assert_eq!(lanes.len(), 1);
    assert_eq!(
        lanes[0].branch,
        crate::om::discriminators::OperationBodyReferenceBranch::Form11
    );
    let OperationBodyReferenceLaneValues::PayloadObjectIndex(values) = &lanes[0].values else {
        panic!("expected payload lane")
    };
    assert_eq!(
        values
            .iter()
            .map(|value| value.token.value())
            .collect::<Vec<_>>(),
        [670, 68]
    );
}
