// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

mod positional_frames;
mod resource_limits;

use super::counted_parameter_scalar_slots;
use super::named_prototype_records;
use super::named_surface_value;
use super::plane_local_systems;
use crate::psb;
use crate::scalar;
use crate::surface::admitted_counted_parameter_body;
use crate::surface::complete_plane_local_system_slots;
use crate::surface::decode_row_scalar;
use crate::surface::plane_direct_frame;
use crate::surface::plane_envelope_scalar_slots_with_tokens_and_end;
use crate::surface::plane_frame;
use crate::surface::plane_local_system_compound_close;
use crate::surface::plane_matrix_frame;
use crate::surface::rows as checked_rows;
use crate::surface::scalar_slots_with_tokens_and_end;
use crate::surface::slot_equality;
use crate::surface::LocalSystemClassification;
use crate::surface::OutlinePlane;
use crate::surface::PlaneEnvelope;
use crate::surface::PlaneEnvelopeRecord;
use crate::surface::PlaneLocalSystem;
use crate::surface::ScalarBodyRefusal;
use crate::surface::SurfaceKind;
use crate::surface::SurfaceNamedValue;
use crate::surface::SurfaceParameterOpaqueSpan;
use crate::surface::SurfaceParameterScalar;
use crate::surface::SurfaceParameterScalarFrame;
use crate::surface::SurfacePrototypeFamily;
use crate::surface::SurfaceRow;

fn rows(payload: &[u8]) -> Vec<SurfaceRow> {
    crate::decode::with_test_decode_ctx(|ctx| checked_rows(ctx, payload))
        .expect("surface rows are admitted")
}

fn service_scalar_tokens(
    kind: SurfaceKind,
    body: &[u8],
    cache: &scalar::ScalarCache,
) -> Vec<SurfaceParameterScalar> {
    crate::decode::with_test_decode_ctx(|ctx| crate::surface::scalar_tokens(ctx, kind, body, cache))
        .expect("scalar tokens fit service limits")
}

fn service_opaque_spans(
    body: &[u8],
    tokens: &[SurfaceParameterScalar],
) -> Vec<SurfaceParameterOpaqueSpan> {
    crate::decode::with_test_decode_ctx(|ctx| crate::surface::opaque_spans(ctx, body, tokens))
        .expect("opaque spans fit service limits")
}

fn service_scalar_frames(tokens: &[SurfaceParameterScalar]) -> Vec<SurfaceParameterScalarFrame> {
    crate::decode::with_test_decode_ctx(|ctx| crate::surface::scalar_frames(ctx, tokens))
        .expect("scalar frames fit service limits")
}

fn service_complete_plane_compact_scalar_suffix<'a>(
    body: &'a [u8],
    cache: &scalar::ScalarCache,
) -> Option<Vec<(Option<f64>, &'a [u8])>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        crate::surface::complete_plane_compact_scalar_suffix(ctx, body, cache)
            .map(|result| result.map(|table| table.slots))
    })
    .expect("compact suffix fits service limits")
}

fn frame_bound_outline_planes(
    envelopes: &[PlaneEnvelopeRecord],
    frames: &[PlaneLocalSystem],
) -> Vec<OutlinePlane> {
    let mut result = envelopes
        .iter()
        .filter_map(|record| crate::surface::frame_bound_outline_plane(record, frames))
        .collect::<Vec<_>>();
    result.sort_by_key(|plane| plane.offset);
    result
}

fn outline_planes(envelopes: &[PlaneEnvelopeRecord]) -> Vec<OutlinePlane> {
    super::with_decode_ctx(&[], |ctx| crate::surface::outline_planes(ctx, envelopes))
}

fn plane_envelopes(payload: &[u8]) -> Vec<PlaneEnvelopeRecord> {
    super::with_decode_ctx(payload, |ctx| crate::surface::plane_envelopes(ctx, payload))
}

fn sequential_named_local_system_slots(
    body: &[u8],
    count: usize,
    cache: &scalar::ScalarCache,
    refusal: &mut ScalarBodyRefusal,
) -> Option<Vec<Option<f64>>> {
    super::with_decode_ctx(body, |ctx| {
        crate::surface::sequential_named_local_system_slots(ctx, body, count, cache, refusal)
    })
}

#[test]
fn derives_one_held_coordinate_outline_plane() {
    let records = [PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
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
    assert_eq!(
        outline_planes(&records),
        vec![OutlinePlane {
            surface_id: 42,
            origin: [3.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 20,
        }]
    );
}

#[test]
fn held_coordinate_outline_refuses_output_vector() {
    let records = [PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
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
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo held-coordinate outline planes",
        |ctx| crate::surface::outline_planes(ctx, &records),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo held-coordinate outline planes")
    );
}

fn placed_frame_bound_limit_error(limit: u64) -> cadmpeg_core::CodecError {
    let records = [PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-3.0), Some(-4.0), Some(7.0)],
                [Some(5.0), Some(-4.0), None],
            ],
        },
        corner_coordinate_equal: [Some(false), Some(true), None],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    }];
    let frames = [PlaneLocalSystem {
        surface_id: 42,
        body: Vec::new(),
        slots: [
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 100.0, 200.0, 300.0,
        ]
        .map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 30,
    }];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    crate::surface::placed_outline_planes(&ctx, &records, &frames)
        .expect_err("placed outline collection exceeds limit")
}

#[test]
fn placed_outline_refuses_frame_bound_vector() {
    let error = placed_frame_bound_limit_error(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo frame-bound outline planes"),
        |cap| Err::<(), _>(placed_frame_bound_limit_error(cap)),
    ));
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo frame-bound outline planes")
    );
}

#[test]
fn placed_outline_refuses_frame_bound_id_node() {
    let error = placed_frame_bound_limit_error(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo frame-bound outline ID nodes"),
        |cap| Err::<(), _>(placed_frame_bound_limit_error(cap)),
    ));
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo frame-bound outline ID nodes")
    );
}

#[test]
fn placed_outline_refuses_output_vector() {
    let error = placed_frame_bound_limit_error(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo placed outline planes"),
        |cap| Err::<(), _>(placed_frame_bound_limit_error(cap)),
    ));
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo placed outline planes")
    );
}

#[test]
fn compact_plane_scalar_suffix_requires_one_complete_nine_slot_frame() {
    let body = [
        0x32, 0xbe, 0xe4, 0xe4, 0xe4, 0x0d, 0x0f, 0xe4, 0x0d, 0xe4, 0x0f,
    ];
    let slots =
        service_complete_plane_compact_scalar_suffix(&body, &scalar::ScalarCache::default())
            .expect("unique compact scalar suffix");

    assert_eq!(
        slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
        vec![
            Some(1.0),
            Some(1.0),
            Some(1.0),
            Some(-1.0),
            Some(0.0),
            Some(1.0),
            Some(-1.0),
            Some(1.0),
            Some(0.0),
        ]
    );
    assert!(service_complete_plane_compact_scalar_suffix(
        &body[2..],
        &scalar::ScalarCache::default()
    )
    .is_none());
}

#[test]
fn positional_plane_envelope_rejects_bytes_before_a_complete_standard_frame() {
    let payload = [
        7, 0x22, 4, 0x01, 0, 0, 0xfb, 0x0f, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0xe4, 0x0f, 0xe4,
        0xe3,
    ];

    assert_eq!(rows(&payload).len(), 1);
    assert!(plane_envelopes(&payload).is_empty());
}

#[test]
fn plane_envelope_scalar_tokens_take_precedence_over_compound_close_bytes() {
    let body = [
        70, 32, 107, 133, 30, 184, 81, 235, 70, 47, 201, 160, 13, 107, 10, 126, 47, 32, 0, 24, 70,
        32, 107, 133, 30, 184, 81, 235, 70, 47, 201, 160, 13, 107, 10, 126, 142, 71, 174, 20, 122,
        225, 72, 47, 32, 0, 24, 142, 71, 174, 20, 122, 225, 72,
    ];
    let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(psb::token::COMPOUND_CLOSE);

    let envelopes = plane_envelopes(&payload);
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].body, body);
    assert_eq!(
        envelopes[0].corner_coordinate_equal,
        [Some(false), Some(false), Some(true)]
    );
}

#[test]
fn plane_envelope_positive_dict_scalar_owns_an_e3_tail() {
    let body = [
        0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f, 0x99, 1, 2, 3, 4, 5, 6,
    ];
    let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(psb::token::COMPOUND_CLOSE);

    let envelopes = plane_envelopes(&payload);
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].body, body);
    assert_eq!(envelopes[0].scalar_tokens.len(), 10);
    assert_eq!(envelopes[0].scalar_tokens[9], vec![0x99, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn plane_envelope_positive_dict_recovery_requires_the_final_slot() {
    let body = [
        0x0f, 0xe4, 0x99, 1, 2, 3, 4, 5, 6, 0x0d, 0x0f, 0x0f, 0xe4, 0x0d, 0x0f, 0x0d,
    ];
    let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(psb::token::COMPOUND_CLOSE);

    assert!(plane_envelopes(&payload).is_empty());
}

#[test]
fn compact_plane_envelope_positive_dict_scalar_owns_an_e3_tail() {
    let body = [
        0x0e, 0x0f, 0xe4, 0x0d, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0x99, 1, 2, 3, 4, 5, 6,
    ];
    let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(psb::token::COMPOUND_CLOSE);

    let envelopes = plane_envelopes(&payload);
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].body, body);
    assert_eq!(envelopes[0].scalar_tokens.len(), 9);
    assert_eq!(envelopes[0].scalar_tokens[8], vec![0x99, 1, 2, 3, 4, 5, 6]);
}

#[test]
fn plane_envelope_coordinates_decode_compact_positive_half() {
    let body = [
        0x0f, 0xe4, 0x0d, 0x0f, 0x43, 0xe0, 0x00, 0xe4, 0x0f, 0x0e, 0xe4, 0x0f,
    ];
    let (slots, consumed) = crate::decode::with_test_decode_ctx(|ctx| {
        plane_envelope_scalar_slots_with_tokens_and_end(
            ctx,
            &body,
            10,
            &scalar::ScalarCache::default(),
        )
        .map(|result| result.map(|table| (table.slots, table.consumed)))
    })
    .expect("envelope slots are admitted")
    .expect("a complete ten-slot envelope table");

    assert_eq!(consumed, body.len());
    assert_eq!(slots[4].0, Some(-0.5));
    assert_eq!(slots[7].0, Some(0.5));
    assert_eq!(slot_equality(&slots[4], &slots[7]), Some(false));
    assert_eq!(slot_equality(&slots[5], &slots[8]), Some(true));
    assert_eq!(slot_equality(&slots[6], &slots[9]), Some(true));
}

#[test]
fn decodes_named_plane_outline_with_zero_boundary_type() {
    let payload = b"srf_array\0\xf8\x01\xe0\x01geom_id\0\x07\xe0\x01geom_type\0\x22\xe0\x01feat_id\0\x04\xe0\x01orient\0\x01\xe0\x01boundary_type\0\x00\xe0\x01next_geom_ptr\0\x00\xe0\x02outline\0\xf9\x02\x03\xe4\x18\xe4\xe4\xe4\x18\xe0\x00srf_prim_ptr(plane)\0\xe3";

    assert_eq!(rows(payload).len(), 1);
    assert_eq!(plane_envelopes(payload).len(), 1);

    assert_eq!(
        outline_planes(&plane_envelopes(payload)),
        vec![OutlinePlane {
            surface_id: 7,
            origin: [1.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 104,
        }]
    );
}

#[test]
fn named_plane_outline_rejects_bytes_between_the_wrapper_and_slots() {
    let payload = b"srf_array\0\xf8\x01\xe0\x01geom_id\0\x07\xe0\x01geom_type\0\x22\xe0\x01feat_id\0\x04\xe0\x01orient\0\x01\xe0\x01boundary_type\0\x00\xe0\x01next_geom_ptr\0\x00\xe0\x02outline\0\xf9\x02\x03\xfb\xe4\x18\xe4\xe4\xe4\x18\xe0\x00srf_prim_ptr(plane)\0\xe3";

    assert_eq!(rows(payload).len(), 1);
    assert!(plane_envelopes(payload).is_empty());
}

#[test]
fn derives_plane_with_unresolved_distinct_corner_coordinates() {
    let records = [PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-3.0), Some(-4.0), None],
                [Some(5.0), Some(-4.0), None],
            ],
        },
        corner_coordinate_equal: [Some(false), Some(true), Some(false)],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    }];
    assert_eq!(outline_planes(&records)[0].origin, [0.0, -4.0, 0.0]);
    assert_eq!(outline_planes(&records)[0].normal(), [0.0, 1.0, 0.0]);
}

#[test]
fn support_frame_selects_held_axis_with_unresolved_other_coordinate() {
    let records = [PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-3.0), Some(-4.0), Some(7.0)],
                [Some(5.0), Some(-4.0), None],
            ],
        },
        corner_coordinate_equal: [Some(false), Some(true), None],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    }];
    let frames = [PlaneLocalSystem {
        surface_id: 42,
        body: Vec::new(),
        slots: [
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 100.0, 200.0, 300.0,
        ]
        .map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 30,
    }];

    assert_eq!(
        frame_bound_outline_planes(&records, &frames),
        [OutlinePlane {
            surface_id: 42,
            origin: [0.0, -4.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            offset: 20,
        }]
    );

    let agreeing_frames = [frames[0].clone(), frames[0].clone()];
    assert_eq!(
        frame_bound_outline_planes(&records, &agreeing_frames),
        frame_bound_outline_planes(&records, &frames)
    );
    let mut conflicting = frames[0].clone();
    conflicting.slots[6..9].copy_from_slice(&[Some(1.0), Some(0.0), Some(0.0)]);
    assert!(frame_bound_outline_planes(&records, &[frames[0].clone(), conflicting]).is_empty());
}

#[test]
fn support_frame_maps_shortened_terminal_outline_coordinate() {
    let records = [PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-3.0), Some(-4.0), Some(7.0)],
                [Some(-4.0), None, None],
            ],
        },
        corner_coordinate_equal: [Some(false), None, None],
        scalar_tokens: vec![
            vec![1],
            vec![2],
            vec![3],
            vec![4],
            vec![5],
            vec![6],
            vec![7],
            vec![6],
            Vec::new(),
            Vec::new(),
        ],
        row_offset: 10,
        offset: 20,
    }];
    let frames = [PlaneLocalSystem {
        surface_id: 42,
        body: Vec::new(),
        slots: [
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 100.0, 200.0, 300.0,
        ]
        .map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Simple,
        row_offset: 10,
        offset: 30,
    }];

    assert_eq!(
        frame_bound_outline_planes(&records, &frames)[0].origin,
        [0.0, -4.0, 0.0]
    );
}

#[test]
fn positional_plane_frame_decodes_terminal_zero_before_null_tail() {
    let body = [
        0x18, 0xe4, 0x0f, 0x10, 0x18, 0xe5, 0x10, 0x18, 0x2f, 0x18, 0x00, 0x2d, 0x29, 0x3d, 0x70,
        0xa3, 0xd7, 0x0a, 0x3d, 0x18, 0xe1,
    ];

    let slots = complete_plane_local_system_slots(&body, &scalar::ScalarCache::default())
        .expect("complete frame");
    assert_eq!(
        slots,
        [0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 6.0, -12.62, 0.0]
    );
    let frame = plane_frame(&slots.map(Some));
    assert_eq!(frame.origin, Some([6.0, -12.62, 0.0]));
    assert_eq!(frame.u_axis(), Some([0.0, 1.0, 0.0]));
    assert_eq!(frame.normal(), Some([1.0, 0.0, 0.0]));
}

#[test]
fn explicit_plane_frame_uses_the_stored_normal_triple() {
    let slots = [
        0.6, 0.0, 0.8, // parameter direction
        0.0, 0.0, 0.0, // zero rank
        0.8, 0.0, -0.6, // stored plane normal
        2.0, 3.0, 4.0,
    ];

    let frame = plane_direct_frame(&slots.map(Some));
    assert_eq!(frame.origin, Some([2.0, 3.0, 4.0]));
    assert_eq!(frame.u_axis(), Some([0.6, 0.0, 0.8]));
    assert_eq!(frame.normal(), Some([0.8, 0.0, -0.6]));
}

#[test]
fn positional_plane_frame_decodes_outline_separator_zero_suffix() {
    let first = [
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x18, 0x2f, 0x05, 0x00, 0x00, 0x0c, 0x98,
    ];
    let second = [
        0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x0f, 0x18, 0x2a, 0xfa, 0x00, 0x00, 0x0c, 0x98,
    ];

    assert_eq!(
        complete_plane_local_system_slots(&first, &scalar::ScalarCache::default())
            .map(|slots| [slots[9], slots[10], slots[11]]),
        Some([0.0, 2.625, 0.0])
    );
    assert_eq!(
        complete_plane_local_system_slots(&second, &scalar::ScalarCache::default())
            .map(|slots| [slots[9], slots[10], slots[11]]),
        Some([0.0, 1.625, 0.0])
    );
}

#[test]
fn outline_separator_precedes_compact_integer_alias_of_compound_close() {
    let payload = [0x0f, 0x00, 0x0c, 0x98, 0xe3, 0xe0, 0x01, b'x', 0];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| crate::surface::first_compound_close(
            ctx,
            &payload,
            0,
            payload.len()
        ))
        .expect("token admission"),
        Some(4)
    );
}

#[test]
fn plane_local_system_close_validates_past_an_e0_numeric_byte() {
    let mut payload = vec![
        0x4e, 0xf0, 0, 0, 0, 0, 0xe0, // finite first support coordinate
        0x18, // zero second coordinate
        0x4c, 0xf0, 0, 0, 0, 0, 0, // finite third coordinate
        0x10, 0x10, 0x10, // zero-rank triple
        0x10, 0x10, 0x4c, 0xf0, 0, 0, 0, 0, 0, // second support triple
        0x10, 0x10, 0x18, // origin
    ];
    let close = payload.len();
    payload.push(psb::token::COMPOUND_CLOSE);
    payload.extend_from_slice(&[psb::token::NAMED_RECORD, 0x01, b'x', 0]);
    let cache = scalar::ScalarCache::from_section(&payload);

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| crate::surface::first_compound_close(
            ctx,
            &payload,
            0,
            payload.len()
        ))
        .expect("token admission"),
        None
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            plane_local_system_compound_close(ctx, &payload, 0, payload.len(), &cache)
        })
        .expect("local-system close fits service limits"),
        Some(close)
    );
}

#[test]
fn positional_plane_frame_decodes_rank_two_image_before_null_tail() {
    let body = [0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe1];

    let slots = complete_plane_local_system_slots(&body, &scalar::ScalarCache::default())
        .expect("complete rank-two frame");
    assert_eq!(
        slots,
        [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    );
    let frame = plane_frame(&slots.map(Some));
    assert_eq!(frame.origin, Some([0.0, 0.0, 0.0]));
    assert_eq!(frame.u_axis(), Some([0.0, 1.0, 0.0]));
    assert_eq!(frame.normal(), Some([0.0, 0.0, -1.0]));
}

#[test]
fn positional_plane_frame_classifies_rank_two_image_before_null_tail() {
    let payload = [
        7, 0x22, 4, 0x01, 0, 0, // plane row
        0xe4, 0xe4, 0xe4, 0xe4, 0x0f, 0x0f, 0x0f, 0xe4, 0x0f, 0xe4, 0xe3, // envelope
        0x18, 0xe4, 0x0f, 0xe4, 0x18, 0xe5, 0x0f, 0x18, 0xe6, 0xe1, 0xe3, // local system
    ];

    let systems = plane_local_systems(&payload);
    assert_eq!(systems.len(), 1);
    assert_eq!(systems[0].classification, LocalSystemClassification::Simple);
    assert_eq!(systems[0].frame().normal(), Some([0.0, 0.0, -1.0]));
}

#[test]
fn positional_plane_frame_rejects_unconsumed_row_bytes() {
    let body = [
        0x18, 0xe4, 0x0f, 0x10, 0x18, 0xe5, 0x10, 0x18, 0x2f, 0x18, 0x00, 0x2d, 0x29, 0x3d, 0x70,
        0xa3, 0xd7, 0x0a, 0x3d, 0x18, 0x00,
    ];

    assert_eq!(
        complete_plane_local_system_slots(&body, &scalar::ScalarCache::default()),
        None
    );
}

#[test]
fn positional_plane_frame_requires_one_unique_orthogonal_support_pair() {
    let options = |slots: [f64; 12]| slots.map(Some);
    assert_eq!(
        plane_frame(&options([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        ]))
        .normal(),
        Some([0.0, 0.0, 1.0])
    );
    let first_rank_zero = plane_frame(&options([
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -3.0, 0.0,
    ]));
    assert_eq!(first_rank_zero.origin, Some([0.0, -3.0, 0.0]));
    assert_eq!(first_rank_zero.u_axis(), Some([1.0, 0.0, 0.0]));
    assert_eq!(first_rank_zero.normal(), Some([0.0, -1.0, 0.0]));
    assert_eq!(
        plane_frame(&options([
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
        ]))
        .normal(),
        Some([0.0, 0.0, 1.0])
    );
    assert!(plane_frame(&options([
        1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0,
    ]))
    .normal
    .is_none());
    assert!(plane_frame(&options([
        1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ]))
    .normal
    .is_none());
}

#[test]
fn positional_plane_frame_keeps_large_orthogonal_support_normal_unit() {
    let slots = [
        1.0e100, 0.0, 0.0, 0.0, 1.0e100, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
    ];
    let frame = plane_frame(&slots.map(Some));

    assert_eq!(frame.u_axis(), Some([1.0, 0.0, 0.0]));
    assert_eq!(frame.normal(), Some([0.0, 0.0, 1.0]));
}

#[test]
fn positional_plane_frame_refuses_overflowed_cross_component() {
    let a = f64::from_bits(0x5fed_817d_bb14_96d1);
    let b = f64::from_bits(0x5fd8_c57e_64a4_a42f);
    let slots = [a, b, 0.0, -b, a, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    let frame = plane_frame(&slots.map(Some));

    assert_eq!(frame.origin, Some([0.0, 0.0, 0.0]));
    assert!(frame.u_axis.is_none());
    assert!(frame.normal.is_none());
}

#[test]
fn matrix_plane_frame_uses_stored_direction_and_normal_columns() {
    let slots = [
        1.0, 0.0, 0.0, // x components of the three columns
        0.0, 0.0, 0.0, // zero-rank column
        0.0, 0.0, 1.0, // z components of the three columns
        2.0, 3.0, 4.0,
    ];

    let frame = plane_matrix_frame(&slots.map(Some));
    assert_eq!(frame.origin, Some([2.0, 3.0, 4.0]));
    assert_eq!(frame.u_axis(), Some([1.0, 0.0, 0.0]));
    assert_eq!(frame.normal(), Some([0.0, 0.0, 1.0]));
}

#[test]
fn signed_surface_dict_slots_decode_as_mirrors() {
    let body = [
        0xbb, 1, 2, 3, 4, 5, 6, 0xbb, 1, 2, 3, 4, 5, 6, 0x73, 1, 2, 3, 4, 5, 6,
    ];
    let slots = crate::decode::with_test_decode_ctx(|ctx| {
        scalar_slots_with_tokens_and_end(ctx, &body, 3, &scalar::ScalarCache::default())
            .map(|result| result.map(|table| (table.slots, table.consumed)))
    })
    .expect("scalar slots are admitted")
    .expect("a complete three-slot table")
    .0;

    let magnitude = f64::from_be_bytes([0x3f, 0xe8, 1, 2, 3, 4, 5, 6]);
    assert_eq!(
        slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
        vec![Some(-magnitude), Some(-magnitude), Some(magnitude)]
    );
    assert_eq!(slot_equality(&slots[0], &slots[1]), Some(true));
    assert_eq!(slot_equality(&slots[1], &slots[2]), Some(false));
}

#[test]
fn a_surface_row_slot_table_states_no_slot_it_did_not_decode() {
    let cache = scalar::ScalarCache::default();
    // A byte the surface-row lane defines no scalar form for ends the table.
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            scalar_slots_with_tokens_and_end(ctx, &[0xe4, 0x01, 0xe4], 3, &cache)
                .map(|result| result.map(|table| (table.slots, table.consumed)))
        })
        .expect("scalar slots are admitted"),
        None
    );
    // A body that runs out before its declared count states fewer slots than
    // it declares.
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            scalar_slots_with_tokens_and_end(ctx, &[0xe4, 0x18], 3, &cache)
                .map(|result| result.map(|table| (table.slots, table.consumed)))
        })
        .expect("scalar slots are admitted"),
        None
    );
    // A complete table states every slot with the bytes it was decoded from,
    // and those bytes run from zero to the returned offset.
    let (slots, consumed) = crate::decode::with_test_decode_ctx(|ctx| {
        scalar_slots_with_tokens_and_end(ctx, &[0xe4, 0xe4, 0x18], 3, &cache)
            .map(|result| result.map(|table| (table.slots, table.consumed)))
    })
    .expect("scalar slots are admitted")
    .expect("a complete three-slot table");
    assert_eq!(consumed, 3);
    assert_eq!(
        slots.iter().map(|slot| slot.1.len()).sum::<usize>(),
        consumed
    );
    assert!(slots.iter().all(|slot| slot.0.is_some()));
}

#[test]
fn a_plane_envelope_slot_table_states_no_slot_it_did_not_decode() {
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            plane_envelope_scalar_slots_with_tokens_and_end(ctx, &[0x0e, 0x01, 0x0e], 3, &cache)
                .map(|result| result.map(|table| (table.slots, table.consumed)))
        })
        .expect("envelope slots are admitted"),
        None
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            plane_envelope_scalar_slots_with_tokens_and_end(ctx, &[0x0e, 0x18], 3, &cache)
                .map(|result| result.map(|table| (table.slots, table.consumed)))
        })
        .expect("envelope slots are admitted"),
        None
    );
    let (slots, consumed) = crate::decode::with_test_decode_ctx(|ctx| {
        plane_envelope_scalar_slots_with_tokens_and_end(ctx, &[0x0e, 0x0e, 0x18], 3, &cache)
            .map(|result| result.map(|table| (table.slots, table.consumed)))
    })
    .expect("envelope slots are admitted")
    .expect("a complete three-slot envelope table");
    assert_eq!(consumed, 3);
    assert_eq!(
        slots.iter().map(|slot| slot.1.len()).sum::<usize>(),
        consumed
    );
    assert!(slots.iter().all(|slot| slot.0.is_some()));
}

#[test]
fn spline_scalar_grid_parser_propagates_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let mut payload = b"srf_prim_ptr(spline)\0\xe0\x02i_points\0\xf9\x02\x03".to_vec();
    payload.extend([0x0f; 6]);
    let arena = DecodeArena::new();
    let error = crate::test_support::last_refusal_at(
        &payload,
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo named spline scalar slots",
        |ctx| {
            crate::surface::named_prototype_records(
                ctx,
                &payload,
                &mut crate::lane_refusal::LaneRefusals::new(),
            )
        },
    );
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo named spline scalar slots"
    ));

    let service = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &service)
        .expect("small prototype payload is admitted");
    let records = crate::surface::named_prototype_records(
        &ctx,
        &payload,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service profile admits six scalar slots");
    let SurfaceNamedValue::ScalarArray(array) = &records[0]
        .field("i_points")
        .expect("named scalar field")
        .value
    else {
        panic!("named scalar field must decode as a grid");
    };
    assert_eq!(array.values(), &[Some(0.0); 6]);
}

#[test]
fn fillet_vectors_use_the_signed_coordinate_dict_lane() {
    let negative = [0xc2, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
    let mut payload = b"srf_prim_ptr(fillet_srf)\0\xe0\x02i_pnts\0\xf9\x01\x03".to_vec();
    payload.extend_from_slice(&negative);
    payload.extend_from_slice(&[0xe4, 0x0f]);

    let records = named_prototype_records(&payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("i_pnts").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarArray({
            let mut array = crate::surface::arrays::DimensionedScalars::empty(1, 3)
                .expect("valid scalar array");
            crate::decode::with_test_decode_ctx(|ctx| {
                array.fill_tokens(
                    ctx,
                    vec![
                        (
                            Some(f64::from_be_bytes([
                                0xbf, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
                            ])),
                            negative.to_vec(),
                        ),
                        (Some(1.0), vec![0xe4]),
                        (Some(0.0), vec![0x0f]),
                    ],
                )
            })
            .expect("admitted scalar fill")
            .expect("matching scalar extent");
            array
        }))
    );
}

#[test]
fn fillet_vectors_dispatch_positive_coordinate_lanes_by_field() {
    let payload = b"srf_prim_ptr(fillet_srf)\0\
        \xe0\x02i_pnts\0\xf9\x01\x03\x98\x01\x02\x03\x04\x05\x06\xe4\xe4\
        \xe0\x02tangts\0\xf9\x01\x03\x4c\x01\x02\x03\x04\x05\x06\xe4\xe4";

    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());
    let prototype = &records[0];

    assert!(matches!(
        prototype.field("i_pnts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray(array))
            if array.values() == [
                Some(f64::from_be_bytes([0x40, 0x0d, 1, 2, 3, 4, 5, 6])),
                Some(1.0),
                Some(1.0),
            ]
    ));
    assert!(matches!(
        prototype.field("tangts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray(array))
            if array.values() == [
                Some(f64::from_be_bytes([0x3f, 1, 2, 3, 4, 5, 6, 0])),
                Some(1.0),
                Some(1.0),
            ]
    ));
}

#[test]
fn interpolation_point_dict_token_does_not_consume_following_world_coordinate() {
    let payload = b"srf_prim_ptr(fillet_srf)\0\
        \xe0\x02i_pnts\0\xf9\x01\x03\
        \x71\x01\x02\x03\x04\x05\x06\
        \x46\x40\x01\x02\x03\x04\x05\x06\xe4";

    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert!(matches!(
        records[0].field("i_pnts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray(array))
            if array.values() == [
                Some(f64::from_be_bytes([0x3f, 0xe6, 1, 2, 3, 4, 5, 6])),
                Some(f64::from_be_bytes([0x40, 0x40, 1, 2, 3, 4, 5, 6])),
                Some(1.0),
            ]
    ));
}

#[test]
fn dimensioned_vectors_own_header_shaped_scalar_payloads() {
    let payload = b"srf_prim_ptr(fillet_srf)\0\
        \xe0\x02i_pnts\0\xf9\x01\x03\
        \xaa\xe0\x01id\0\xe3\xe4\x0f\
        \xe0\x01tangts\0\xf9\x01\x03\xe4\xe4\xe4";

    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());
    let prototype = &records[0];

    assert_eq!(
        prototype.field("i_pnts").map(|field| field.body.as_slice()),
        Some(&[0xf9, 0x01, 0x03, 0xaa, 0xe0, 0x01, b'i', b'd', 0x00, 0xe3, 0xe4, 0x0f,][..])
    );
    assert!(prototype.field("id").is_none());
    assert!(matches!(
        prototype.field("tangts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray(array)) if array.dimensions() == 1 && array.count() == 3 && array.values() == [Some(1.0), Some(1.0), Some(1.0)]
    ));
}

#[test]
fn named_torus_radii_decode_compact_positive_quarters() {
    let payload = b"srf_prim_ptr(torus)\0\
        \xe0\x01radius1\0\x0e\
        \xe0\x01radius2\0\x0d\xf1\xf7\x0e\xe3";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("radius1").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarSequence(vec![0.5]))
    );
    assert_eq!(
        records[0].field("radius2").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarSequence(vec![0.25]))
    );
}

#[test]
fn named_prototype_radius_decodes_positive_eight_byte_form() {
    let value = 0.125_f64;
    let raw = value.to_be_bytes();
    assert_eq!(raw[0], 0x3f);
    let mut payload = b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0".to_vec();
    payload.push(0x28);
    payload.extend_from_slice(&raw[1..]);

    let records = named_prototype_records(&payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("radius").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarSequence(vec![value]))
    );
}

#[test]
fn named_prototype_radius_decodes_positive_dict_form() {
    let value = 4.5_f64;
    let raw = value.to_be_bytes();
    let prefix = u8::try_from(u16::from_be_bytes([raw[0], raw[1]]) - 0x3f75)
        .expect("synthetic value lies in the named-radius DICT lattice");
    let mut payload = b"srf_prim_ptr(cylinder)\0\xe0\x01radius\0".to_vec();
    payload.push(prefix);
    payload.extend_from_slice(&raw[2..]);

    let records = named_prototype_records(&payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("radius").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarSequence(vec![value]))
    );
}

#[test]
fn fillet_parameter_bounds_use_the_named_positive_dict_lane() {
    let upper = 4.5_f64;
    let raw = upper.to_be_bytes();
    let prefix = u8::try_from(u16::from_be_bytes([raw[0], raw[1]]) - 0x3f75)
        .expect("synthetic value lies in the named positive DICT lattice");
    let mut payload = b"srf_prim_ptr(fillet_srf)\0\
        \xe0\x01par_v_0\0\x18\
        \xe0\x01par_v_1\0"
        .to_vec();
    payload.push(prefix);
    payload.extend_from_slice(&raw[2..]);

    let records = named_prototype_records(&payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("par_v_0").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarSequence(vec![0.0]))
    );
    assert_eq!(
        records[0].field("par_v_1").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarSequence(vec![upper]))
    );
}

#[test]
fn fillet_parameter_bounds_do_not_use_the_radius_only_28_form() {
    let payload = b"srf_prim_ptr(fillet_srf)\0\
        \xe0\x01par_v_1\0\x28\x01\x02\x03\x04\x05\x06\x07";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("par_v_1").map(|field| &field.value),
        Some(&SurfaceNamedValue::Opaque(vec![0x28, 1, 2, 3, 4, 5, 6, 7]))
    );
}

#[test]
fn spline_metadata_decodes_wrapped_compact_values() {
    let payload = b"srf_prim_ptr(fillet_srf)\0\
        \xe0\x01flip\0\xf1\x01\
        \xe0\x01offset_type\0\x00\xf1\xf7\x0e\
        \xe0\x00frst_cntr_crv_hdr_ptr\0\x2f\
        \xe0\x01trv\0\x01\
        \xe0\x01tan_spline\0";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("flip").map(|field| &field.value),
        Some(&SurfaceNamedValue::CompactInt(1))
    );
    assert_eq!(
        records[0].field("offset_type").map(|field| &field.value),
        Some(&SurfaceNamedValue::CompactInt(0))
    );
    assert_eq!(
        records[0].field("tan_spline").map(|field| &field.value),
        Some(&SurfaceNamedValue::Empty)
    );
    assert_eq!(
        records[0]
            .field("frst_cntr_crv_hdr_ptr")
            .map(|field| &field.value),
        Some(&SurfaceNamedValue::CompactInt(47))
    );
    assert_eq!(
        records[0].field("trv").map(|field| &field.value),
        Some(&SurfaceNamedValue::CompactInt(1))
    );
}

#[test]
fn spline_metadata_rejects_malformed_compact_wrappers() {
    for (name, body) in [
        ("flip", &[0xf1][..]),
        ("flip", &[0xf1, 0x01, 0x00]),
        ("flip", &[0xf8, 0x01, 0x01]),
        ("offset_type", &[0x00, 0xf1, 0xf7]),
        ("offset_type", &[0x00, 0xf1, 0xf7, 0x00]),
        ("offset_type", &[0x00, 0xf1, 0xf7, 0x0e, 0x00]),
        ("offset_type", &[0xf8, 0x01, 0x00]),
    ] {
        assert_eq!(
            named_surface_value(
                &SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline),
                name,
                body,
                &scalar::ScalarCache::default(),
                &"prototype fixture",
                &mut crate::lane_refusal::LaneRefusals::new()
            ),
            SurfaceNamedValue::Opaque(body.to_vec())
        );
    }
}

#[test]
fn parent_feature_array_accepts_its_exact_reference_trailer() {
    let payload = b"srf_prim_ptr(plane)\0\xe0\0parent_feats\0\
        \xf8\x02\x07\x08\xf7\x03\x09\xe1\xf6\xf6";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());

    assert_eq!(
        records[0].field("parent_feats").map(|field| &field.value),
        Some(&SurfaceNamedValue::CompactIntArray(vec![7, 8]))
    );
}

#[test]
fn parent_feature_array_rejects_malformed_reference_trailers() {
    for trailer in [
        &[0xf7][..],
        &[0xf7, 0x00, 0x09],
        &[0xf7, 0x03, 0x00],
        &[0xf7, 0x03, 0x09, 0xf6],
        &[0xf7, 0x03, 0x09, 0xe1, 0xf6],
        &[0xf7, 0x03, 0x09, 0xe1, 0xf6, 0xf6, 0x00],
    ] {
        let mut body = vec![0xf8, 0x01, 0x07];
        body.extend_from_slice(trailer);
        assert_eq!(
            named_surface_value(
                &SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline),
                "parent_feats",
                &body,
                &scalar::ScalarCache::default(),
                &"prototype fixture",
                &mut crate::lane_refusal::LaneRefusals::new()
            ),
            SurfaceNamedValue::Opaque(body)
        );
    }
}

#[test]
fn a_compact_integer_array_holding_fewer_values_than_it_declares_is_not_an_array() {
    let cache = scalar::ScalarCache::default();
    let family = SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline);

    // `f8 02` declares two values and the body states two.
    assert_eq!(
        named_surface_value(
            &family,
            "dum_array",
            &[0xf8, 0x02, 0x07, 0x08],
            &cache,
            &"prototype fixture",
            &mut crate::lane_refusal::LaneRefusals::new()
        ),
        SurfaceNamedValue::CompactIntArray(vec![7, 8])
    );
    // `f8 03` declares three and the body still states two.
    assert_eq!(
        named_surface_value(
            &family,
            "dum_array",
            &[0xf8, 0x03, 0x07, 0x08],
            &cache,
            &"prototype fixture",
            &mut crate::lane_refusal::LaneRefusals::new()
        ),
        SurfaceNamedValue::Opaque(vec![0xf8, 0x03, 0x07, 0x08])
    );
}

#[test]
fn a_parent_feature_array_that_states_fewer_values_than_it_declares_states_no_trailer() {
    let cache = scalar::ScalarCache::default();
    let family = SurfacePrototypeFamily::Spline(crate::surface::SplineLabel::Spline);
    let trailer = [0xf7u8, 0x03, 0x09, 0xe1, 0xf6, 0xf6];

    let mut two = vec![0xf8u8, 0x02, 0x07, 0x08];
    two.extend_from_slice(&trailer);
    assert_eq!(
        named_surface_value(
            &family,
            "parent_feats",
            &two,
            &cache,
            &"prototype fixture",
            &mut crate::lane_refusal::LaneRefusals::new()
        ),
        SurfaceNamedValue::CompactIntArray(vec![7, 8])
    );

    // `compact_int` advances on every byte inside the body, so an array that
    // holds fewer values than it declares ran out of body: the cursor is on
    // the end and no bytes remain to state a trailer.
    let short = vec![0xf8u8, 0x03, 0x07, 0x08];
    assert_eq!(
        named_surface_value(
            &family,
            "parent_feats",
            &short,
            &cache,
            &"prototype fixture",
            &mut crate::lane_refusal::LaneRefusals::new()
        ),
        SurfaceNamedValue::Opaque(short.clone())
    );

    // A body that does hold three values reads the trailer's first byte as the
    // third, so the trailer no longer begins at the cursor.
    let mut three = vec![0xf8u8, 0x03, 0x07, 0x08];
    three.extend_from_slice(&trailer);
    assert_eq!(
        named_surface_value(
            &family,
            "parent_feats",
            &three,
            &cache,
            &"prototype fixture",
            &mut crate::lane_refusal::LaneRefusals::new()
        ),
        SurfaceNamedValue::Opaque(three.clone())
    );
}

/// A scalar body declaring more slots than the positional table is wide and
/// fewer value bytes than slots states more slots than its bytes can carry, so
/// the guard in `admitted_scalar_body` refuses it before the decode runs.
#[test]
fn a_surface_scalar_body_with_fewer_bytes_than_slots_above_twelve_is_refused() {
    let family = SurfacePrototypeFamily::Plane;
    let cache = scalar::ScalarCache::default();

    // `f9 0d 01`: thirteen dimensions, one entry, no value bytes.
    let body = [0xf9u8, 0x0d, 0x01];
    assert_eq!(
        named_surface_value(
            &family,
            "dum_array",
            &body,
            &cache,
            &"prototype fixture",
            &mut crate::lane_refusal::LaneRefusals::new()
        ),
        SurfaceNamedValue::Opaque(body.to_vec())
    );
}

#[test]
fn a_counted_parameter_body_whose_values_start_past_the_body_is_refused() {
    let body = [0xf8u8, 0x03, 0xe6];

    // One byte past the body states no value bytes at all. The record is
    // refused rather than read as "zero bytes remain".
    assert_eq!(
        admitted_counted_parameter_body(&body, body.len() + 1, 0),
        None
    );
    assert_eq!(admitted_counted_parameter_body(&body, usize::MAX, 0), None);

    // The values start on the last byte of the body, so one byte remains and
    // states at most three slots.
    assert_eq!(
        admitted_counted_parameter_body(&body, body.len() - 1, 3),
        Some(&body[body.len() - 1..])
    );
    assert_eq!(
        admitted_counted_parameter_body(&body, body.len() - 1, 4),
        None
    );
}

#[test]
fn a_counted_parameter_body_admits_three_slots_per_remaining_byte() {
    // `f8 09` declares nine slots; the three `e6` run tokens state three zero
    // slots each.
    const VALUES_START: usize = 2;
    const SLOTS: usize = 9;
    let body = [0xf8u8, 0x09, 0xe6, 0xe6, 0xe6];

    assert_eq!(body.len() - VALUES_START, SLOTS / 3);
    assert_eq!(
        admitted_counted_parameter_body(&body, VALUES_START, 9),
        Some(&body[VALUES_START..])
    );
    // One slot above the densest parse the walk can answer.
    assert_eq!(
        admitted_counted_parameter_body(&body, VALUES_START, 10),
        None
    );
    assert_eq!(
        admitted_counted_parameter_body(&body, VALUES_START, u32::MAX),
        None
    );

    // The bound is the density `counted_parameter_scalar_slots` reaches over
    // those same bytes.
    let cache = scalar::ScalarCache::default();
    let slots = counted_parameter_scalar_slots(&body[VALUES_START..], SLOTS, &cache);
    assert_eq!(
        slots.as_ref().map(std::vec::Vec::len),
        Some(SLOTS),
        "three run tokens state nine slots"
    );
    assert_eq!(
        counted_parameter_scalar_slots(&body[VALUES_START..], SLOTS + 1, &cache),
        None
    );
}

#[test]
fn withholds_ambiguous_outline_plane() {
    let records = [PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Compact {
            prefix: [None; 3],
            corners_3d: [
                [Some(3.0), Some(2.0), Some(4.0)],
                [Some(3.0), Some(2.0), Some(9.0)],
            ],
        },
        corner_coordinate_equal: [Some(true), Some(true), Some(false)],
        scalar_tokens: Vec::new(),
        row_offset: 10,
        offset: 20,
    }];
    assert!(outline_planes(&records).is_empty());
}

#[test]
fn torus_rows_keep_the_byte_after_a_seven_byte_coordinate() {
    let body = [0x2d, 0x1c, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf6];
    let cache = scalar::ScalarCache::default();

    assert_eq!(
        decode_row_scalar(SurfaceKind::TorusOrSphere, &body, 0, &cache),
        Some((-7.0, 7))
    );
    assert_eq!(body[7], 0xf6);
}

mod named_local_systems;



#[test]
fn placed_outline_support_index_preserves_duplicates_and_conflicts() {
    let record = PlaneEnvelopeRecord {
        surface_id: 42,
        body: Vec::new(),
        envelope: PlaneEnvelope::Standard {
            bounds_2d: [[None; 2]; 2],
            corners_3d: [
                [Some(-3.0), Some(-4.0), Some(7.0)],
                [Some(5.0), Some(-4.0), None],
            ],
        },
        corner_coordinate_equal: [Some(false), Some(true), None],
        scalar_tokens: Vec::new(),
        row_offset: 0,
        offset: 20,
    };
    let mut earlier = record.clone();
    earlier.offset = 5;
    let mut unframed = record.clone();
    unframed.surface_id = 84;
    unframed.offset = 10;
    unframed.corner_coordinate_equal[2] = Some(false);
    let records = [record, earlier, unframed];
    let frame = PlaneLocalSystem {
        surface_id: 42,
        body: Vec::new(),
        slots: [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0].map(Some),
        layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 0,
        offset: 0,
    };
    let mut invalid = frame.clone();
    invalid.slots = [None; 12];
    let frames = [frame.clone(), invalid, frame.clone()];
    let result = crate::decode::with_test_decode_ctx(|ctx| {
        crate::surface::placed_outline_planes(ctx, &records, &frames)
    })
    .expect("support index admitted");
    assert_eq!(
        result.iter().map(|plane| plane.offset).collect::<Vec<_>>(),
        [5, 10, 20]
    );
    assert_eq!(result[0].origin, [0.0, -4.0, 0.0]);
    assert_eq!(result[0].normal(), [0.0, 1.0, 0.0]);
    assert_eq!(result[0].u_axis(), [0.0, 0.0, 1.0]);
    assert_eq!(result[1].surface_id, 84);
    let mut conflicting = frame.clone();
    conflicting.slots[..3].copy_from_slice(&[Some(1.0), Some(0.0), Some(0.0)]);
    let frames = [frame.clone(), conflicting, frame];
    let result = crate::decode::with_test_decode_ctx(|ctx| {
        crate::surface::placed_outline_planes(ctx, &records, &frames)
    })
    .expect("conflicting support index admitted");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].surface_id, 84);
}
