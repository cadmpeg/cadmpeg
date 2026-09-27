// SPDX-License-Identifier: Apache-2.0
//! Resource refusals on the topology merge route.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

#[test]
fn deltas_merge_route_refuses_scoped_limit() {
    let partition = [0xff; 10];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 9;
    let (ctx, _) = DecodeContext::from_root_bytes(&partition, &arena, &policy).unwrap();
    let census = crate::deltas::census::walk(&ctx, &[]).unwrap();
    let error = crate::deltas::merge_full_records_with_census(&ctx, &partition, &[], &census)
        .expect_err("merge scoped refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn deltas_merge_route_refuses_retained_limit() {
    let partition = [0xff; 10];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 9;
    let (ctx, _) = DecodeContext::from_root_bytes(&partition, &arena, &policy).unwrap();
    let census = crate::deltas::census::walk(&ctx, &[]).unwrap();
    let error = crate::deltas::merge_full_records_with_census(&ctx, &partition, &[], &census)
        .expect_err("merge retained refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}
