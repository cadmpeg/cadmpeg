use super::{catalog_stream, PREFIX};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn catalog_short_prefix_rejects_before_candidate_storage() {
    for entries in [&PREFIX[..1], &PREFIX[..2], &PREFIX[..3]] {
        let bytes = catalog_stream(entries);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("borrowed catalog input");
        assert!(crate::catalog::parse(&ctx, &bytes)
            .expect("incomplete required prefix needs no candidate storage")
            .is_empty());
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn catalog_wrong_first_prefix_rejects_before_later_utf8_or_copy() {
    let mut bytes = catalog_stream(&["wrong", "catalogManager", "catalogLinks", "", "payload"]);
    *bytes.last_mut().expect("later payload") = 0xff;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("borrowed catalog input");
    assert!(crate::catalog::parse(&ctx, &bytes)
        .expect("wrong required prefix needs no candidate storage")
        .is_empty());
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn catalog_prefix_rejection_preserves_original_sticky_refusal() {
    let bytes = catalog_stream(&["wrong", "catalogManager", "catalogLinks", ""]);
    crate::test_support::with_work_limit(0, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original catalog refusal")
        else {
            panic!("original refusal")
        };
        assert!(matches!(crate::catalog::parse(ctx, &bytes),
            Err(CodecError::ResourceLimit(next)) if next == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}

#[test]
fn catalog_candidate_refuses_retention_at_actual_return_boundary() {
    let bytes = catalog_stream(&PREFIX);
    crate::test_support::with_retained_limit(0, |ctx| {
        let Err(CodecError::ResourceLimit(original)) = crate::catalog::parse(ctx, &bytes) else {
            panic!("returned catalog owns entry backing")
        };
        assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(original.operation, "catia_catalog_candidate_storage");
        assert_eq!(ctx.resource_refusal(), Some(original));
    });
}
