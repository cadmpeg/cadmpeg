// SPDX-License-Identifier: Apache-2.0
//! Recursion-depth refusal for counted surface branch paths.

const PAYLOAD: &[u8] = b"\xa0\x5a\x14\x13\x01\x02\x40\x01\x04\xf1\x1b\xf4\xf1\x1b\xf5\xf1\x1b\xf6\x01\x04\x00\x00\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xf7\x00\x81\x58\x01\x02\x40\x01\x05\xf1\x1b\xf8\xf1\x1b\xf9\xf1\x1b\xfa\xf1\x1b\xfb\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xfc\x00\x81\x1c\x00\x00\x00\x01\x03\x00\x00\x00\xff\xff\x01";

fn surface_branch_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    crate::test_support::with_decode_context_over(
        PAYLOAD,
        |policy| {
            configure(policy);
        },
        |ctx| {
            let record =
                crate::om::operation_record::OperationPayload::new(PAYLOAD, 0, "SKIN").unwrap();
            crate::om::surface_branches::surface_feature_payload_branches(ctx, record).unwrap_err()
        },
    )
}

#[test]
fn surface_branches_refuse_depth_limit() {
    let error = surface_branch_refusal(|policy| policy.limits.max_recursion_depth = 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == cadmpeg_core::decode::ResourceDimension::RecursionDepth)
    );
}
