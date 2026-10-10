// SPDX-License-Identifier: Apache-2.0

use super::ScalarCache;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const IMAGE: [u8; 8] = [0x46, 0x08, 1, 2, 3, 4, 5, 6];

fn constructor_peak() -> u64 {
    crate::test_support::allocation_limit_at(ResourceDimension::MaterializedBytes, None, |cap| {
        with_limits(cap, |ctx| ScalarCache::from_section_checked(&ctx, &IMAGE).map(|cache| {
            assert_eq!(cache.entries.len(), 1);
            assert_eq!(cache.paired_byte_1(&IMAGE[2..]), Some(0x08));
        }))
    })
}

fn with_limits<T>(materialized: u64, run: impl FnOnce(DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = materialized;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&IMAGE, &arena, &policy).expect("root");
    run(ctx)
}

#[test]
fn checked_scalar_cache_constructor_has_exact_first_image_peak() {
    let peak = constructor_peak();
    crate::test_support::assert_refusal_order(
        ResourceDimension::MaterializedBytes,
        &["creo scalar cache unique images", "creo scalar cache paired tails", "creo scalar cache entries"],
        |cap| with_limits(cap, |ctx| {
            let result = match ScalarCache::from_section_checked(&ctx, &IMAGE) {
                Ok(cache) => {
                    assert_eq!(cap, peak);
                    assert_eq!(cache.entries.len(), 1);
                    assert_eq!(cache.paired_byte_1(&IMAGE[2..]), Some(0x08));
                    drop(cache);
                    let all = ctx.reserve_scoped(cap, "cache backing released")
                        .expect("all backing refunded");
                    drop(all);
                    Ok(())
                }
                Err(CodecError::ResourceLimit(original)) => {
                    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
                    assert_eq!(original.limit, cap);
                    assert!(matches!(ScalarCache::from_section_checked(&ctx, &[]),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                    assert!(matches!(ScalarCache::from_section_checked(&ctx, &IMAGE),
                        Err(CodecError::ResourceLimit(actual)) if actual == original));
                    Err(CodecError::ResourceLimit(original))
                }
                Err(error) => panic!("constructor error: {error}"),
            };
            if let Err(CodecError::ResourceLimit(original)) = &result {
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == *original));
            } else {
                ctx.finish_session().expect("active session");
            }
            result
        }),
    );

}

#[test]
fn checked_scalar_cache_keeps_only_returned_backing_live() {
    let peak = constructor_peak();
    let error = crate::test_support::last_refusal_at(
        &IMAGE, ResourceDimension::MaterializedBytes, "live cache backing",
        |ctx| {
            let cache = ScalarCache::from_section_checked(ctx, &IMAGE)?;
            assert_eq!(cache.entries.len(), 1);
            assert_eq!(cache.paired_byte_1(&IMAGE[2..]), Some(0x08));
            let probe = ctx.reserve_scoped(peak, "live cache backing");
            drop(cache);
            probe.map(drop)
        },
    );
    let CodecError::ResourceLimit(original) = error else { panic!("live cache refusal"); };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.operation, "live cache backing");
    let live = original.used;
    assert!(live > 0, "returned cache buffers retain their reservation");
    assert!(live < peak, "construction-only image set has dropped");
    with_limits(peak, |ctx| {
        let cache = ScalarCache::from_section_checked(&ctx, &IMAGE).expect("cache");
        let rest = ctx.reserve_scoped(peak - live, "remaining cache allowance")
            .expect("image scratch refunded");
        let CodecError::ResourceLimit(original) = ctx.reserve_scoped(1, "live cache backing")
            .expect_err("both returned buffers remain admitted") else { panic!("resource refusal"); };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!((original.used, original.additional, original.limit), (peak, 1, peak));
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(cache.paired_byte_1(&IMAGE[2..]), Some(0x08));
        drop(cache);
        drop(rest);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    });

}

#[test]
fn checked_scalar_cache_drop_preserves_actual_parent_storage() {
    const PARENT_BYTES: u64 = 37;
    let peak = constructor_peak();
    with_limits(PARENT_BYTES + peak, |ctx| {
        let mut parent = ctx.reserve_scoped(0, "actual cache parent").expect("parent");
        let parent_value = parent.with_storage(|| ctx.copy_retained(&[0; 37], "parent backing"))
            .expect("actual parent value");
        let cache = parent.with_storage(|| ScalarCache::from_section_checked(&ctx, &IMAGE)).expect("child cache");
        assert_eq!(cache.entries.len(), 1);
        assert_eq!(parent_value.len(), 37);
        drop(cache);
        let rest = ctx.reserve_scoped(peak, "cache backing refunded under parent").expect("child refunded independently");
        drop(rest);
        drop(parent_value);
        drop(parent);
        let all = ctx.reserve_scoped(PARENT_BYTES + peak, "parent backing refunded").expect("parent refunded separately");
        drop(all);
        ctx.finish_session().expect("active zero-retained session");
    });
}

#[test]
fn empty_checked_scalar_cache_is_free_and_keeps_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let cache = ScalarCache::from_section_checked(&ctx, &[]).expect("empty cache is free");
    assert!(cache.entries.is_empty());
    assert!(cache.paired_byte_1_by_tail.is_empty());
    drop(cache);
    let original = ctx.charge_work_limit(1, "before empty checked cache").expect_err("zero work");
    for _ in 0..2 {
        assert!(matches!(ScalarCache::from_section_checked(&ctx, &[]),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}
