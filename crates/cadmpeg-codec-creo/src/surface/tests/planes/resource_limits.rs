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
    let (ctx, _) =
        DecodeContext::from_root_bytes(input, &arena, &policy).expect("input fits root byte limit");
    run(&ctx)
}

fn assert_surface_limit(
    error: &cadmpeg_core::CodecError,
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &'static str,
) {
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation)
    );
}

fn limit_reaching_operation(
    operation: &'static str,
    run: impl Fn(u64) -> cadmpeg_core::CodecError,
) -> u64 {
    crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some(operation), |cap| Err::<(), _>(run(cap)))
}

#[test]
fn named_spline_invalid_scalar_refusal_text_obeys_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = [0xff];
    let parse = |limit| {
        with_surface_limits(&body, u64::MAX, limit, |ctx| {
            crate::surface::named_spline_scalar_slots(
                ctx,
                &SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline),
                "tangts",
                &body,
                1,
                &scalar::ScalarCache::default(),
                &mut crate::surface::ScalarBodyRefusal::default(),
            )
        })
    };
    assert!(parse(u64::MAX).expect("service profile").is_none());
    assert_surface_limit(
        &parse(crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo scalar body refusal text"),
            parse,
        ))
        .expect_err("refusal text exceeds retained limit"),
        ResourceDimension::RetainedBytes,
        "creo scalar body refusal text",
    );
}

#[test]
fn named_local_system_invalid_scalar_refusal_text_obeys_retained_limit() {
    use cadmpeg_core::decode::ResourceDimension;

    let body = [0xff];
    let parse = |limit| {
        with_surface_limits(&body, u64::MAX, limit, |ctx| {
            crate::surface::sequential_named_local_system_slots(
                ctx,
                &body,
                1,
                &scalar::ScalarCache::default(),
                &mut crate::surface::ScalarBodyRefusal::default(),
            )
        })
    };
    assert!(parse(u64::MAX).expect("service profile").is_none());
    assert_surface_limit(
        &parse(crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo scalar body refusal text"),
            parse,
        ))
        .expect_err("refusal text exceeds retained limit"),
        ResourceDimension::RetainedBytes,
        "creo scalar body refusal text",
    );
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
    let error = with_surface_limits(&payload, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo surface parameter headers"), |cap| with_surface_limits(&payload, cap, u64::MAX, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })), u64::MAX, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })
    .expect_err("one header exceeds zero collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo surface parameter headers",
    );
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
    let error = crate::test_support::last_refusal_at(
        &payload,
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        "creo surface parameter body",
        |ctx| crate::surface::parameter_records_for_rows(ctx, &payload, &rows),
    );
    assert_surface_limit(
        &error,
        ResourceDimension::RetainedBytes,
        "creo surface parameter body",
    );
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
    let error = with_surface_limits(&payload, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo surface parameter records"), |cap| with_surface_limits(&payload, cap, u64::MAX, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })), u64::MAX, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows)
    })
    .expect_err("the record follows four earlier item admissions");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo surface parameter records",
    );
}

#[test]
fn surface_scalar_refuses_token_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0xe4];
    let service = with_surface_limits(&body, u64::MAX, u64::MAX, |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::Plane,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("one token fits service limits");
    assert_eq!(service.len(), 1);
    let error = with_surface_limits(&body, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo surface scalar token items"), |cap| with_surface_limits(&body, cap, u64::MAX, |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::Plane,
            &body,
            &scalar::ScalarCache::default(),
        )
    })), u64::MAX, |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::Plane,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect_err("one token exceeds zero collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo surface scalar token items",
    );
}

#[test]
fn surface_scalar_refuses_token_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0xe4];
    let error = with_surface_limits(&body, u64::MAX, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo surface scalar token bytes"), |cap| with_surface_limits(&body, u64::MAX, cap, |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::Plane,
            &body,
            &scalar::ScalarCache::default(),
        )
    })), |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::Plane,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect_err("one token byte exceeds zero retained bytes");
    assert_surface_limit(
        &error,
        ResourceDimension::RetainedBytes,
        "creo surface scalar token bytes",
    );
}

#[test]
fn surface_token_slot_table_refuses_before_counted_reservation() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0xe4, 0x18];
    let run = |limit| {
        with_surface_limits(&body, limit, u64::MAX, |ctx| {
            crate::surface::scalar_slots_with_tokens_and_end(
                ctx,
                &body,
                2,
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert_eq!(
        run(u64::MAX)
            .expect("two slots are admitted")
            .expect("complete table")
            .slots
            .len(),
        2
    );
    let error = run(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo surface scalar token slots"), |cap| run(cap))).expect_err("two slots exceed one collection item");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo surface scalar token slots",
    );
}

#[test]
fn plane_envelope_slot_table_refuses_before_counted_reservation() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x0e, 0x0e, 0x18];
    let run = |limit| {
        with_surface_limits(&body, limit, u64::MAX, |ctx| {
            crate::surface::plane_envelope_scalar_slots_with_tokens_and_end(
                ctx,
                &body,
                3,
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert_eq!(
        run(u64::MAX)
            .expect("three slots are admitted")
            .expect("complete table")
            .slots
            .len(),
        3
    );
    let error = run(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo plane envelope token slots"), |cap| run(cap))).expect_err("three slots exceed two collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo plane envelope token slots",
    );
}

#[test]
fn plane_envelope_final_positive_slot_refuses_before_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [
        0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f, 0x99, 1, 2, 3, 4, 5, 6,
    ];
    let run = |limit| {
        with_surface_limits(&body, limit, u64::MAX, |ctx| {
            crate::surface::complete_plane_envelope_slots_with_final_positive_dict(
                ctx,
                &body,
                9,
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert_eq!(
        run(u64::MAX)
            .expect("ten slots are admitted")
            .expect("complete table")
            .slots
            .len(),
        10
    );
    let error = run(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo plane envelope final token slot"), |cap| run(cap))).expect_err("the final slot exceeds nine collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo plane envelope final token slot",
    );
}

#[test]
fn plane_envelope_close_slot_refuses_before_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [
        0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f, 0x99, 1, 2, 3, 4, 5, 6, 0xe3,
    ];
    let run = |limit| {
        with_surface_limits(&body, limit, u64::MAX, |ctx| {
            crate::surface::plane_envelope_compound_close(
                ctx,
                &body,
                &scalar::ScalarCache::default(),
            )
        })
    };
    assert_eq!(
        run(u64::MAX).expect("ten slots are admitted"),
        Some(body.len() - 1)
    );
    let error = run(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo plane envelope close token slot"), |cap| run(cap))).expect_err("the close slot exceeds nine collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo plane envelope close token slot",
    );
}

#[test]
fn plane_envelope_reader_propagates_slot_refusal() {
    use cadmpeg_core::decode::ResourceDimension;
    let payload = [
        7, 0x22, 4, 0x01, 0, 0, 0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f, 0x99, 1, 2,
        3, 4, 5, 6, 0xe3,
    ];
    let rows = [crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 4,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }];
    let run = |limit| {
        with_surface_limits(&payload, limit, u64::MAX, |ctx| {
            crate::surface::plane_envelopes_for_rows(ctx, &payload, &rows)
        })
    };
    assert_eq!(run(u64::MAX).expect("service admits the envelope").len(), 1);
    let error = run(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo plane envelope token slots"), |cap| run(cap))).expect_err("the first table needs nine slots");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo plane envelope token slots",
    );
}

#[test]
fn named_surface_scalar_sequence_refuses_before_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x0e];
    let run = |limit| {
        with_surface_limits(&body, limit, u64::MAX, |ctx| {
            crate::surface::named_surface_value(
                ctx,
                &SurfacePrototypeFamily::Torus(crate::surface::TorusLabel::Torus),
                "radius1",
                &body,
                &scalar::ScalarCache::default(),
                &"named torus fixture",
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        })
    };
    assert_eq!(
        run(u64::MAX).expect("one scalar is admitted"),
        crate::surface::SurfaceNamedValue::ScalarSequence(vec![0.5])
    );
    let error = run(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo named surface scalar sequence"), |cap| run(cap))).expect_err("one scalar exceeds zero collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo named surface scalar sequence",
    );
}

#[test]
fn normalized_plane_frame_refuses_scoped_bytes_before_copy() {
    use cadmpeg_core::decode::{ResourceDimension};
    let body = [
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x18, 0x2f, 0x05, 0x00, 0x00, 0x0c, 0x98,
    ];
    let error = crate::test_support::last_refusal_at(&body, cadmpeg_core::decode::ResourceDimension::MaterializedBytes, "creo normalized plane frame bytes", |ctx| { crate::surface::complete_plane_local_system(ctx, &body, &scalar::ScalarCache::default()) });
    assert_surface_limit(
        &error,
        ResourceDimension::MaterializedBytes,
        "creo normalized plane frame bytes",
    );
}

#[test]
fn normalized_plane_frame_refuses_collection_before_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x18, 0x2f, 0x05, 0x00, 0x00, 0x0c, 0x98,
    ];
    let run = |limit| {
        with_surface_limits(&body, limit, u64::MAX, |ctx| {
            crate::surface::complete_plane_local_system(ctx, &body, &scalar::ScalarCache::default())
        })
    };
    assert!(run(crate::test_support::allocation_limit_at(ResourceDimension::CollectionItems, None, run))
        .expect("normalized bytes are admitted")
        .is_some());
    let boundary = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems, Some("creo normalized plane frame bytes"), run,
    );
    let error = run(boundary).expect_err("normalized frame exceeds its collection boundary");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo normalized plane frame bytes",
    );
}

#[test]
fn torus_scalar_refuses_outline_marker_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x01, 0x12, 0x50, 0x50];
    let error = with_surface_limits(&body, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo torus outline marker items"), |cap| with_surface_limits(&body, cap, u64::MAX, |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::TorusOrSphere,
            &body,
            &scalar::ScalarCache::default(),
        )
    })), u64::MAX, |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::TorusOrSphere,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect_err("one marker exceeds zero collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo torus outline marker items",
    );
}

fn plane_corner_limit_error(collection_limit: bool) -> cadmpeg_core::CodecError {
    let body = [
        0x18, 0x18, 0x6d, 0xeb, 0x81, 0x84, 0xcc, 0xcc, 0xd0, 0x00, 0x0c, 0x9a, 0xd5, 0xd6, 0x25,
        0xa6, 0xec, 0x06, 0x18, 0x46, 0x1a, 0xdf, 0x09, 0x9b, 0x3c, 0x32, 0xed, 0x2f, 0x20, 0x00,
        0xd5, 0xd6, 0x25, 0xa6, 0xec, 0x06, 0x18, 0x46, 0x18, 0x81, 0x99, 0x6a, 0xa2, 0x99, 0x53,
        0x2e, 0x20, 0x33, 0xf7, 0x0c,
    ];
    let service = with_surface_limits(&body, u64::MAX, u64::MAX, |ctx| {
        crate::surface::scalar_tokens(
            ctx,
            crate::surface::SurfaceKind::Plane,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("corner parser fits service limits");
    assert_eq!(service.iter().filter(|token| token.offset >= 12).count(), 6);
    let dimension = if collection_limit { cadmpeg_core::decode::ResourceDimension::CollectionItems } else { cadmpeg_core::decode::ResourceDimension::RetainedBytes };
    let operation = if collection_limit { "creo surface scalar token items" } else { "creo surface scalar token bytes" };
    crate::test_support::last_refusal_at(&body, dimension, operation, |ctx| {
        crate::surface::scalar_tokens(ctx, crate::surface::SurfaceKind::Plane, &body, &scalar::ScalarCache::default())
    })

}

#[test]
fn plane_corner_refuses_token_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        &plane_corner_limit_error(true),
        ResourceDimension::CollectionItems,
        "creo surface scalar token items",
    );
}

#[test]
fn plane_corner_refuses_token_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        &plane_corner_limit_error(false),
        ResourceDimension::RetainedBytes,
        "creo surface scalar token bytes",
    );
}

#[test]
fn surface_opaque_refuses_span_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x01];
    let error = with_surface_limits(&body, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo surface opaque span items"), |cap| with_surface_limits(&body, cap, u64::MAX, |ctx| {
        crate::surface::opaque_spans(ctx, &body, &[])
    })), u64::MAX, |ctx| {
        crate::surface::opaque_spans(ctx, &body, &[])
    })
    .expect_err("one span exceeds zero collection items");
    assert_surface_limit(
        &error,
        ResourceDimension::CollectionItems,
        "creo surface opaque span items",
    );
}

#[test]
fn surface_opaque_refuses_span_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    let body = [0x01];
    let error = with_surface_limits(&body, u64::MAX, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo surface opaque span bytes"), |cap| with_surface_limits(&body, u64::MAX, cap, |ctx| {
        crate::surface::opaque_spans(ctx, &body, &[])
    })), |ctx| {
        crate::surface::opaque_spans(ctx, &body, &[])
    })
    .expect_err("one span byte exceeds zero retained bytes");
    assert_surface_limit(
        &error,
        ResourceDimension::RetainedBytes,
        "creo surface opaque span bytes",
    );
}

fn scalar_frame_limit_error(
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    let tokens = [crate::surface::SurfaceParameterScalar {
        value: Some(1.0),
        raw: vec![0xe4],
        offset: 0,
    }];
    with_surface_limits(&[0xe4], collection_limit, retained_limit, |ctx| {
        crate::surface::scalar_frames(ctx, &tokens)
    })
    .expect_err("one scalar frame exceeds requested limit")
}

#[test]
fn surface_scalar_frame_refuses_slot_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let boundary = limit_reaching_operation("creo surface scalar frame slots", |limit| scalar_frame_limit_error(limit, u64::MAX));
    assert_surface_limit(
        &scalar_frame_limit_error(boundary, u64::MAX),
        ResourceDimension::CollectionItems,
        "creo surface scalar frame slots",
    );
}

#[test]
fn surface_scalar_frame_refuses_slot_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    assert_surface_limit(
        &scalar_frame_limit_error(
            u64::MAX,
            crate::test_support::allocation_limit_at(
                cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                Some("creo surface scalar frame bytes"),
                |cap| Err::<(), _>(scalar_frame_limit_error(u64::MAX, cap)),
            ),
        ),
        ResourceDimension::RetainedBytes,
        "creo surface scalar frame bytes",
    );
}

#[test]
fn surface_scalar_frame_refuses_frame_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let boundary = limit_reaching_operation("creo surface scalar frame items", |limit| scalar_frame_limit_error(limit, u64::MAX));
    assert_surface_limit(
        &scalar_frame_limit_error(boundary, u64::MAX),
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
        7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3,
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = collection_limit;
    policy.limits.max_retained_bytes = retained_limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy)
        .expect("plane envelope fits root input limit");
    crate::surface::plane_envelopes(&ctx, &payload).expect_err("one envelope exceeds its limit")
}

#[test]
fn plane_envelope_refuses_scalar_token_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let limit = limit_reaching_operation("creo plane envelope scalar token items", |limit| {
        plane_envelope_limit_error(limit, u64::MAX)
    });
    let error = plane_envelope_limit_error(limit, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane envelope scalar token items"),
        "{error:?}"
    );
}

#[test]
fn plane_envelope_refuses_scalar_token_bytes() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_envelope_limit_error(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo plane envelope scalar token bytes"),
            |cap| Err::<(), _>(plane_envelope_limit_error(u64::MAX, cap)),
        ),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo plane envelope scalar token bytes")
    );
}

#[test]
fn plane_envelope_refuses_body_copy() {
    use cadmpeg_core::decode::ResourceDimension;
    let error = plane_envelope_limit_error(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo plane envelope body"),
            |cap| Err::<(), _>(plane_envelope_limit_error(u64::MAX, cap)),
        ),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo plane envelope body")
    );
}

#[test]
fn plane_envelope_refuses_output_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let limit = limit_reaching_operation("creo plane envelopes", |limit| {
        plane_envelope_limit_error(limit, u64::MAX)
    });
    let error = plane_envelope_limit_error(limit, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane envelopes"),
        "{error:?}"
    );
}

#[test]
fn named_plane_outline_refuses_envelope_output_vector() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let payload = b"srf_array\0\xf8\x01\xe0\x01geom_id\0\x07\xe0\x01geom_type\0\x22\xe0\x01feat_id\0\x04\xe0\x01orient\0\x01\xe0\x01boundary_type\0\x00\xe0\x01next_geom_ptr\0\x00\xe0\x02outline\0\xf9\x02\x03\xe4\x18\xe4\xe4\xe4\x18\xe0\x00srf_prim_ptr(plane)\0\xe3";
    let arena = DecodeArena::new();
    let run = |limit| {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("named outline fits root input limit");
        crate::surface::plane_envelopes(&ctx, payload)
            .expect_err("six tokens and row admission leave no envelope output slot")
    };
    let limit = limit_reaching_operation("creo plane envelopes", run);
    let error = run(limit);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane envelopes"),
        "{error:?}"
    );
}

fn plane_local_system_limit_error(
    collection_limit: u64,
    retained_limit: u64,
) -> cadmpeg_core::CodecError {
    use crate::surface::{BoundaryType, SurfaceKind, SurfaceRow};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let payload = [
        7, 0x22, 4, 0x01, 0, 0, 0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3,
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
    let error = plane_local_system_limit_error(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            Some("creo plane local-system body"),
            |cap| Err::<(), _>(plane_local_system_limit_error(u64::MAX, cap)),
        ),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo plane local-system body"),
        "{error:?}"
    );
}

#[test]
fn plane_local_system_refuses_output_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    let limit = limit_reaching_operation("creo plane local systems", |limit| {
        plane_local_system_limit_error(limit, u64::MAX)
    });
    let error = plane_local_system_limit_error(limit, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo plane local systems"),
        "{error:?}"
    );
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

    let error = crate::test_support::last_refusal_at(&body, cadmpeg_core::decode::ResourceDimension::CollectionItems, "creo local-system scalar slots", |ctx| { crate::surface::sequential_named_local_system_slots(
        ctx,
        &body,
        1,
        &scalar::ScalarCache::default(),
        &mut crate::surface::ScalarBodyRefusal::default(),
    ) });
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
    let error = named_surface_limit_error("flip", &[0xf1], u64::MAX, crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo opaque surface parameter bytes"), |cap| Err::<(), _>(named_surface_limit_error("flip", &[0xf1], u64::MAX, cap))));
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo opaque surface parameter bytes"
    ));
}

#[test]
fn zero_surface_scalar_refuses_before_vector_allocation() {
    let error = named_surface_limit_error("radius", &[0x18], crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo zero surface scalar"), |cap| Err::<(), _>(named_surface_limit_error("radius", &[0x18], cap, u64::MAX))), u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo zero surface scalar"
    ));
}

#[test]
fn compact_surface_integers_refuse_before_vector_growth() {
    let error = named_surface_limit_error("dum_array", &[0xf8, 0x02, 0x07, 0x08], crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo compact surface integers"), |cap| Err::<(), _>(named_surface_limit_error("dum_array", &[0xf8, 0x02, 0x07, 0x08], cap, u64::MAX))), u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo compact surface integers"
    ));
}

#[test]
fn contiguous_surface_references_refuse_before_vector_allocation() {
    let error =
        named_surface_limit_error("i_pnts", &[0xf8, 0x03, 0xf7, 0x80, 0x80, 0xfb], crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo contiguous surface references"), |cap| Err::<(), _>(named_surface_limit_error("i_pnts", &[0xf8, 0x03, 0xf7, 0x80, 0x80, 0xfb], cap, u64::MAX))), u64::MAX);
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "creo contiguous surface references"
    ));
}
