// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SCALAR_IMAGE: &[u8] = &[0x46, 0, 0, 0, 0, 0, 0, 0];

fn run_with_collection_limit<T>(
    limit: u64,
    run: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(SCALAR_IMAGE, &arena, &policy)
        .expect("root scalar image is admitted");
    run(&ctx)
}

fn assert_scalar_cache_refusal(error: CodecError) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo scalar cache unique images"));
}

#[test]
fn named_prototype_scalar_cache_refuses_before_hashset_growth() {
    assert!(
        run_with_collection_limit(3, |ctx| super::super::named_prototype_records(
            ctx,
            SCALAR_IMAGE,
            &mut crate::lane_refusal::LaneRefusals::new()
        ))
        .expect("service collection budget admits scalar cache")
        .is_empty()
    );
    assert_scalar_cache_refusal(
        run_with_collection_limit(0, |ctx| {
            super::super::named_prototype_records(
                ctx,
                SCALAR_IMAGE,
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
        .expect_err("scalar image needs a HashSet item"),
    );
}

#[test]
fn positional_parameter_scalar_cache_refuses_before_hashset_growth() {
    assert!(run_with_collection_limit(3, |ctx| {
        super::super::parameter_records_for_rows(ctx, SCALAR_IMAGE, &[])
    })
    .expect("service collection budget admits scalar cache")
    .is_empty());
    assert_scalar_cache_refusal(
        run_with_collection_limit(0, |ctx| {
            super::super::parameter_records_for_rows(ctx, SCALAR_IMAGE, &[])
        })
        .expect_err("scalar image needs a HashSet item"),
    );
}

#[test]
fn contour_scalar_cache_refuses_before_hashset_growth() {
    assert!(run_with_collection_limit(3, |ctx| {
        super::super::contour_records_for_rows(ctx, SCALAR_IMAGE, &[])
    })
    .expect("service collection budget admits scalar cache")
    .is_empty());
    assert_scalar_cache_refusal(
        run_with_collection_limit(0, |ctx| {
            super::super::contour_records_for_rows(ctx, SCALAR_IMAGE, &[])
        })
        .expect_err("scalar image needs a HashSet item"),
    );
}

#[test]
fn plane_local_system_scalar_cache_refuses_before_hashset_growth() {
    assert!(run_with_collection_limit(6, |ctx| {
        super::super::plane_local_systems_for_rows(ctx, SCALAR_IMAGE, &[])
    })
    .expect("service collection budget admits both scalar caches")
    .is_empty());
    assert_scalar_cache_refusal(
        run_with_collection_limit(0, |ctx| {
            super::super::plane_local_systems_for_rows(ctx, SCALAR_IMAGE, &[])
        })
        .expect_err("scalar image needs a HashSet item"),
    );
}
