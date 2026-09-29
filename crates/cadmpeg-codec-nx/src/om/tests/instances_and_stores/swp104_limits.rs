// SPDX-License-Identifier: Apache-2.0
//! Resource refusals for the SWP104 leading branch.

use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn refusal(configure: impl FnOnce(&mut DecodePolicy)) -> CodecError {
    let raw_scalar = [0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b];
    let mut payload = vec![0x21, 0, 0, 1, 0];
    for _ in 0..4 {
        payload.extend(raw_scalar);
    }
    payload.extend([0x23, 1, 3, 0xf0, 0x31, 0xf0, 0x32, 1, 4]);
    payload.extend([0, 1, 1, 0, 0, 0, 0]);
    payload.extend([0xff, 1, 2, 0xf0, 0x33, 0, 0xaa]);
    
    
    
    crate::test_support::with_decode_context_over(&payload, |policy| { configure(policy); }, |ctx| {

    let record =
        crate::om::operation_record::OperationPayload::new(&payload, 200, "SWP104").unwrap();
    crate::om::swp104_payload_leading_branch(ctx, record).unwrap_err()

})
}

#[test]
fn swp104_leading_branch_refuses_collection_limit() {
    let error = refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems)
    );
}

#[test]
fn swp104_leading_branch_refuses_retained_limit() {
    let error = refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
}

#[test]
fn swp104_leading_branch_refuses_work_limit() {
    let error = refusal(|policy| policy.limits.max_work_units = 0);
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::WorkUnits)
    );
}
