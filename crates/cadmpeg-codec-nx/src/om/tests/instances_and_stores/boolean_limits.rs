// SPDX-License-Identifier: Apache-2.0
//! Resource refusals for Boolean operation references.

fn boolean_operation_refusal(configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy)) -> cadmpeg_core::CodecError {
    let bytes = b"\x80\xcd\x01\x04\x01\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\xff\xff\xff\xff\xff\xff\x03\x0aSUBTRACT\0\x31\x00\x00\x01\x00\x14\x2f\xa4\x7a\xe1\x47\xae\x14\x7b\x03\x00\x00\xe0\x7f\xff\xff\xff\x01\x01\x01\x02\x90\x19\x5e\x00\x01\x05\x90\x19\x5f\x90\x19\x44\x90\x19\x43\x90\x19\x60\x00";
    let labels = super::operation_labels(bytes, 100);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy).unwrap();
    crate::om::boolean_operations_with_labels(&ctx, bytes, 100, &labels).unwrap_err()
}

#[test]
fn boolean_operations_refuse_collection_limit() {
    let error = boolean_operation_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn boolean_operations_refuse_retained_limit() {
    let error = boolean_operation_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn boolean_operations_refuse_work_limit() {
    let error = boolean_operation_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
