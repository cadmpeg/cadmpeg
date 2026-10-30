// SPDX-License-Identifier: Apache-2.0

use crate::scalar;
use crate::surface::SurfacePrototypeFamily;

fn with_surface_ctx<T>(
    input: &[u8],
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> Result<T, cadmpeg_core::CodecError> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(input, &arena, &policy).expect("input fits root byte limit");
    run(&ctx)
}

#[test]
fn named_spline_invalid_scalar_has_no_slots() {
    let body = [0xff];
    let slots = crate::surface::named_spline_scalar_slots(
        &SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline),
        "tangts",
        &body,
        1,
        &scalar::ScalarCache::default(),
        &mut crate::surface::ScalarBodyRefusal::default(),
    );
    assert!(slots.is_none());
}

#[test]
fn named_local_system_invalid_scalar_has_no_slots() {
    let body = [0xff];
    let slots = crate::surface::sequential_named_local_system_slots(
        &body,
        1,
        &scalar::ScalarCache::default(),
        &mut crate::surface::ScalarBodyRefusal::default(),
    );
    assert!(slots.is_none());
}

#[test]
fn surface_parameter_reads_one_header() {
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
    let service = with_surface_ctx(&payload, |ctx| {
        crate::surface::parameter_records_for_rows(ctx, &payload, &rows, &mut Vec::new())
    })
    .expect("one parameter record fits service limits");
    assert_eq!(service.len(), 1);
}

#[test]
fn surface_scalar_reads_one_token() {
    let body = [0xe4];
    let service = crate::surface::scalar_tokens(
        crate::surface::SurfaceKind::Plane,
        &body,
        &scalar::ScalarCache::default(),
    );
    assert_eq!(service.len(), 1);
}

#[test]
fn surface_token_slot_table_reads_two_slots() {
    let body = [0xe4, 0x18];
    let table =
        crate::surface::scalar_slots_with_tokens_and_end(&body, 2, &scalar::ScalarCache::default())
            .expect("two slots are admitted");
    assert_eq!(table.slots.len(), 2);
}

#[test]
fn plane_envelope_slot_table_reads_three_slots() {
    let body = [0x0e, 0x0e, 0x18];
    let table = crate::surface::plane_envelope_scalar_slots_with_tokens_and_end(
        &body,
        3,
        &scalar::ScalarCache::default(),
    )
    .expect("three slots are admitted");
    assert_eq!(table.slots.len(), 3);
}

#[test]
fn plane_envelope_final_positive_slot_completes_table() {
    let body = [
        0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f, 0x99, 1, 2, 3, 4, 5, 6,
    ];
    let table = crate::surface::complete_plane_envelope_slots_with_final_positive_dict(
        &body,
        9,
        &scalar::ScalarCache::default(),
    )
    .expect("ten slots are admitted");
    assert_eq!(table.slots.len(), 10);
}

#[test]
fn plane_envelope_close_slot_finds_close() {
    let body = [
        0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f, 0x99, 1, 2, 3, 4, 5, 6, 0xe3,
    ];
    let close = with_surface_ctx(&body, |ctx| {
        crate::surface::plane_envelope_compound_close(
            ctx,
            &body,
            &scalar::ScalarCache::default(),
            &mut Vec::new(),
        )
    })
    .expect("ten slots are admitted");
    assert_eq!(close, Some(body.len() - 1));
}

#[test]
fn plane_envelope_reader_reads_one_envelope() {
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
    let envelopes = with_surface_ctx(&payload, |ctx| {
        crate::surface::plane_envelopes_for_rows(ctx, &payload, &rows, &mut Vec::new())
    })
    .expect("compute the plane envelopes for rows");
    assert_eq!(envelopes.len(), 1);
}

#[test]
fn named_surface_scalar_sequence_reads_one_scalar() {
    let body = [0x0e];
    let value = with_surface_ctx(&body, |ctx| {
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
    .expect("one scalar is admitted");
    assert_eq!(
        value,
        crate::surface::SurfaceNamedValue::ScalarSequence(vec![0.5])
    );
}

#[test]
fn normalized_plane_frame_reads_local_system() {
    let body = [
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x18, 0x2f, 0x05, 0x00, 0x00, 0x0c, 0x98,
    ];
    let frame = with_surface_ctx(&body, |ctx| {
        crate::surface::complete_plane_local_system(
            ctx,
            &body,
            &scalar::ScalarCache::default(),
            &mut Vec::new(),
        )
    })
    .expect("twelve normalized bytes are admitted");
    assert!(frame.is_some());
}

#[test]
fn local_system_slots_read_declared_count() {
    let body = [0x10];
    let values = crate::surface::sequential_named_local_system_slots(
        &body,
        1,
        &scalar::ScalarCache::default(),
        &mut crate::surface::ScalarBodyRefusal::default(),
    );
    assert_eq!(values, Some(vec![Some(0.0)]));
}

#[test]
fn compact_plane_origin_requires_exact_consumption_and_reports_its_loss() {
    let mut body = Vec::new();
    for _ in 0..2 {
        body.extend([0x0f, 0x18, 0xe5]);
    }
    body.push(0x0f);
    body.extend([0x2f, 0x00, 0x00, 0x18, 0xe4]);
    let cache = scalar::ScalarCache::default();
    let mut losses = Vec::new();
    let parsed = with_surface_ctx(&body, |ctx| {
        crate::surface::complete_plane_local_system(ctx, &body, &cache, &mut losses)
    })
    .expect("bounded frame")
    .expect("complete frame");
    assert_eq!(&parsed.0.as_raw()[9..], &[2.0, 0.0, 0.0]);
    assert!(losses.is_empty());
    body.push(0x18);
    assert!(
        with_surface_ctx(&body, |ctx| crate::surface::complete_plane_local_system(
            ctx,
            &body,
            &cache,
            &mut losses
        ))
        .expect("bounded frame")
        .is_none()
    );
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::PlaneCompactFrameUnresolved.kind()
    );
    let reflected = [0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x10, 0x18, 0xe5];
    losses.clear();
    assert!(with_surface_ctx(&reflected, |ctx| {
        crate::surface::complete_plane_local_system(ctx, &reflected, &cache, &mut losses)
    })
    .expect("bounded reflected frame")
    .is_none());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::CreoLossCode::PlaneCompactFrameUnresolved.kind()
    );
}

#[test]
fn plane_envelope_slots_keep_signed_units_and_bounded_scalar_repeats() {
    let body = [
        0x2f, 0, 0, 0xe4, 0x18, 0x0f, 0x2d, 0x10, 0, 0, 0, 0, 0, 0, 0xe5, 0x10, 0x0e, 0xe4,
    ];
    let table =
        crate::surface::complete_plane_envelope_slots(&body, 10, &scalar::ScalarCache::default())
            .expect("complete stateful envelope");
    let values: Vec<_> = table.slots.iter().map(|slot| slot.0).collect();
    assert_eq!(
        values,
        [2.0, 2.0, 0.0, 1.0, 4.0, 4.0, 4.0, -1.0, 0.5, 0.5].map(Some)
    );
    for body in [
        &[0xe4, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18][..],
        &[0x0f, 0xe6, 0xe6, 0xe5, 0xe5][..],
    ] {
        assert!(crate::surface::complete_plane_envelope_slots(
            body,
            10,
            &scalar::ScalarCache::default()
        )
        .is_none());
    }
}

#[test]
fn plane_envelope_invalid_literal_fallback_is_retained_with_a_loss() {
    let payload = [
        7, 0x22, 4, 1, 0, 0, 0xe4, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xff, 0xe3,
    ];
    let mut losses = Vec::new();
    let envelopes = with_surface_ctx(&payload, |ctx| {
        crate::surface::plane_envelopes_with_losses(ctx, &payload, &mut losses)
    })
    .expect("bounded plane envelope");
    assert!(envelopes.is_empty());
    assert!(losses
        .iter()
        .any(|loss| loss.code == crate::loss::CreoLossCode::PlaneEnvelopeUnresolved.kind()));
}

#[test]
fn standard_plane_envelope_prefix_selects_its_literal_production() {
    let body = [0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f, 0xe4];
    let cache = scalar::ScalarCache::default();
    let slots = crate::surface::complete_standard_plane_envelope_slots(&body, &cache)
        .expect("complete literal envelope");
    assert_eq!(
        slots
            .slots
            .iter()
            .map(|slot| slot.0.expect("finite envelope scalar"))
            .collect::<Vec<_>>(),
        [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0]
    );
    let mut unsupported_full_ieee = vec![0x0f; 4];
    unsupported_full_ieee.extend_from_slice(&[0xed, 0x7f, 0xf8, 0, 0, 0, 0, 0, 0]);
    unsupported_full_ieee.extend_from_slice(&[0x0f; 5]);
    assert!(
        crate::surface::complete_standard_plane_envelope_slots(&unsupported_full_ieee, &cache)
            .is_none()
    );
    let mut trailing = body.to_vec();
    trailing.push(0x18);
    assert!(crate::surface::complete_standard_plane_envelope_slots(&trailing, &cache).is_none());
    let mut stateful = body;
    stateful[0] = 0x18;
    let slots = crate::surface::complete_standard_plane_envelope_slots(&stateful, &cache)
        .expect("complete stateful envelope");
    assert_eq!(
        slots
            .slots
            .iter()
            .map(|slot| slot.0.expect("finite envelope scalar"))
            .collect::<Vec<_>>(),
        [0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
    );
}

#[test]
fn standard_plane_literal_fallback_reconciles_with_the_stored_frame() {
    let body = [
        0x2f, 0, 0, 0xe4, 0x0f, 0xe4, 0x0f, 0xe4, 0x0f, 0xe4, 0xe4, 0x0f,
    ];
    let cache = scalar::ScalarCache::default();
    let slots = crate::surface::complete_standard_plane_envelope_slots(&body, &cache)
        .expect("complete stateful envelope");
    let record = crate::surface::PlaneEnvelopeRecord {
        surface_id: 7,
        body: body.to_vec(),
        envelope: crate::surface::PlaneEnvelope::Standard {
            bounds_2d: [
                [slots.slots[0].0, slots.slots[1].0],
                [slots.slots[2].0, slots.slots[3].0],
            ],
            corners_3d: [
                [slots.slots[4].0, slots.slots[5].0, slots.slots[6].0],
                [slots.slots[7].0, slots.slots[8].0, slots.slots[9].0],
            ],
        },
        corner_coordinate_equal: [Some(true); 3],
        scalar_tokens: crate::surface::copied_plane_envelope_tokens(&slots)
            .into_iter()
            .map(Into::into)
            .collect(),
        row_offset: 0,
        offset: 6,
    };
    for (z, expected) in [(0.0, 0.0), (1.0, 1.0)] {
        let frame = crate::surface::PlaneLocalSystem {
            surface_id: 7,
            body: Vec::new(),
            slots: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, z].map(Some),
            layout: Some(scalar::PlaneSupportFrameLayout::SupportTriples),
            classification: crate::surface::LocalSystemClassification::Unclassified,
            row_offset: 0,
            offset: 20,
        };
        let mut records = [record.clone()];
        crate::surface::reconcile_plane_envelope_literals(&mut records, &[frame], &cache);
        let crate::surface::PlaneEnvelope::Standard { corners_3d, .. } = &records[0].envelope
        else {
            panic!("standard envelope")
        };
        assert_eq!(corners_3d[0][2], Some(expected));
        assert_eq!(corners_3d[1][2], Some(expected));
    }
}
