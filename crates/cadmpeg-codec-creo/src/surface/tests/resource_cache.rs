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

fn named_records_with_limits(
    payload: &[u8],
    collection_items: u64,
    retained_bytes: u64,
) -> Result<Vec<super::super::SurfacePrototypeRecord>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_items;
    policy.limits.max_retained_bytes = retained_bytes;
    let (ctx, _) =
        DecodeContext::from_root_bytes(payload, &arena, &policy).expect("root input is admitted");
    super::super::named_prototype_records(
        &ctx,
        payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
}

#[test]
fn named_prototype_record_refuses_before_vec_growth() {
    let payload = b"srf_prim_ptr(plane)\0";
    assert_eq!(
        named_records_with_limits(payload, 1, u64::MAX)
            .expect("one record admitted")
            .len(),
        1
    );
    let error =
        named_records_with_limits(payload, 0, u64::MAX).expect_err("record needs one Vec item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo named prototype records"));
}

#[test]
fn named_prototype_parameter_refuses_before_vec_growth() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius2\0\x2e\x05\x33\xf1\xf7\x0e\xe3";
    assert_eq!(
        named_records_with_limits(payload, u64::MAX, u64::MAX).expect("one parameter admitted")[0]
            .parameters
            .len(),
        1
    );
    let error =
        named_records_with_limits(payload, 0, u64::MAX).expect_err("parameter needs one Vec item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo named prototype parameters"));
}

#[test]
fn named_prototype_body_refuses_before_retained_copy() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius2\0\x2e\x05\x33\xf1\xf7\x0e\xe3";
    assert_eq!(
        named_records_with_limits(payload, u64::MAX, u64::MAX).expect("body admitted")[0]
            .parameters[0]
            .body,
        [0x2e, 0x05, 0x33, 0xf1, 0xf7, 0x0e]
    );
    let error =
        named_records_with_limits(payload, u64::MAX, 0).expect_err("body needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo named prototype parameter body"));
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
