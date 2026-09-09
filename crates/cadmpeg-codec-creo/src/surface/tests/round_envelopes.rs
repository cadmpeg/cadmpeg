// SPDX-License-Identifier: Apache-2.0

use super::EPS_FRAME_COMPONENT;
use crate::scalar;
use crate::surface::{
    decode_complete_directrix_interval_cylinder_frame, parameter_records, PositionalCylinderFrame,
    SurfaceBodyBoundary, SurfaceParameterRecord, Type24RoundEdgeEnvelope,
};

#[test]
fn decodes_structurally_delimited_type24_round_edge_envelope() {
    let mut body = vec![0x34, 0xe0, 0x00];
    body.extend_from_slice(&[0x56, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x00, 0x12, 0x68]);
    body.extend_from_slice(&[0x6b, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x0f, 0xe4, 0x2f, 0x00, 0x00]);
    body.extend_from_slice(&[0x0d, 0x2f, 0x00, 0x00, 0x0f]);
    body.extend_from_slice(&[0xf7, 0x17]);

    let record = SurfaceParameterRecord {
        surface_id: 7,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Cylinder,
        ),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };

    assert_eq!(
        record.type24_round_edge_envelope(),
        Some(Type24RoundEdgeEnvelope {
            parameter_interval: [
                f64::from_be_bytes([0x3f, 0xcb, 0, 0, 0, 0, 0, 0]),
                f64::from_be_bytes([0x3f, 0xe0, 0, 0, 0, 0, 0, 0]),
            ],
            vertices: [[0.0, 1.0, 2.0], [-1.0, 2.0, 0.0]],
            generated_entity_reference: Some(0x17),
        })
    );
    assert!(crate::surface::SurfaceParameterRecord {
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Cone
        ),
        ..record.clone()
    }
    .type24_round_edge_envelope()
    .is_none());
}

#[test]
fn round_edge_envelope_accepts_model_reference_shell() {
    let mut body = vec![0x32, 0xe4, 0, 0, 0, 0, 0, 0];
    body.extend_from_slice(&[0x0f, 0x12, 0xe4]);
    body.extend_from_slice(&[0x2d, 0x00, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    body.push(0x0f);
    body.extend_from_slice(&[0x2d, 0x10, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x46, 0x14, 0, 0, 0, 0, 0, 0]);
    body.push(0xe4);

    let parameter = SurfaceParameterRecord {
        surface_id: 7,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Cylinder,
        ),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let envelope = parameter
        .type24_round_edge_envelope()
        .expect("complete model-reference-shell round envelope");

    assert_eq!(envelope.parameter_interval, [0.0, 1.0]);
    assert_eq!(envelope.vertices, [[2.0, -3.0, 0.0], [4.0, -5.0, 1.0]]);

    let mut truncated = parameter;
    truncated.body.remove(7);
    assert!(truncated.type24_round_edge_envelope().is_none());
}

#[test]
fn round_edge_vertices_use_the_first_directrix_coordinate_lane() {
    let mut body = vec![0x18, 0x0f, 0x12, 0xe4];
    body.extend_from_slice(&[0x2d, 0x00, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
    body.push(0x0f);
    body.extend_from_slice(&[0x2d, 0x10, 0, 0, 0, 0, 0, 0]);
    body.extend_from_slice(&[0x46, 0x14, 0, 0, 0, 0, 0, 0]);
    body.push(0xe4);

    let parameter = SurfaceParameterRecord {
        surface_id: 7,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Cylinder,
        ),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let envelope = parameter
        .type24_round_edge_envelope()
        .expect("complete directrix-lane endpoint envelope");

    assert_eq!(envelope.parameter_interval, [0.0, 1.0]);
    assert_eq!(envelope.vertices, [[2.0, -3.0, 0.0], [4.0, -5.0, 1.0]]);
}

#[test]
fn complete_directrix_interval_cylinders_accept_selector_opener_variants() {
    let build = |opener: &[u8], values: [f64; 7]| {
        let mut body = opener.to_vec();
        for value in values {
            let raw = value.to_be_bytes();
            assert_eq!(raw[0], 0x40, "test value uses the positive directrix form");
            body.push(0x2d);
            body.extend_from_slice(&raw[1..]);
        }
        body.extend_from_slice(&[0xf7, 0x17, 0xe3, 0x99]);
        body
    };
    let values = [2.0, 2.0, 3.0, 4.0, 6.0, 5.0, 6.0];
    let expected = PositionalCylinderFrame::new(
        [4.0, 5.0, 2.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        2.0,
        Some(4.0),
    )
    .expect("valid positional cylinder frame");
    for opener in [
        &[0x18, 0xe4, 0x11][..],
        &[0x18, 0xe4, 0x00, 0x11, 0x07],
        &[0x00, 0x11, 0x07, 0x18, 0x13],
    ] {
        assert_eq!(
            decode_complete_directrix_interval_cylinder_frame(
                &build(opener, values),
                &scalar::ScalarCache::default(),
            ),
            Some(expected)
        );
    }
    let inconsistent_interval = build(&[0x18, 0xe4, 0x11], [2.0, 2.0, 3.0, 4.0, 6.0, 5.0, 7.0]);
    assert!(decode_complete_directrix_interval_cylinder_frame(
        &inconsistent_interval,
        &scalar::ScalarCache::default(),
    )
    .is_none());
}

#[test]
fn decodes_terminal_square_radial_type24_round_envelope() {
    let body = [
        0x32, 0x90, 0x32, 0x70, 0x63, 0x1c, 0x71, 0xa7, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad, 0x2a,
        0x4c, 0x12, 0x2d, 0x4f, 0x30, 0xcb, 0xcd, 0xcc, 0x62, 0xc5, 0x48, 0x58, 0xc0, 0x2d, 0x57,
        0x75, 0x9c, 0xe9, 0x32, 0x3b, 0xfa, 0x48, 0x28, 0x00, 0x48, 0x56, 0x80, 0x2d, 0x59, 0x2d,
        0x7c, 0x1f, 0xc1, 0xd8, 0x36, 0x48, 0x08, 0x00, 0xf7, 0x40,
    ];
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let record = parameter_records(&payload).remove(0);

    let frame = record
        .positional_cylinder_frame()
        .expect("complete square-radial carrier");
    assert_eq!(frame.origin(), [-94.5, -93.837_702_082_688_25, -7.5]);
    assert_eq!(frame.axis(), [0.0, -1.0, 0.0]);
    assert_eq!(frame.ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 4.5);
    assert!((frame.length.expect("required invariant") - 6.872_998_848_194_527).abs() < 1.0e-12);

    let control_terminated_body = [
        24, 45, 53, 164, 168, 193, 84, 201, 135, 18, 45, 59, 164, 168, 193, 84, 201, 135, 72, 51,
        0, 47, 67, 0, 72, 24, 0, 72, 34, 0, 47, 72, 0, 24,
    ];
    let mut control_terminated_payload = vec![7, 0x24, 4, 0x01, 0, 0];
    control_terminated_payload.extend_from_slice(&control_terminated_body);
    control_terminated_payload.push(0xe3);
    let control_terminated = parameter_records(&control_terminated_payload).remove(0);
    let frame = control_terminated
        .positional_cylinder_frame()
        .expect("control-terminated square-radial carrier");
    assert!(frame
        .origin()
        .into_iter()
        .zip([-27.643_2, -14.0, 43.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert_eq!(frame.axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction(), [0.0, 1.0, 0.0]);
    assert!((frame.radius - 5.0).abs() < 1.0e-12);
    assert!((frame.length.expect("required invariant") - 21.643_2).abs() < 1.0e-12);

    let mut ambiguous = record.clone();
    ambiguous.scalar_frames[1].slots[6].value = Some(-102.837_702_082_688_25);
    assert!(ambiguous.type24_square_radial_round_frame().is_none());

    let mut unowned_tail = record;
    unowned_tail.body.push(0x00);
    assert!(unowned_tail.type24_square_radial_round_frame().is_none());

    let six_slot_body = [
        27, 244, 0, 86, 19, 73, 195, 99, 182, 160, 18, 45, 26, 98, 51, 231, 180, 183, 80, 72, 62,
        0, 45, 29, 51, 51, 51, 51, 51, 153, 71, 9, 153, 71, 61, 204, 45, 30, 0, 0, 0, 0, 0, 101,
        46, 9, 153, 247, 23,
    ];
    let mut six_slot_payload = vec![7, 0x24, 4, 0x01, 0, 0];
    six_slot_payload.extend_from_slice(&six_slot_body);
    six_slot_payload.push(0xe3);
    let six_slot = parameter_records(&six_slot_payload).remove(0);
    let frame = six_slot
        .positional_cylinder_frame()
        .expect("complete six-slot square-radial carrier");
    assert!(frame
        .origin()
        .into_iter()
        .zip([-29.9, -7.4, -3.2])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert_eq!(frame.axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction(), [1.0, 0.0, 0.0]);
    assert!((frame.radius - 0.1).abs() < 1.0e-12);
    assert!((frame.length.expect("required invariant") - 6.4).abs() < 1.0e-12);

    let nine_slot_body = [
        0x18, 0x18, 0x18, 0x48, 0x24, 0x00, 0x2e, 0x1f, 0xff, 0x2f, 0x14, 0x00, 0x48, 0x22, 0x00,
        0x2f, 0x48, 0x00, 0x2f, 0x18, 0x00, 0xf7, 0x18,
    ];
    let mut nine_slot_payload = vec![7, 0x24, 4, 0x01, 0, 0];
    nine_slot_payload.extend_from_slice(&nine_slot_body);
    nine_slot_payload.push(0xe3);
    let nine_slot = parameter_records(&nine_slot_payload).remove(0);
    let frame = nine_slot
        .positional_cylinder_frame()
        .expect("complete nine-slot square-radial carrier");
    assert!(frame
        .origin()
        .into_iter()
        .zip([-9.5, 8.0, 5.5])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert_eq!(frame.axis(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 0.5);
    assert_eq!(frame.length, Some(40.0));

    let single_diameter_body = [
        0x18, 0x2f, 0x00, 0x00, 0x48, 0x68, 0x10, 0x48, 0x14, 0x00, 0x2f, 0x3b, 0x80, 0x48, 0x64,
        0xf0, 0x48, 0x08, 0x00, 0x2f, 0x44, 0x00, 0xf7, 0x16,
    ];
    let mut single_diameter_payload = vec![7, 0x24, 4, 0x01, 0, 0];
    single_diameter_payload.extend_from_slice(&single_diameter_body);
    single_diameter_payload.push(0xe3);
    let single_diameter = parameter_records(&single_diameter_payload).remove(0);
    let frame = single_diameter
        .positional_cylinder_frame()
        .expect("complete single-diameter carrier");
    assert_eq!(frame.origin(), [-192.5, -4.0, 27.5]);
    assert_eq!(
        frame.axis(),
        [2.0 / 5.0_f64.sqrt(), 0.0, 1.0 / 5.0_f64.sqrt()]
    );
    assert_eq!(frame.ref_direction(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.radius, 1.0);
    assert!((frame.length.expect("bounded carrier") - 31.25_f64.sqrt() * 5.0).abs() < 1.0e-12);

    let collision_body = [
        0x2f, 0x00, 0x00, 0x2f, 0x10, 0x00, 0x0f, 0x0f, 0x0f, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00,
        0x2f, 0x10, 0x00,
    ];
    let mut collision_payload = vec![7, 0x24, 4, 0x01, 0, 0];
    collision_payload.extend_from_slice(&collision_body);
    collision_payload.push(0xe3);
    let collision = parameter_records(&collision_payload).remove(0);
    assert!(collision.type24_single_diameter_round_frame().is_some());
    assert!(collision.type24_square_radial_round_frame().is_some());
    assert!(collision.positional_cylinder_frame().is_none());

    let unbounded_body = [
        0x18, 0x2d, 0x5f, 0x25, 0xa4, 0x69, 0xd7, 0x34, 0x2d, 0x00, 0x12, 0x00, 0x2d, 0x67, 0x06,
        0x05, 0x68, 0x1e, 0xcd, 0x4a, 0x46, 0x3d, 0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0xd0, 0x46, 0x16,
        0xcc, 0xcc, 0xcc, 0xcc, 0xcc, 0x5c, 0x2e, 0x1f, 0x33, 0x2e, 0x3d, 0xcc, 0x46, 0x15, 0xff,
        0xff, 0xff, 0xff, 0xff, 0x8f, 0x2f, 0x20, 0x00,
    ];
    let mut unbounded_payload = vec![7, 0x24, 4, 0x01, 0, 0];
    unbounded_payload.extend_from_slice(&unbounded_body);
    unbounded_payload.push(0xe3);
    let unbounded = parameter_records(&unbounded_payload).remove(0);
    let frame = unbounded
        .positional_cylinder_frame()
        .expect("complete zero-axial square-radial carrier");
    assert!((frame.origin()[0] - 29.8).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.origin()[1] - 5.6).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.origin()[2] - 7.9).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction(), [0.0, -1.0, 0.0]);
    assert!((frame.radius - 0.1).abs() < 1.0e-12);
    assert_eq!(frame.length, None);

    let mut unequal_radials = unbounded;
    unequal_radials.scalar_frames[1].slots[6].value = Some(8.1);
    assert!(unequal_radials.type24_square_radial_round_frame().is_none());
}

#[test]
fn decodes_negative_a7_repeated_diameter_round_envelope() {
    let body = [
        0x18, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x2d, 0x45, 0x7d, 0x56, 0x6c,
        0xf4, 0x1f, 0x22, 0x2d, 0x45, 0x26, 0x66, 0x66, 0x66, 0x66, 0x66, 0x2a, 0xf4, 0x00, 0xa7,
        0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0x2e, 0x45, 0x66, 0x2a, 0xfc, 0x00, 0x5e, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x80,
    ];
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let frame = parameter_records(&payload)[0]
        .positional_cylinder_frame()
        .expect("complete signed-DICT repeated-diameter carrier");

    assert_eq!(frame.origin(), [-42.3, 1.25, 0.0]);
    assert_eq!(frame.ref_direction(), [0.0, 0.0, 1.0]);
    assert!((frame.radius - 0.3).abs() < 1.0e-12);
    let length = 85.1_f64.hypot(0.5);
    assert!((frame.length.expect("required invariant") - length).abs() < 1.0e-12);
    assert!((frame.axis()[0] - 85.1 / length).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.axis()[1] - 0.5 / length).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.axis()[2], 0.0);
}

#[test]
fn decodes_prefixed_repeated_diameter_round_envelope() {
    let body = [
        0xeb, 0xba, 0xc2, 0x1d, 0x3a, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x2d,
        0x45, 0x7d, 0x56, 0x6c, 0xf4, 0x1f, 0x22, 0x2d, 0x45, 0x26, 0x66, 0x66, 0x66, 0x66, 0x66,
        0x42, 0xfb, 0xff, 0xa7, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0x2e, 0x45, 0x66, 0x42, 0xf3,
        0xff, 0x5e, 0x33, 0x33, 0x33, 0x33, 0x33, 0x80,
    ];
    let record = |body: &[u8]| {
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend_from_slice(body);
        payload.push(0xe3);
        parameter_records(&payload).remove(0)
    };
    let frame = record(&body)
        .positional_cylinder_frame()
        .expect("complete prefixed repeated-diameter carrier");

    assert_eq!(frame.origin()[0], -42.3);
    assert!((frame.origin()[1] + 1.75).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.origin()[2], 0.0);
    assert_eq!(frame.ref_direction(), [0.0, 0.0, 1.0]);
    assert!((frame.radius - 0.3).abs() < 1.0e-12);
    let length = 85.1_f64.hypot(0.5);
    assert!((frame.length.expect("required invariant") - length).abs() < 1.0e-12);
    assert!((frame.axis()[0] - 85.1 / length).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.axis()[1] - 0.5 / length).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.axis()[2], 0.0);

    let mut wrong_prefix = body;
    wrong_prefix[1] = 0xbb;
    assert!(record(&wrong_prefix).positional_cylinder_frame().is_none());
    let mut wrong_separator = body;
    wrong_separator[13] = 0x13;
    assert!(record(&wrong_separator)
        .positional_cylinder_frame()
        .is_none());
    assert!(record(&body[..body.len() - 7])
        .positional_cylinder_frame()
        .is_none());
}

#[test]
fn decodes_held_coordinate_type24_round_envelope() {
    let record = |body: &[u8]| {
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend_from_slice(body);
        payload.push(0xe3);
        parameter_records(&payload).remove(0)
    };
    let body = [
        0x18, 0x2d, 0x4f, 0x12, 0x6e, 0x97, 0x8d, 0x4f, 0xe0, 0x78, 0xac, 0x67, 0x05, 0x61, 0xbb,
        0x50, 0x2d, 0x54, 0x89, 0x37, 0x4b, 0xc6, 0xa7, 0xf0, 0x48, 0x24, 0x00, 0x2f, 0x41, 0x00,
        0x2f, 0x10, 0x00, 0x2f, 0x24, 0x00, 0x2f, 0x43, 0x00, 0x2f, 0x18, 0x00,
    ];
    let base_record = record(&body);
    let frame = base_record
        .positional_cylinder_frame()
        .expect("complete held-coordinate round carrier");

    assert_eq!(frame.origin(), [34.0, 5.0, 10.0]);
    assert_eq!(frame.axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.radius, 1.0);
    assert_eq!(frame.length, Some(4.0));
    assert_eq!(base_record.type24_round_radius(), Some(1.0));

    let replay_body = [
        24, 45, 79, 146, 110, 151, 141, 79, 224, 120, 172, 103, 5, 97, 187, 80, 45, 84, 73, 55, 75,
        198, 167, 240, 72, 34, 0, 47, 65, 0, 47, 16, 0, 47, 34, 0, 47, 67, 0, 47, 24, 0, 247, 24,
    ];
    let replay = record(&replay_body);
    assert_eq!(
        replay.positional_cylinder_frame(),
        Some(
            PositionalCylinderFrame::new(
                [34.0, 5.0, 9.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                1.0,
                Some(4.0)
            )
            .expect("valid positional cylinder frame")
        )
    );
    assert_eq!(replay.type24_round_radius(), Some(1.0));
    assert_eq!(
        record(&replay_body[..replay_body.len() - 2]).positional_cylinder_frame(),
        replay.positional_cylinder_frame(),
    );

    let mut broken_replay = replay_body;
    broken_replay[43] = 0x19;
    assert!(record(&broken_replay).positional_cylinder_frame().is_none());

    let mut wrong_control = body;
    wrong_control[25] = 0x25;
    assert!(record(&wrong_control).positional_cylinder_frame().is_none());
}

#[test]
fn decodes_terminal_type24_round_radius() {
    let record = |body: &[u8]| {
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend_from_slice(body);
        payload.push(0xe3);
        parameter_records(&payload).remove(0)
    };
    let terminal = [
        0x18, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x2d, 0x45, 0x7d, 0x56, 0x6c,
        0xf4, 0x1f, 0x22, 0x2d, 0x45, 0x26, 0x66, 0x66, 0x66, 0x66, 0x66, 0x2a, 0xf4, 0x00, 0xa7,
        0x33, 0x33, 0x33, 0x33, 0x33, 0x80, 0x2e, 0x45, 0x66, 0x2a, 0xfc, 0x00, 0x5e, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x80,
    ];
    assert!(
        (record(&terminal)
            .type24_round_radius()
            .expect("required invariant")
            - 0.3)
            .abs()
            < 1.0e-12
    );

    let mut replay_terminated = terminal.to_vec();
    replay_terminated.extend_from_slice(&[0xf7, 0x17]);
    assert!(
        (record(&replay_terminated)
            .type24_round_radius()
            .expect("required invariant")
            - 0.3)
            .abs()
            < 1.0e-12
    );

    let mut trailing_payload = terminal.to_vec();
    trailing_payload.push(0x18);
    assert!(record(&trailing_payload).type24_round_radius().is_none());
    let coordinate_terminal = [
        0x18, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x46, 0x16, 0xd9, 0xc0, 0xeb,
        0x43, 0x76, 0xac,
    ];
    assert!(record(&coordinate_terminal).type24_round_radius().is_none());
    assert!(crate::surface::SurfaceParameterRecord {
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Plane
        ),
        ..record(&terminal)
    }
    .type24_round_radius()
    .is_none());
}
