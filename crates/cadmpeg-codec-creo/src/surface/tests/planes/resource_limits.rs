// SPDX-License-Identifier: Apache-2.0

use crate::scalar;
use crate::surface::SurfacePrototypeFamily;

fn with_surface_limits<T>(
    input: &[u8],
    collection_limit: u64,
    retained_limit: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<T, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("input fits root byte limit");
    run(&ctx)
}

fn assert_surface_limit(
    error: cadmpeg_core::CodecError,
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn surface_parameter_refuses_header_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let payload = [7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe3];
    let rows = [crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }];
    let service = with_surface_limits(&payload, u64::MAX, u64::MAX, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })
    .expect("one parameter record fits service limits");
    assert_eq!(service.len(), 1);
    let error = with_surface_limits(&payload, 0, u64::MAX, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })
    .err()
    .expect("one header exceeds zero collection items");
    assert_surface_limit(error, ResourceDimension::CollectionItems, "creo surface parameter headers");
}

#[test]
fn surface_parameter_refuses_body_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    let payload = [7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe3];
    let rows = [crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }];
    let error = with_surface_limits(&payload, u64::MAX, 0, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })
    .err()
    .expect("one body byte exceeds zero retained bytes");
    assert_surface_limit(error, ResourceDimension::RetainedBytes, "creo surface parameter body");
}

#[test]
fn surface_parameter_refuses_record_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let payload = [7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe3];
    let rows = [crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }];
    let error = with_surface_limits(&payload, 4, u64::MAX, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })
    .err()
    .expect("the record follows four earlier item admissions");
    assert_surface_limit(error, ResourceDimension::CollectionItems, "creo surface parameter records");
}

#[test]
fn surface_scalar_refuses_token_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0xe4];
    let service = with_surface_limits(&body, u64::MAX, u64::MAX, |ctx| {
        crate::surface::scalar_tokens(ctx, crate::surface::SurfaceKind::Plane, &body, &scalar::ScalarCache::default())
    })
    .expect("one token fits service limits");
    assert_eq!(service.len(), 1);
    let error = with_surface_limits(&body, 0, u64::MAX, |ctx| {
        crate::surface::scalar_tokens(ctx, crate::surface::SurfaceKind::Plane, &body, &scalar::ScalarCache::default())
    })
    .err()
    .expect("one token exceeds zero collection items");
    assert_surface_limit(error, ResourceDimension::CollectionItems, "creo surface scalar token items");
}

#[test]
fn surface_scalar_refuses_token_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0xe4];
    let error = with_surface_limits(&body, u64::MAX, 0, |ctx| {
        crate::surface::scalar_tokens(ctx, crate::surface::SurfaceKind::Plane, &body, &scalar::ScalarCache::default())
    })
    .err()
    .expect("one token byte exceeds zero retained bytes");
    assert_surface_limit(error, ResourceDimension::RetainedBytes, "creo surface scalar token bytes");
}

#[test]
fn torus_scalar_refuses_outline_marker_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x01, 0x12, 0x50, 0x50];
    let error = with_surface_limits(&body, 0, u64::MAX, |ctx| {
        crate::surface::scalar_tokens(ctx, crate::surface::SurfaceKind::TorusOrSphere, &body, &scalar::ScalarCache::default())
    })
    .err()
    .expect("one marker exceeds zero collection items");
    assert_surface_limit(error, ResourceDimension::CollectionItems, "creo torus outline marker items");
}

fn plane_corner_limit_error(collection_limit: u64, retained_limit: u64) -> cadmpeg_core::CodecError {
    let body = [
        0x18, 0x18, 0x6d, 0xeb, 0x81, 0x84, 0xcc, 0xcc, 0xd0, 0x00, 0x0c, 0x9a, 0xd5, 0xd6, 0x25,
        0xa6, 0xec, 0x06, 0x18, 0x46, 0x1a, 0xdf, 0x09, 0x9b, 0x3c, 0x32, 0xed, 0x2f, 0x20, 0x00,
        0xd5, 0xd6, 0x25, 0xa6, 0xec, 0x06, 0x18, 0x46, 0x18, 0x81, 0x99, 0x6a, 0xa2, 0x99, 0x53,
        0x2e, 0x20, 0x33, 0xf7, 0x0c,
    ];
    let service = with_surface_limits(&body, u64::MAX, u64::MAX, |ctx| {
        crate::surface::first_coordinate_plane_corner_tokens(ctx, &body, &scalar::ScalarCache::default())
    })
    .expect("corner parser fits service limits");
    assert_eq!(service.expect("unique corner frame").len(), 6);
    with_surface_limits(&body, collection_limit, retained_limit, |ctx| {
        crate::surface::first_coordinate_plane_corner_tokens(ctx, &body, &scalar::ScalarCache::default())
    })
    .err()
    .expect("corner frame exceeds requested limit")
}

#[test]
fn plane_corner_refuses_token_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        plane_corner_limit_error(5, u64::MAX),
        ResourceDimension::CollectionItems,
        "creo plane corner token items",
    );
}

#[test]
fn plane_corner_refuses_token_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        plane_corner_limit_error(u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo plane corner token bytes",
    );
}

#[test]
fn surface_opaque_refuses_span_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x01];
    let error = with_surface_limits(&body, 0, u64::MAX, |ctx| {
        crate::surface::opaque_spans(ctx, &body, &[])
    })
    .err()
    .expect("one span exceeds zero collection items");
    assert_surface_limit(error, ResourceDimension::CollectionItems, "creo surface opaque span items");
}

#[test]
fn surface_opaque_refuses_span_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x01];
    let error = with_surface_limits(&body, u64::MAX, 0, |ctx| {
        crate::surface::opaque_spans(ctx, &body, &[])
    })
    .err()
    .expect("one span byte exceeds zero retained bytes");
    assert_surface_limit(error, ResourceDimension::RetainedBytes, "creo surface opaque span bytes");
}

fn scalar_frame_limit_error(collection_limit: u64, retained_limit: u64) -> cadmpeg_core::CodecError {
    let tokens = [crate::surface::SurfaceParameterScalar {
        value: Some(1.0),
        raw: vec![0xe4],
        offset: 0,
    }];
    with_surface_limits(&[0xe4], collection_limit, retained_limit, |ctx| {
        crate::surface::scalar_frames(ctx, &tokens)
    })
    .err()
    .expect("one scalar frame exceeds requested limit")
}

#[test]
fn surface_scalar_frame_refuses_slot_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        scalar_frame_limit_error(0, u64::MAX),
        ResourceDimension::CollectionItems,
        "creo surface scalar frame slots",
    );
}

#[test]
fn surface_scalar_frame_refuses_slot_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        scalar_frame_limit_error(u64::MAX, 0),
        ResourceDimension::RetainedBytes,
        "creo surface scalar frame bytes",
    );
}

#[test]
fn surface_scalar_frame_refuses_frame_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        scalar_frame_limit_error(1, u64::MAX),
        ResourceDimension::CollectionItems,
        "creo surface scalar frame items",
    );
}

fn plane_envelope_limit_error(
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = [
        7, 0x22, 4, 0x01, 0, 0,
        0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3,
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("plane envelope fits root input limit");
    crate::surface::plane_envelopes(&ctx, &payload)
        .expect_err("one envelope exceeds its limit")
}

#[test]
fn plane_envelope_refuses_scalar_token_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_envelope_limit_error(0, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane envelope scalar token items"));
}

#[test]
fn plane_envelope_refuses_scalar_token_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_envelope_limit_error(u64::MAX, 0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo plane envelope scalar token bytes"));
}

#[test]
fn plane_envelope_refuses_body_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_envelope_limit_error(u64::MAX, 10);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo plane envelope body"));
}

#[test]
fn plane_envelope_refuses_output_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_envelope_limit_error(10, u64::MAX);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane envelopes"));
}

#[test]
fn named_plane_outline_refuses_envelope_output_vector() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let payload = b"srf_array\0\xf8\x01\xe0\x01geom_id\0\x07\xe0\x01geom_type\0\x22\xe0\x01feat_id\0\x04\xe0\x01orient\0\x01\xe0\x01boundary_type\0\x00\xe0\x01next_geom_ptr\0\x00\xe0\x02outline\0\xf9\x02\x03\xe4\x18\xe4\xe4\xe4\x18\xe0\x00srf_prim_ptr(plane)\0\xe3";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
        .expect("named outline fits root input limit");
    let error = crate::surface::plane_envelopes(&ctx, payload)
        .expect_err("six tokens leave no envelope output slot");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane envelopes"));
}

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
