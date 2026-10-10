// SPDX-License-Identifier: Apache-2.0
use super::super::compose_feature_id;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn feature_identity_actual_value_refuses_before_retained_transfer() {
    let bytes = u64::try_from("creo:model:feature#40".len()).expect("fixed identity bytes");
    crate::test_support::assert_refusal_order(
        ResourceDimension::RetainedBytes,
        &["creo model feature identity"],
        |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_retained_bytes = cap;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let parts = compose_feature_id(&ctx, 40).expect("scoped identity");
        let storage = parts.1;
        let id = parts.0;
        assert_eq!(id.as_str(), "creo:model:feature#40");
        let result = storage.commit_value(id);
        if ctx.resource_refusal().is_none() {
            assert_eq!(result.expect("retained identity admitted").as_str(), "creo:model:feature#40");
            return Ok(());
        }
        let original = ctx.resource_refusal().expect("retained transfer refusal");
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!((original.dimension, original.limit, original.used, original.additional, original.operation),
            (ResourceDimension::RetainedBytes, cap, 0, bytes, "creo model feature identity"));
        assert!(matches!(compose_feature_id(&ctx, 40),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        Err(original.into())
    });
}

#[test]
fn feature_identity_actual_value_preserves_native_identity_and_releases_scratch() {
    let bytes = u64::try_from("creo:model:feature#40".len()).expect("fixed identity bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_retained_bytes = bytes;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let parts = compose_feature_id(&ctx, 40).expect("scoped identity");
    let storage = parts.1;
    let id = parts.0;
    let id = storage.commit_value(id).expect("exact retained bytes");
    assert_eq!(id.as_str(), "creo:model:feature#40");
    let released = ctx.reserve_scoped(bytes, "after owned feature identity transfer").expect("scratch released while actual ID survives");
    drop(released);
    let original = ctx.charge_retained_limit(1, "after owned feature identity transfer").expect_err("exact retained cap");
    assert_eq!((original.dimension, original.used, original.additional),
        (ResourceDimension::RetainedBytes, bytes, 1));
    assert!(matches!(compose_feature_id(&ctx, 40),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(id.as_str(), "creo:model:feature#40");
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(id);
}

