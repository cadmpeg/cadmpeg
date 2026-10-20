// SPDX-License-Identifier: Apache-2.0
//! Counted branch paths are derived work, independent of session recursion.

const PAYLOAD: &[u8] = b"\xa0\x5a\x14\x13\x01\x02\x40\x01\x04\xf1\x1b\xf4\xf1\x1b\xf5\xf1\x1b\xf6\x01\x04\x00\x00\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xf7\x00\x81\x58\x01\x02\x40\x01\x05\xf1\x1b\xf8\xf1\x1b\xf9\xf1\x1b\xfa\xf1\x1b\xfb\x00\x00\x00\x00\x00\xff\x01\x02\xf1\x1b\xfc\x00\x81\x1c\x00\x00\x00\x01\x03\x00\x00\x00\xff\xff\x01";

#[test]
fn counted_surface_branches_do_not_consume_session_recursion_depth() {
    crate::test_support::with_decode_context_over(
        PAYLOAD,
        |policy| policy.limits.max_recursion_depth = 0,
        |ctx| {
            let record =
                crate::om::operation_record::OperationPayload::new(PAYLOAD, 0, "SKIN").unwrap();
            let group = crate::om::surface_branches::surface_feature_payload_branches(record)
                .expect("two complete counted groups");
            assert_eq!(group.into_branches().len(), 2);
            assert_eq!(ctx.policy().limits.max_recursion_depth, 0);
        },
    );
}
