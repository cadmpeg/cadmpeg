// SPDX-License-Identifier: Apache-2.0

use crate::native::features::offset_store_named_points;

fn named_point_refusal(
    configure: impl FnOnce(&mut cadmpeg_core::decode::DecodePolicy),
) -> cadmpeg_core::CodecError {
    let container = crate::test_support::with_decode_context(|ctx| {
        crate::container::scan_bytes(ctx,
            crate::test_support::test_prt::composed_feature_history_prt())
    }).expect("composed feature-history container");
    let admitted = crate::test_support::with_decode_context(|ctx| {
        offset_store_named_points(ctx, &container)
    }).expect("admitted named point route");
    assert!(!admitted.is_empty());
    assert!(admitted[0].data_blocks.len() >= 2);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    configure(&mut policy);
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root");
    offset_store_named_points(&ctx, &container).expect_err("named point resource limit")
}

#[test]
fn named_point_refuses_collection_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_collection_items = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems));
}

#[test]
fn named_point_refuses_retained_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_retained_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
}

#[test]
fn named_point_refuses_scoped_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_materialized_bytes = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::MaterializedBytes));
}

#[test]
fn named_point_refuses_work_limit() {
    let error = named_point_refusal(|policy| policy.limits.max_work_units = 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}
