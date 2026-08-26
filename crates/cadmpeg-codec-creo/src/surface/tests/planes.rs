// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::*;

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
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 20,
        }]
    );
}

#[test]
fn derives_plane_from_unique_six_scalar_positional_frame() {
    let slot = |value, offset| SurfaceParameterScalar {
        value: Some(value),
        raw: vec![offset as u8],
        offset,
        length: 1,
    };
    let record = SurfaceParameterRecord {
        surface_id: 41,
        body: vec![0x00, 0x0c, 0x9a],
        scalar_values: Vec::new(),
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
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        positional_torus_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 3,
        body_offset: 11,
    };
    let row = SurfaceRow {
        id: 41,
        type_byte: SurfaceKind::Plane.canonical_type_byte(),
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [8.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
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

#[test]
fn derives_plane_from_auxiliary_corner_frame() {
    let slot = |value, offset, length| SurfaceParameterScalar {
        value: Some(value),
        raw: vec![0; length],
        offset,
        length,
    };
    let record = SurfaceParameterRecord {
        surface_id: 41,
        body: vec![0; 49],
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: vec![
            SurfaceParameterOpaqueSpan {
                raw: vec![0; 3],
                offset: 0,
                length: 3,
            },
            SurfaceParameterOpaqueSpan {
                raw: vec![0; 8],
                offset: 10,
                length: 8,
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
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        positional_torus_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 3,
        body_offset: 11,
    };
    let row = SurfaceRow {
        id: 41,
        type_byte: SurfaceKind::Plane.canonical_type_byte(),
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 1.75, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
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
            length: 1,
        },
        SurfaceParameterOpaqueSpan {
            raw: vec![0; 4],
            offset: 11,
            length: 4,
        },
        SurfaceParameterOpaqueSpan {
            raw: vec![0; 2],
            offset: 16,
            length: 2,
        },
        SurfaceParameterOpaqueSpan {
            raw: vec![0xf7, 0x0c],
            offset: 63,
            length: 2,
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
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 37,
        }]
    );
    assert_eq!(
        positional_frame_planes(&[domain_prefixed], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 0.0, 7.5],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
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
    compact_prefix.scalar_tokens = scalar_tokens(
        SurfaceKind::Plane,
        &compact_prefix.body,
        &scalar::ScalarCache::default(),
    );
    compact_prefix.scalar_frames = scalar_frames(&compact_prefix.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[compact_prefix], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [2.479_564_003_064_99, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 23,
        }]
    );

    let mut incomplete = record;
    incomplete.opaque_spans[1].length = 7;
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
        type_byte: SurfaceKind::Plane.canonical_type_byte(),
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: record.surface_id,
            origin: [-92.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
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
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
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
        type_byte: SurfaceKind::Plane.canonical_type_byte(),
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 3,
    };

    assert_eq!(
        positional_frame_planes(std::slice::from_ref(&record), std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: record.surface_id,
            origin: [0.0, 0.0, 7.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: record.body_offset + 27,
        }]
    );

    let mut incomplete_controls = record.clone();
    incomplete_controls.opaque_spans[1].length -= 1;
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
    let tokens = scalar_tokens(SurfaceKind::Plane, &body, &scalar::ScalarCache::default());
    let frames = scalar_frames(&tokens);
    let record = SurfaceParameterRecord {
        surface_id: 41,
        scalar_values: tokens.iter().filter_map(|token| token.value).collect(),
        opaque_spans: opaque_spans(&body, &tokens),
        terminal_scalar_frame: terminal_scalar_frame(&body, &frames),
        scalar_tokens: tokens,
        scalar_frames: frames,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        body,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 3,
        body_offset: 11,
    };
    let row = SurfaceRow {
        id: 41,
        type_byte: SurfaceKind::Plane.canonical_type_byte(),
        kind: SurfaceKind::Plane,
        feature_id: 17,
        reversed: false,
        boundary_type: 0,
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
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
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
    prefixed_eight_byte.scalar_tokens = scalar_tokens(
        SurfaceKind::Plane,
        &prefixed_eight_byte.body,
        &scalar::ScalarCache::default(),
    );
    prefixed_eight_byte.scalar_frames = scalar_frames(&prefixed_eight_byte.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[prefixed_eight_byte], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [7.560_920_554_712_176, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
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
    prefixed_seven_byte.scalar_tokens = scalar_tokens(
        SurfaceKind::Plane,
        &prefixed_seven_byte.body,
        &scalar::ScalarCache::default(),
    );
    prefixed_seven_byte.scalar_frames = scalar_frames(&prefixed_seven_byte.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[prefixed_seven_byte], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [6.290_581_268_384_813, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 24,
        }]
    );

    let mut unterminated = record.clone();
    unterminated.body.truncate(unterminated.body.len() - 2);
    unterminated.scalar_tokens = scalar_tokens(
        SurfaceKind::Plane,
        &unterminated.body,
        &scalar::ScalarCache::default(),
    );
    unterminated.scalar_frames = scalar_frames(&unterminated.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[unterminated], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [3.326_456_464_841_722_7, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
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
    y_held.scalar_tokens = scalar_tokens(
        SurfaceKind::Plane,
        &y_held.body,
        &scalar::ScalarCache::default(),
    );
    y_held.scalar_frames = scalar_frames(&y_held.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[y_held], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 6.777_498_012_261_868, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
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
    mixed_width.scalar_tokens = scalar_tokens(
        SurfaceKind::Plane,
        &mixed_width.body,
        &scalar::ScalarCache::default(),
    );
    mixed_width.scalar_frames = scalar_frames(&mixed_width.scalar_tokens);
    assert_eq!(
        positional_frame_planes(&[mixed_width], std::slice::from_ref(&row)),
        vec![OutlinePlane {
            surface_id: 41,
            origin: [0.0, 6.540_998_686_777_831, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 23,
        }]
    );

    let mut malformed = record;
    malformed.body[31] = 0x00;
    let tokens = scalar_tokens(
        SurfaceKind::Plane,
        &malformed.body,
        &scalar::ScalarCache::default(),
    );
    assert!(tokens.iter().all(|token| token.offset != 13));
    malformed.scalar_frames = scalar_frames(&tokens);
    assert!(positional_frame_planes(&[malformed], &[row]).is_empty());
}

#[test]
fn compact_plane_scalar_suffix_requires_one_complete_nine_slot_frame() {
    let body = [
        0x32, 0xbe, 0xe4, 0xe4, 0xe4, 0x0d, 0x0f, 0xe4, 0x0d, 0xe4, 0x0f,
    ];
    let slots = complete_plane_compact_scalar_suffix(&body, &scalar::ScalarCache::default())
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
    assert!(
        complete_plane_compact_scalar_suffix(&body[2..], &scalar::ScalarCache::default()).is_none()
    );
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
    let (slots, consumed) =
        plane_envelope_scalar_slots_with_tokens_and_end(&body, 10, &scalar::ScalarCache::default());

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
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
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
    assert_eq!(outline_planes(&records)[0].normal, [0.0, 1.0, 0.0]);
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
        slots: Vec::new(),
        origin: Some([100.0, 200.0, 300.0]),
        u_axis: Some([0.0, 0.0, 1.0]),
        normal: Some([0.0, 1.0, 0.0]),
        classification: LocalSystemClassification::Unclassified,
        row_offset: 10,
        offset: 30,
    }];

    assert_eq!(
        frame_bound_outline_planes(&records, &frames),
        [OutlinePlane {
            surface_id: 42,
            origin: [0.0, -4.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [0.0, 0.0, 1.0],
            offset: 20,
        }]
    );

    let agreeing_frames = [frames[0].clone(), frames[0].clone()];
    assert_eq!(
        frame_bound_outline_planes(&records, &agreeing_frames),
        frame_bound_outline_planes(&records, &frames)
    );
    let mut conflicting = frames[0].clone();
    conflicting.normal = Some([1.0, 0.0, 0.0]);
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
        slots: Vec::new(),
        origin: Some([100.0, 200.0, 300.0]),
        u_axis: Some([0.0, 0.0, 1.0]),
        normal: Some([0.0, 1.0, 0.0]),
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
    assert_eq!(frame.u_axis, Some([0.0, 1.0, 0.0]));
    assert_eq!(frame.normal, Some([1.0, 0.0, 0.0]));
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
    assert_eq!(frame.u_axis, Some([0.6, 0.0, 0.8]));
    assert_eq!(frame.normal, Some([0.8, 0.0, -0.6]));
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
    assert_eq!(first_compound_close(&payload, 0, payload.len()), Some(4));
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

    assert_eq!(first_compound_close(&payload, 0, payload.len()), None);
    assert_eq!(
        plane_local_system_compound_close(&payload, 0, payload.len(), &cache),
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
    assert_eq!(frame.u_axis, Some([0.0, 1.0, 0.0]));
    assert_eq!(frame.normal, Some([0.0, 0.0, -1.0]));
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
    assert_eq!(systems[0].normal, Some([0.0, 0.0, -1.0]));
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
        .normal,
        Some([0.0, 0.0, 1.0])
    );
    let first_rank_zero = plane_frame(&options([
        0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -3.0, 0.0,
    ]));
    assert_eq!(first_rank_zero.origin, Some([0.0, -3.0, 0.0]));
    assert_eq!(first_rank_zero.u_axis, Some([1.0, 0.0, 0.0]));
    assert_eq!(first_rank_zero.normal, Some([0.0, -1.0, 0.0]));
    assert_eq!(
        plane_frame(&options([
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0,
        ]))
        .normal,
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
fn matrix_plane_frame_uses_stored_direction_and_normal_columns() {
    let slots = [
        1.0, 0.0, 0.0, // x components of the three columns
        0.0, 0.0, 0.0, // zero-rank column
        0.0, 0.0, 1.0, // z components of the three columns
        2.0, 3.0, 4.0,
    ];

    let frame = plane_matrix_frame(&slots.map(Some));
    assert_eq!(frame.origin, Some([2.0, 3.0, 4.0]));
    assert_eq!(frame.u_axis, Some([1.0, 0.0, 0.0]));
    assert_eq!(frame.normal, Some([0.0, 0.0, 1.0]));
}

#[test]
fn signed_surface_dict_slots_decode_as_mirrors() {
    let body = [
        0xbb, 1, 2, 3, 4, 5, 6, 0xbb, 1, 2, 3, 4, 5, 6, 0x73, 1, 2, 3, 4, 5, 6,
    ];
    let slots = scalar_slots_with_tokens_and_end(&body, 3, &scalar::ScalarCache::default()).0;

    let magnitude = f64::from_be_bytes([0x3f, 0xe8, 1, 2, 3, 4, 5, 6]);
    assert_eq!(
        slots.iter().map(|slot| slot.0).collect::<Vec<_>>(),
        vec![Some(-magnitude), Some(-magnitude), Some(magnitude)]
    );
    assert_eq!(slot_equality(&slots[0], &slots[1]), Some(true));
    assert_eq!(slot_equality(&slots[1], &slots[2]), Some(false));
}

#[test]
fn terminal_positional_slot_zero_occupies_one_byte() {
    let slots =
        scalar_slots_with_tokens_and_end(&[0xe4, 0x18], 2, &scalar::ScalarCache::default()).0;

    assert_eq!(slots, [(Some(1.0), vec![0xe4]), (Some(0.0), vec![0x18])]);
}

#[test]
fn named_local_system_expands_row_lane_zero_forms() {
    let body = [
        0xf9, 0x04, 0x03, 0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4, 0x43, 0xe0, 0x00,
        0x18, 0xe4,
    ];

    assert_eq!(
        named_surface_value(
            &SurfacePrototypeFamily::Plane,
            "local_sys",
            &body,
            &scalar::ScalarCache::default(),
        ),
        SurfaceNamedValue::ScalarArray {
            dimensions: 4,
            count: 3,
            values: vec![
                Some(0.0),
                Some(1.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(1.0),
                Some(-0.5),
                Some(0.0),
                Some(1.0),
            ],
            tokens: Vec::new(),
        }
    );
}

#[test]
fn named_local_system_splits_zero_before_coordinate_token() {
    let body = [
        0x41, 0xd2, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2, 0x79, 0xac, 0x53, 0x1a, 0x28, 0x66, 0x9d,
        0x18, 0x79, 0xac, 0x53, 0x1a, 0x28, 0x66, 0x9d, 0x5d, 0x3c, 0xfc, 0xe9, 0x9e, 0x37, 0xb2,
        0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f,
    ];

    let slots = sequential_named_local_system_slots(&body, 12, &scalar::ScalarCache::default())
        .expect("complete local system");

    assert_eq!(slots[2], Some(0.0));
    assert_eq!(slots[3], slots[1]);
    assert_eq!(slots[4], slots[0].map(|value| -value));
    assert_eq!(slots[5..], [Some(0.0); 7]);
}

#[test]
fn named_local_system_decodes_terminal_zero_slot() {
    let payload = b"srf_prim_ptr(cylinder)\0\xe0\x02local_sys\0\xf9\x04\x03\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18\xe0\x01radius\0\xe4";
    let records = named_prototype_records(payload);

    assert_eq!(
        records[0].field("local_sys").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarArray {
            dimensions: 4,
            count: 3,
            values: vec![
                Some(0.0),
                Some(1.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(1.0),
                Some(0.0),
                Some(0.0),
                Some(0.0),
                Some(15.0),
                Some(0.0),
            ],
            tokens: Vec::new(),
        })
    );
}

#[test]
fn named_local_system_advances_across_inherited_slots() {
    let body = [
        0xe4, 0x0f, 0xe7, 0x03, 0xe4, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f,
    ];

    assert_eq!(
        sequential_named_local_system_slots(&body, 12, &scalar::ScalarCache::default()),
        Some(vec![
            Some(1.0),
            Some(0.0),
            None,
            None,
            None,
            Some(1.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
            Some(0.0),
        ])
    );
}

#[test]
fn named_local_system_rejects_invalid_inherited_slot_transitions() {
    for body in [
        &[0xe7][..],
        &[0xe7, 0x00],
        &[0xe7, 0x0d],
        &[0xe4, 0xe7, 0x0c],
    ] {
        assert_eq!(
            sequential_named_local_system_slots(body, 12, &scalar::ScalarCache::default()),
            None
        );
    }
}

#[test]
fn named_local_system_rejects_an_unknown_byte_before_complete_slots() {
    let payload = b"srf_prim_ptr(cylinder)\0\
        \xe0\x02local_sys\0\xf9\x04\x03\xfb\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18\
        \xe0\x01radius\0\xe4";
    let records = named_prototype_records(payload);

    assert_eq!(
        records[0].field("local_sys").map(|field| &field.value),
        Some(&SurfaceNamedValue::Opaque(
            b"\xf9\x04\x03\xfb\x18\xe5\x0f\x0f\x0f\xe4\x0f\x0f\x0f\x2f\x2e\0\x18".to_vec()
        ))
    );
}

#[test]
fn named_local_system_uses_the_signed_coordinate_dict_lane() {
    let payload = b"srf_prim_ptr(torus)\0\
        \xe0\x02local_sys\0\xf9\x04\x03\
        \x7a\xeb\xb6\x28\xd0\x03\x82\
        \x28\xb2\x01\x83\xce\x09\x70\xf1\
        \x18\xe5\x10\
        \x41\xb2\x01\x83\xce\x09\x70\xf1\
        \x7a\xeb\xb6\x28\xd0\x03\x82\x18\
        \x48\x66\x80\x48\x08\x00\x2f\x44\x00";
    let records = named_prototype_records(payload);
    let SurfaceNamedValue::ScalarArray { values, .. } =
        &records[0].field("local_sys").expect("local system").value
    else {
        panic!("scalar local system");
    };

    assert_eq!(values[0], Some(0.997_523_383_819_597_8));
    assert_eq!(values[1], Some(0.070_335_614_969_227_37));
    assert_eq!(values[6], Some(-0.070_335_614_969_227_37));
    assert_eq!(values[7], Some(0.997_523_383_819_597_8));
    assert_eq!(&values[9..12], &[Some(-180.0), Some(-3.0), Some(40.0)]);
}

#[test]
fn named_local_system_decodes_positive_compact_half_coordinate() {
    let body = [0xf9, 0x04, 0x03, 0x0e];
    let SurfaceNamedValue::ScalarArray { values, .. } = named_surface_value(
        &SurfacePrototypeFamily::Plane,
        "local_sys",
        &body,
        &scalar::ScalarCache::default(),
    ) else {
        panic!("scalar local system");
    };

    assert_eq!(values[0], Some(0.5));
}

#[test]
fn dimensioned_scalar_arrays_decode_compact_extents() {
    let mut body = vec![0xf9, 0x80, 0x88, 0x03];
    body.extend([0x0f; 136 * 3]);
    let SurfaceNamedValue::ScalarArray {
        dimensions,
        count,
        values,
        ..
    } = named_surface_value(
        &SurfacePrototypeFamily::Spline,
        "i_points",
        &body,
        &scalar::ScalarCache::default(),
    )
    else {
        panic!("dimensioned scalar array");
    };

    assert_eq!(dimensions, 136);
    assert_eq!(count, 3);
    assert_eq!(values.len(), 408);
    assert!(values.iter().all(|value| *value == Some(0.0)));
}

#[test]
fn fillet_vectors_use_the_signed_coordinate_dict_lane() {
    let negative = [0xc2, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc];
    let mut payload = b"srf_prim_ptr(fillet_srf)\0\xe0\x02i_pnts\0\xf9\x01\x03".to_vec();
    payload.extend_from_slice(&negative);
    payload.extend_from_slice(&[0xe4, 0x0f]);

    let records = named_prototype_records(&payload);

    assert_eq!(
        records[0].field("i_pnts").map(|field| &field.value),
        Some(&SurfaceNamedValue::ScalarArray {
            dimensions: 1,
            count: 3,
            values: vec![
                Some(f64::from_be_bytes([
                    0xbf, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc,
                ])),
                Some(1.0),
                Some(0.0),
            ],
            tokens: vec![negative.to_vec(), vec![0xe4], vec![0x0f]],
        })
    );
}

#[test]
fn fillet_vectors_dispatch_positive_coordinate_lanes_by_field() {
    let payload = b"srf_prim_ptr(fillet_srf)\0\
        \xe0\x02i_pnts\0\xf9\x01\x03\x98\x01\x02\x03\x04\x05\x06\xe4\xe4\
        \xe0\x02tangts\0\xf9\x01\x03\x4c\x01\x02\x03\x04\x05\x06\xe4\xe4";

    let records = named_prototype_records(payload);
    let prototype = &records[0];

    assert!(matches!(
        prototype.field("i_pnts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray { values, .. })
            if values == &[
                Some(f64::from_be_bytes([0x40, 0x0d, 1, 2, 3, 4, 5, 6])),
                Some(1.0),
                Some(1.0),
            ]
    ));
    assert!(matches!(
        prototype.field("tangts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray { values, .. })
            if values == &[
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

    let records = named_prototype_records(payload);

    assert!(matches!(
        records[0].field("i_pnts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray { values, .. })
            if values == &[
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

    let records = named_prototype_records(payload);
    let prototype = &records[0];

    assert_eq!(
        prototype.field("i_pnts").map(|field| field.body.as_slice()),
        Some(&[0xf9, 0x01, 0x03, 0xaa, 0xe0, 0x01, b'i', b'd', 0x00, 0xe3, 0xe4, 0x0f,][..])
    );
    assert!(prototype.field("id").is_none());
    assert!(matches!(
        prototype.field("tangts").map(|field| &field.value),
        Some(SurfaceNamedValue::ScalarArray {
            dimensions: 1,
            count: 3,
            values,
            ..
        }) if values == &[Some(1.0), Some(1.0), Some(1.0)]
    ));
}

#[test]
fn named_torus_radii_decode_compact_positive_quarters() {
    let payload = b"srf_prim_ptr(torus)\0\
        \xe0\x01radius1\0\x0e\
        \xe0\x01radius2\0\x0d\xf1\xf7\x0e\xe3";
    let records = named_prototype_records(payload);

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

    let records = named_prototype_records(&payload);

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

    let records = named_prototype_records(&payload);

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

    let records = named_prototype_records(&payload);

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
    let records = named_prototype_records(payload);

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
    let records = named_prototype_records(payload);

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
                &SurfacePrototypeFamily::Spline,
                name,
                body,
                &scalar::ScalarCache::default(),
            ),
            SurfaceNamedValue::Opaque(body.to_vec())
        );
    }
}

#[test]
fn parent_feature_array_accepts_its_exact_reference_trailer() {
    let payload = b"srf_prim_ptr(plane)\0\xe0\0parent_feats\0\
        \xf8\x02\x07\x08\xf7\x03\x09\xe1\xf6\xf6";
    let records = named_prototype_records(payload);

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
                &SurfacePrototypeFamily::Spline,
                "parent_feats",
                &body,
                &scalar::ScalarCache::default(),
            ),
            SurfaceNamedValue::Opaque(body)
        );
    }
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
