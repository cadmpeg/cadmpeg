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

#[test]
fn deltas_merge_route_refuses_collection_limit() {
    let partition = [0xff; 10];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&partition, &arena, &policy).unwrap();
    let census = crate::deltas::census::walk(&ctx, &[]).unwrap();
    let error = crate::deltas::merge_full_records_with_census(&ctx, &partition, &[], &census)
        .expect_err("merge collection refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn deltas_merge_route_refuses_work_limit() {
    let partition = [0xff; 10];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&partition, &arena, &policy).unwrap();
    let census = crate::deltas::census::walk(&ctx, &[]).unwrap();
    let error = crate::deltas::merge_full_records_with_census(&ctx, &partition, &[], &census)
        .expect_err("merge work refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn deltas_semantic_residual_route_refuses_retained_limit() {
    let bytes = [0xff; 10];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 9;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let census = crate::deltas::census::walk(&ctx, &[]).unwrap();
    let error = crate::deltas::semantic_residual_with_census(&ctx, &bytes, &census)
        .expect_err("semantic residual retained refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes));
}

#[test]
fn deltas_semantic_residual_route_refuses_collection_limit() {
    let bytes = [0xff; 10];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let census = crate::deltas::census::walk(&ctx, &[]).unwrap();
    let error = crate::deltas::semantic_residual_with_census(&ctx, &bytes, &census)
        .expect_err("semantic residual collection refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
}

#[test]
fn deltas_semantic_residual_route_refuses_scoped_limit() {
    let bytes = super::deltas_body_revision(1);
    let census = crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes)).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = crate::deltas::semantic_residual_with_census(&ctx, &bytes, &census)
        .expect_err("semantic residual scoped refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes));
}

#[test]
fn deltas_semantic_residual_route_refuses_work_limit() {
    let bytes = super::deltas_body_revision(1);
    let census = crate::test_support::with_decode_context(|ctx| crate::deltas::census::walk(ctx, &bytes)).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = crate::deltas::semantic_residual_with_census(&ctx, &bytes, &census)
        .expect_err("semantic residual work refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits));
}
