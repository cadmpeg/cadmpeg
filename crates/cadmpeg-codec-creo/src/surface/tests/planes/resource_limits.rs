// SPDX-License-Identifier: Apache-2.0

use crate::scalar;
use crate::surface::SurfacePrototypeFamily;

fn plane_local_system_limit_error(
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use crate::surface::{BoundaryType, SurfaceKind, SurfaceRow};

    let payload = [
        7, 0x22, 4, 0x01, 0, 0,
        0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3,
        0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe1, 0xe3,
    ];
    let rows = [SurfaceRow {
        id: 7,
        kind: SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("plane frame fits root input limit");
    crate::surface::plane_local_systems_for_rows(&ctx, &payload, &rows)
        .expect_err("one local system exceeds its limit")
}

#[test]
fn plane_local_system_refuses_retained_body() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_local_system_limit_error(u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo plane local-system body"));
}

#[test]
fn plane_local_system_refuses_row_system_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_local_system_limit_error(0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane row systems"));
}

#[test]
fn plane_local_system_refuses_output_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_local_system_limit_error(1, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane local systems"));
}

#[test]
fn local_system_slots_refuse_before_declared_count_reserve() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let body = [0x10];
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &service)
        .expect("one local-system token fits the input limit");
    let values = crate::surface::sequential_named_local_system_slots(
        &ctx,
        &body,
        1,
        &scalar::ScalarCache::default(),
        &mut crate::surface::ScalarBodyRefusal::default(),
    )
    .expect("service profile admits the declared slot");
    assert_eq!(values, Some(vec![Some(0.0)]));

    let mut limited = service;
    limited.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&body, &arena, &limited).expect("input fits the root limit");
    let error = crate::surface::sequential_named_local_system_slots(
        &ctx,
        &body,
        1,
        &scalar::ScalarCache::default(),
        &mut crate::surface::ScalarBodyRefusal::default(),
    )
    .expect_err("one local-system slot exceeds zero collection items");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo local-system scalar slots"
    ));
}

fn named_surface_limit_error(
    name: &str,
    body: &[u8],
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(body, &arena, &policy)
        .expect("named surface input fits the root limit");
    crate::surface::named_surface_value(
        &ctx,
        &SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline),
        name,
        body,
        &scalar::ScalarCache::default(),
        &"prototype fixture",
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("the selected surface allocation exceeds its limit")
}

#[test]
fn opaque_surface_parameter_refuses_before_byte_copy() {
    let error = named_surface_limit_error("flip", &[0xf1], u64::MAX, 0);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo opaque surface parameter bytes"
    ));
}

#[test]
fn zero_surface_scalar_refuses_before_vector_allocation() {
    let error = named_surface_limit_error("radius", &[0x18], 0, u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo zero surface scalar"
    ));
}

#[test]
fn compact_surface_integers_refuse_before_vector_growth() {
    let error = named_surface_limit_error("dum_array", &[0xf8, 0x02, 0x07, 0x08], 0, u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo compact surface integers"
    ));
}

#[test]
fn contiguous_surface_references_refuse_before_vector_allocation() {
    let error =
        named_surface_limit_error("i_pnts", &[0xf8, 0x03, 0xf7, 0x80, 0x80, 0xfb], 2, u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo contiguous surface references"
    ));
}
