// SPDX-License-Identifier: Apache-2.0
use super::{service_scalar_tokens, service_scalar_frames, service_opaque_spans};
use super::super::parameter_records;
use crate::surface::SurfaceParameterOpaqueSpan;
use crate::scalar;
use crate::surface::{OutlinePlane, SurfaceBodyBoundary, SurfaceKind, SurfaceParameterRecord, SurfaceParameterScalar, SurfaceParameterScalarFrame, SurfaceRow};

fn positional_frame_planes(
    parameters: &[SurfaceParameterRecord],
    rows: &[SurfaceRow],
) -> Vec<OutlinePlane> {
    let parameters = crate::surface::SurfaceParameters::from_rows(parameters.to_vec());
    let rows = crate::surface::SurfaceRows::from_rows(rows.to_vec());
    super::super::with_decode_ctx(&[], |ctx| {
        crate::surface::positional_frame_planes(ctx, &parameters, &rows)
    })
}

fn unique_positional_frame_fixture() -> (SurfaceParameterRecord, SurfaceRow) {
    let slot = |value, offset| SurfaceParameterScalar {
        value: Some(value),
        raw: vec![u8::try_from(offset).expect("fixture value fits u8")],
        offset,
    };
    let record = SurfaceParameterRecord {
        surface_id: 41,
        body: vec![0x00, 0x0c, 0x9a],
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: vec![SurfaceParameterScalarFrame {
            offset: 3,
            slots: [8.0, 2.0, -3.0, 8.0, 5.0, 4.0]
                .into_iter()
                .enumerate()
                .map(|(offset, value)| slot(value, offset))
                .collect(),
        }],
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Plane,
        ),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 3,
        body_offset: 11,
    };
    let row = SurfaceRow {
        id: 41,
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 3,
    };
    (record, row)
}

#[test]
fn derives_plane_from_unique_six_scalar_positional_frame() {
    let (record, row) = unique_positional_frame_fixture();

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [8.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 14,
        }]
    );

    let mut unmarked = record.clone();
    unmarked.body[2] = 0x99;
    assert!(positional_frame_planes(&[unmarked], std::slice::from_ref(&row)).is_empty());

    let mut ambiguous = record;
    ambiguous.scalar_frames[0].slots[4].value = Some(2.0);
    assert!(positional_frame_planes(&[ambiguous], &[row]).is_empty());
}

fn positional_frame_limit_error(limit: u64) -> cadmpeg_core::CodecError {
    let (record, row) = unique_positional_frame_fixture();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    crate::surface::positional_frame_planes(
        &ctx,
        &crate::surface::SurfaceParameters::from_rows(vec![record]),
        &crate::surface::SurfaceRows::from_rows(vec![row]),
    )
    .expect_err("positional plane collection exceeds limit")
}



#[test]
fn positional_frame_refuses_output_vector() {
    let error = positional_frame_limit_error(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::CollectionItems, Some("creo positional frame planes"), |cap| Err::<(), _>(positional_frame_limit_error(cap))));
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo positional frame planes")
    );
}

#[test]
fn derives_plane_from_auxiliary_corner_frame() {
    let slot = |value, offset, length| SurfaceParameterScalar {
        value: Some(value),
        raw: vec![0; length],
        offset,
    };
    let record = SurfaceParameterRecord {
        surface_id: 41,
        body: vec![0; 49],
        scalar_tokens: Vec::new(),
        opaque_spans: vec![
            SurfaceParameterOpaqueSpan {
                raw: vec![0; 3],
                offset: 0,
            },
            SurfaceParameterOpaqueSpan {
                raw: vec![0; 8],
                offset: 10,
            },
        ],
        scalar_frames: vec![
            SurfaceParameterScalarFrame {
                offset: 3,
                slots: vec![slot(0.86, 3, 7)],
            },
            SurfaceParameterScalarFrame {
                offset: 18,
                slots: vec![
                    slot(0.8, 18, 3),
                    slot(42.3, 21, 8),
                    slot(1.75, 29, 3),
                    slot(-0.3, 32, 3),
                    slot(37.6, 35, 8),
                    slot(1.75, 43, 3),
                    slot(0.3, 46, 3),
                ],
            },
        ],
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Plane,
        ),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 3,
        body_offset: 11,
    };
    let row = SurfaceRow {
        id: 41,
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 1.75, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 32,
        }]
    );

    let mut trailed = record.clone();
    trailed.body = vec![0; 65];
    trailed.body[63..].copy_from_slice(&[0xf7, 0x0c]);
    trailed.opaque_spans = vec![
        SurfaceParameterOpaqueSpan {
            raw: vec![0],
            offset: 0,
        },
        SurfaceParameterOpaqueSpan {
            raw: vec![0; 4],
            offset: 11,
        },
        SurfaceParameterOpaqueSpan {
            raw: vec![0; 2],
            offset: 16,
        },
        SurfaceParameterOpaqueSpan {
            raw: vec![0xf7, 0x0c],
            offset: 63,
        },
    ];
    trailed.scalar_frames = vec![
        SurfaceParameterScalarFrame {
            offset: 1,
            slots: vec![slot(0.001, 1, 7), slot(0.2, 8, 3)],
        },
        SurfaceParameterScalarFrame {
            offset: 15,
            slots: vec![slot(-1.0, 15, 1)],
        },
        SurfaceParameterScalarFrame {
            offset: 18,
            slots: vec![
                slot(-59.8, 18, 8),
                slot(-29.8, 26, 3),
                slot(4.1, 29, 7),
                slot(7.5, 36, 8),
                slot(29.8, 44, 3),
                slot(3.9, 47, 8),
                slot(7.5, 55, 8),
            ],
        },
    ];
    let mut domain_prefixed = trailed.clone();
    domain_prefixed.scalar_frames.remove(1);
    domain_prefixed.scalar_frames[1].offset = 15;
    domain_prefixed.scalar_frames[1].slots.splice(
        0..0,
        [slot(-2.0, 15, 1), slot(2.0, 16, 1), slot(0.0, 17, 1)],
    );
    assert_eq!(
        positional_frame_planes(&[trailed], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 0.0, 7.5],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 37,
        }]
    );
    assert_eq!(
        positional_frame_planes(&[domain_prefixed], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 0.0, 7.5],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 37,
        }]
    );

    let mut compact_prefix = record.clone();
    compact_prefix.body = vec![
        0x18, 0x18, 0x6d, 0xeb, 0x81, 0x84, 0xcc, 0xcc, 0xd0, 0x00, 0x0c, 0x9a, 0xd5, 0xd6, 0x25,
        0xa6, 0xec, 0x06, 0x18, 0x46, 0x1a, 0xdf, 0x09, 0x9b, 0x3c, 0x32, 0xed, 0x2f, 0x20, 0x00,
        0xd5, 0xd6, 0x25, 0xa6, 0xec, 0x06, 0x18, 0x46, 0x18, 0x81, 0x99, 0x6a, 0xa2, 0x99, 0x53,
        0x2e, 0x20, 0x33, 0xf7, 0x0c,
    ];
    compact_prefix.scalar_tokens = service_scalar_tokens(
        SurfaceKind::Plane,
        &compact_prefix.body,
        &scalar::ScalarCache::default(),
    );
    compact_prefix.scalar_frames = service_scalar_frames(&compact_prefix.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[compact_prefix], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [2.479_564_003_064_99, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 23,
        }]
    );

    let mut incomplete = record;
    incomplete.opaque_spans[1].raw.truncate(7);
    assert!(positional_frame_planes(&[incomplete.clone()], std::slice::from_ref(&row)).is_empty());

    let mut short = incomplete;
    short.scalar_frames.push(SurfaceParameterScalarFrame {
        offset: 18,
        slots: vec![slot(1.0, 18, 1)],
    });
    assert!(positional_frame_planes(&[short], &[row]).is_empty());
}

#[test]
fn derives_plane_from_terminal_corner_frame() {
    let body = [
        0x37, 0x01, 0x5f, 0xff, 0xff, 0xff, 0xff, 0xf4, 0x2d, 0x4c, 0x75, 0xdb, 0x19, 0xc2, 0x89,
        0x40, 0x2e, 0x17, 0xff, 0x2d, 0x4f, 0x01, 0x49, 0xdf, 0x84, 0xdb, 0x18, 0x48, 0x57, 0x00,
        0x2d, 0x57, 0xd0, 0x03, 0xc5, 0xbc, 0xeb, 0x74, 0xda, 0x00, 0x00, 0x00, 0x00, 0x00, 0x11,
        0x47, 0x56, 0xff, 0x2d, 0x59, 0x15, 0xbb, 0x28, 0x9e, 0x14, 0x60, 0x2e, 0x07, 0xff, 0xf7,
        0x1f,
    ];
    let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let record = parameter_records(&payload).remove(0);
    let row = SurfaceRow {
        id: record.surface_id,
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: record.surface_id,
            origin: [-92.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: record.body_offset + 27,
        }]
    );

    let unprefixed_body = [
        0x48, 0x67, 0xd0, 0x46, 0x49, 0x43, 0xd8, 0x44, 0x0e, 0x17, 0x8e, 0x2f, 0x61, 0x90, 0x2d,
        0x49, 0x43, 0xd8, 0x44, 0x0e, 0x17, 0x90, 0x48, 0x67, 0xd0, 0x48, 0x14, 0x00, 0x46, 0x49,
        0x43, 0xd8, 0x44, 0x0e, 0x17, 0x8e, 0x2f, 0x61, 0x90, 0x48, 0x14, 0x00, 0x2d, 0x49, 0x43,
        0xd8, 0x44, 0x0e, 0x17, 0x90, 0xf7, 0x1f,
    ];
    let mut unprefixed_payload = vec![7, 0x22, 4, 0x01, 0, 0];
    unprefixed_payload.extend_from_slice(&unprefixed_body);
    unprefixed_payload.push(0xe3);
    let unprefixed = parameter_records(&unprefixed_payload).remove(0);
    assert_eq!(
        positional_frame_planes(
            std::slice::from_ref(&unprefixed),
            std::slice::from_ref(&row)
        ),
        vec![OutlinePlane {
            surface_id: unprefixed.surface_id,
            origin: [0.0, -5.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: unprefixed.body_offset + 22,
        }]
    );

    let mut wrong_trailer = record.clone();
    *wrong_trailer
        .body
        .last_mut()
        .expect("terminal reference id") = 0x1e;
    assert!(positional_frame_planes(&[wrong_trailer], std::slice::from_ref(&row)).is_empty());

    let mut multiple_frames = record.clone();
    multiple_frames.scalar_frames.insert(
        0,
        SurfaceParameterScalarFrame {
            offset: 0,
            slots: vec![multiple_frames.scalar_frames[0].slots[0].clone()],
        },
    );
    assert!(positional_frame_planes(&[multiple_frames], std::slice::from_ref(&row)).is_empty());

    let mut ambiguous = record;
    ambiguous.scalar_frames[0].slots[7].value = Some(-95.250_230_249_874_05);
    assert!(positional_frame_planes(&[ambiguous], &[row]).is_empty());
}

#[test]
fn derives_plane_from_split_terminal_corner_frame() {
    let body = [
        0x32, 0xf7, 0xf0, 0x6c, 0x6b, 0x2d, 0x51, 0x9a, 0x2d, 0x42, 0x50, 0x4a, 0x32, 0x0f, 0x60,
        0x20, 0x2e, 0x4e, 0xff, 0x2d, 0x4e, 0x4f, 0x19, 0xda, 0x50, 0x97, 0xe8, 0x46, 0x64, 0x1f,
        0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x52, 0xbd, 0x3b, 0x51, 0xe3, 0x56, 0xe4, 0x2f, 0x1c,
        0x00, 0x46, 0x58, 0xbf, 0xff, 0xff, 0xff, 0xff, 0xf8, 0x2d, 0x58, 0xbc, 0xa3, 0x26, 0x03,
        0xf2, 0xc8, 0x2f, 0x1c, 0x00, 0xf7, 0x1f,
    ];
    let mut payload = vec![7, 0x22, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let record = parameter_records(&payload).remove(0);
    let row = SurfaceRow {
        id: record.surface_id,
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: record.surface_id,
            origin: [0.0, 0.0, 7.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: record.body_offset + 27,
        }]
    );

    let mut incomplete_controls = record.clone();
    incomplete_controls.opaque_spans[1].raw.pop();
    assert!(positional_frame_planes(&[incomplete_controls], std::slice::from_ref(&row)).is_empty());

    let mut ambiguous = record;
    ambiguous.scalar_frames[1].slots[5].value = ambiguous.scalar_frames[1].slots[2].value;
    assert!(positional_frame_planes(&[ambiguous], &[row]).is_empty());
}

#[test]
fn derives_plane_from_marker_bounded_corner_frames() {
    let body = vec![
        0x18, 0xe4, 0x28, 0xad, 0xfb, 0xcd, 0xe8, 0xf5, 0xc2, 0x80, 0x00, 0x0c, 0x9a, 0xdc, 0x9c,
        0x95, 0x35, 0x00, 0x80, 0xf8, 0x46, 0x1a, 0xdf, 0x09, 0x9b, 0x3c, 0x32, 0xed, 0x2f, 0x20,
        0x00, 0xdc, 0x9c, 0x95, 0x35, 0x00, 0x80, 0xf8, 0x46, 0x1a, 0xa3, 0x11, 0xff, 0x6a, 0x47,
        0x68, 0x2e, 0x20, 0x33, 0xf7, 0x0c,
    ];
    let tokens = service_scalar_tokens(SurfaceKind::Plane, &body, &scalar::ScalarCache::default());
    let frames = service_scalar_frames(&tokens);
    let record = SurfaceParameterRecord {
        surface_id: 41,
        opaque_spans: service_opaque_spans(&body, &tokens),
        scalar_tokens: tokens,
        scalar_frames: frames,
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Plane,
        ),
        body,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 3,
        body_offset: 11,
    };
    let row = SurfaceRow {
        id: 41,
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        record
            .scalar_frames
            .last()
            .expect("reflected corner frame")
            .slots
            .len(),
        6
    );
    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [3.326_456_464_841_722_7, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 24,
        }]
    );

    let mut prefixed_eight_byte = record.clone();
    prefixed_eight_byte.body = vec![
        0x18, 0xe4, 0x28, 0xc6, 0xc6, 0xa8, 0x58, 0x51, 0xeb, 0xa0, 0x00, 0x0c, 0x9a, 0x46, 0x1e,
        0x3e, 0x61, 0xf5, 0x38, 0x92, 0x68, 0x46, 0x19, 0xb0, 0xe5, 0x1d, 0x83, 0xe1, 0x02, 0x2f,
        0x20, 0x00, 0x46, 0x1e, 0x3e, 0x61, 0xf5, 0x38, 0x92, 0x68, 0x46, 0x18, 0xfa, 0xaf, 0xda,
        0xc1, 0x51, 0xa5, 0x2e, 0x20, 0x33,
    ];
    prefixed_eight_byte.scalar_tokens = service_scalar_tokens(
        SurfaceKind::Plane,
        &prefixed_eight_byte.body,
        &scalar::ScalarCache::default(),
    );
    prefixed_eight_byte.scalar_frames = service_scalar_frames(&prefixed_eight_byte.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[prefixed_eight_byte], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [7.560_920_554_712_176, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 24,
        }]
    );

    let mut prefixed_seven_byte = record.clone();
    prefixed_seven_byte.body = vec![
        0x18, 0xe4, 0x28, 0xc6, 0xc6, 0xa8, 0x58, 0x51, 0xeb, 0xa0, 0x00, 0x0c, 0x9a, 0x4a, 0x19,
        0x29, 0x8e, 0x22, 0xd2, 0x2c, 0x46, 0x19, 0xb0, 0xe5, 0x1d, 0x83, 0xe1, 0x02, 0x2f, 0x20,
        0x00, 0x4a, 0x19, 0x29, 0x8e, 0x22, 0xd2, 0x2c, 0x46, 0x18, 0xfa, 0xaf, 0xda, 0xc1, 0x51,
        0xa5, 0x2e, 0x20, 0x33,
    ];
    prefixed_seven_byte.scalar_tokens = service_scalar_tokens(
        SurfaceKind::Plane,
        &prefixed_seven_byte.body,
        &scalar::ScalarCache::default(),
    );
    prefixed_seven_byte.scalar_frames = service_scalar_frames(&prefixed_seven_byte.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[prefixed_seven_byte], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [6.290_581_268_384_813, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 24,
        }]
    );

    let mut unterminated = record.clone();
    unterminated.body.truncate(unterminated.body.len() - 2);
    unterminated.scalar_tokens = service_scalar_tokens(
        SurfaceKind::Plane,
        &unterminated.body,
        &scalar::ScalarCache::default(),
    );
    unterminated.scalar_frames = service_scalar_frames(&unterminated.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[unterminated], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [3.326_456_464_841_722_7, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 24,
        }]
    );

    let mut y_held = record.clone();
    y_held.body = vec![
        0x18, 0xe4, 0x2c, 0xbe, 0x45, 0xa8, 0x7a, 0xe1, 0x48, 0x00, 0x0c, 0x9a, 0xd1, 0xf1, 0x60,
        0x5a, 0xa4, 0xd9, 0x00, 0x46, 0x1b, 0x1c, 0x28, 0x70, 0x5d, 0x7a, 0x9b, 0x2f, 0x20, 0x00,
        0xd0, 0x0d, 0x05, 0xd2, 0xf6, 0xc4, 0x80, 0x46, 0x1b, 0x1c, 0x28, 0x70, 0x5d, 0x7a, 0x9b,
        0x2e, 0x20, 0x33, 0xf7, 0x0c,
    ];
    y_held.scalar_tokens = service_scalar_tokens(
        SurfaceKind::Plane,
        &y_held.body,
        &scalar::ScalarCache::default(),
    );
    y_held.scalar_frames = service_scalar_frames(&y_held.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[y_held], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 6.777_498_012_261_868, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 23,
        }]
    );

    let mut mixed_width = record.clone();
    mixed_width.body = vec![
        0x18, 0xe4, 0x2c, 0xbe, 0x45, 0x9b, 0x33, 0x33, 0x33, 0x00, 0x0c, 0x9a, 0x4a, 0x19, 0x29,
        0x8e, 0x22, 0xd2, 0x2c, 0x46, 0x1a, 0x29, 0xfb, 0x8f, 0x4b, 0x8f, 0x16, 0x2f, 0x20, 0x00,
        0x46, 0x18, 0xb0, 0x77, 0xb6, 0x05, 0x5f, 0x34, 0x46, 0x1a, 0x29, 0xfb, 0x8f, 0x4b, 0x8f,
        0x16, 0x2e, 0x20, 0x33,
    ];
    mixed_width.scalar_tokens = service_scalar_tokens(
        SurfaceKind::Plane,
        &mixed_width.body,
        &scalar::ScalarCache::default(),
    );
    mixed_width.scalar_frames = service_scalar_frames(&mixed_width.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[mixed_width], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 6.540_998_686_777_831, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 23,
        }]
    );

    let mut malformed = record;
    malformed.body[31] = 0x00;
    let tokens = service_scalar_tokens(
        SurfaceKind::Plane,
        &malformed.body,
        &scalar::ScalarCache::default(),
    );
    assert!(tokens.iter().all(|token| token.offset != 13));
    malformed.scalar_frames = service_scalar_frames(&tokens);
    assert!(positional_frame_planes(&[malformed], &[row]).is_empty());
}
