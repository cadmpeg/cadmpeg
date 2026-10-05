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

fn assert_scalar_cache_refusal(error: &CodecError) {
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
        named_records_with_limits(payload, 2, u64::MAX)
            .expect("one record admitted")
            .len(),
        1
    );
    let error =
        named_records_with_limits(payload, 1, u64::MAX).expect_err("record needs one Vec item");
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
        named_records_with_limits(payload, 4, u64::MAX).expect_err("parameter needs one Vec item");
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
    let error = named_records_with_limits(
        payload,
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo named prototype parameter body"),
            |cap| named_records_with_limits(payload, u64::MAX, cap),
        ),
    )
    .expect_err("body needs retained bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo named prototype parameter body"));
}

#[test]
fn named_prototype_scalar_cache_refuses_before_hashset_growth() {
    assert!(
        run_with_collection_limit(6, |ctx| super::super::named_prototype_records(
            ctx,
            SCALAR_IMAGE,
            &mut crate::lane_refusal::LaneRefusals::new()
        ))
        .expect("service collection budget admits scalar cache")
        .is_empty()
    );
    assert_scalar_cache_refusal(
        &run_with_collection_limit(0, |ctx| {
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
        &run_with_collection_limit(0, |ctx| {
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
        &run_with_collection_limit(0, |ctx| {
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
        &run_with_collection_limit(0, |ctx| {
            super::super::plane_local_systems_for_rows(ctx, SCALAR_IMAGE, &[])
        })
        .expect_err("scalar image needs a HashSet item"),
    );
}

#[test]
fn prototype_family_utf8_refuses_before_unknown_family() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::super::named_prototype_frames(ctx, b"srf_prim_ptr(\xff)\0"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn prototype_count_utf8_refuses_before_unknown_family() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::super::prototype_count(ctx, b"srf_prim_ptr(\xff)\0"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn prototype_parameter_utf8_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::super::named_prototype_frames(ctx, b"srf_prim_ptr(plane)\0\xe0\x01radius\0"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn prototype_fields_retain_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo named prototype field retain",
        |ctx| super::super::named_prototype_frames(ctx, b"srf_prim_ptr(plane)\0\xe0\x01radius\0"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo named prototype field retain")
    );
}

#[test]
fn placed_outline_retain_refuses_work() {
    let records = [crate::surface::PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: crate::surface::PlaneEnvelope::Standard {
            bounds_2d: [[Some(0.0), Some(1.0)], [Some(0.0), Some(1.0)]],
            corners_3d: [
                [Some(3.0), Some(-2.0), Some(4.0)],
                [Some(3.0), Some(5.0), Some(9.0)],
            ],
        },
        corner_coordinate_equal: [Some(true), Some(false), Some(false)],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    }];
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo placed outline retain",
        |ctx| crate::surface::placed_outline_planes(ctx, &records, &[]),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo placed outline retain")
    );
}

#[test]
fn unique_surface_row_retain_refuses_work() {
    let payload = b"srf_array\0geom_id\0\x01geom_type\0\x22feat_id\0\x01next_geom_ptr\0\x01orient\0\x01boundary_type\0\x00";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo unique surface row retain",
        |ctx| {
            crate::surface::rows_with_boundaries(
                ctx,
                payload,
                &[crate::surface::BoundaryType::Code00],
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo unique surface row retain")
    );
}

#[test]
fn prototype_surface_row_retain_refuses_work() {
    let payload = b"srf_array\0geom_id\0\x01geom_type\0\x22feat_id\0\x01next_geom_ptr\0\x01orient\0\x01boundary_type\0\x00";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo prototype surface row retain",
        |ctx| {
            crate::surface::rows_with_boundaries(
                ctx,
                payload,
                &[crate::surface::BoundaryType::Code00],
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo prototype surface row retain")
    );
}

#[test]
fn boundary_surface_row_retain_refuses_work() {
    let payload = b"srf_array\0geom_id\0\x01geom_type\0\x22feat_id\0\x01next_geom_ptr\0\x01orient\0\x01boundary_type\0\x00";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo boundary surface row retain",
        |ctx| {
            crate::surface::rows_with_boundaries(
                ctx,
                payload,
                &[crate::surface::BoundaryType::Code00],
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo boundary surface row retain")
    );
}
