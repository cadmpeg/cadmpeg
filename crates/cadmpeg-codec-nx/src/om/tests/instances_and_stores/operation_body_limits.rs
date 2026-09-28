// SPDX-License-Identifier: Apache-2.0
//! Resource refusals for operation body lanes.

fn body_continuation_refusal(configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy)) -> cadmpeg_core::CodecError {
    let bytes = b"\x01\x02\x10\x72\xff\x11\x00\x50\x40\x00\x00\xb0\x65\x40\x00\x00\x00\x00\x00\x01\x02\x2e\x41\x00\x01\x02\x80\x43\x00\x00\x01\x72\x00\x00";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let record = crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, "TRIM BODY").unwrap();
    crate::om::operation_body_11_continuations(&ctx, record).unwrap_err()
}

#[test]
fn body_continuations_refuse_collection_limit() {
    let error = body_continuation_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn body_continuations_refuse_retained_limit() {
    let error = body_continuation_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn body_continuations_refuse_work_limit() {
    let error = body_continuation_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

fn body_reference_lane_refusal(configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy)) -> cadmpeg_core::CodecError {
    let bytes = b"\x01\x02\x10\x6e\xff\x1c\x00\x00\x00\x01\x03\x80\x0d\x69\x00\x00\x0b\x00";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    let record = crate::om::operation_record::OperationBodyInput::new(bytes, 100, 0, "OFFSET").unwrap();
    crate::om::operation_body_reference_lanes(&ctx, record).unwrap_err()
}

#[test]
fn body_reference_lanes_refuse_collection_limit() {
    let error = body_reference_lane_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn body_reference_lanes_refuse_retained_limit() {
    let error = body_reference_lane_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn body_reference_lanes_refuse_work_limit() {
    let error = body_reference_lane_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
