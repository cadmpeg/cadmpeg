// SPDX-License-Identifier: Apache-2.0

use super::canonical_json_sha256;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn canonical_hash_refuses_at_the_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error: CodecError = canonical_json_sha256(&ctx, &"text", "canonical hash depth fixture")
        .unwrap_err()
        .into();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("hash must preserve resource refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}
