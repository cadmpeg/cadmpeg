// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::EPS_FRAME_COMPONENT;
use crate::surface::named_prototype_records;
use crate::surface::parameter_records;
use crate::surface::prototype_count;
use crate::surface::split_cylinder_outline_bounds;
use crate::surface::tabulated_cylinder_curve_replays;
use crate::surface::unique_surface_parameter;
use crate::surface::PositionalCylinderFrame;
use crate::surface::PositionalTorusFrame;
use crate::surface::SurfaceBodyBoundary;
use crate::surface::SurfaceNamedValue;
use crate::surface::SurfaceParameterOpaqueSpan;
use crate::surface::SurfaceParameterRecord;
use crate::surface::SurfaceParameterScalar;
use crate::surface::SurfaceParameterScalarFrame;
use crate::surface::Type24RoundEnvelope;
use crate::surface::EPS_CYLINDER_GEOMETRY_MIN;
use cadmpeg_ir::scalar::PositiveLength;

const EPS_ROUND_RADIUS: f64 = 1.0e-12;

fn line_extrusion_parameter_record(
    direction: [f64; 3],
    directrix: [[f64; 3]; 2],
) -> SurfaceParameterRecord {
    let slot = |value, offset| SurfaceParameterScalar {
        value: Some(value),
        raw: vec![0x18],
        offset,
    };
    let direction_slots = direction
        .into_iter()
        .enumerate()
        .map(|(index, value)| slot(value, index))
        .collect::<Vec<_>>();
    let directrix_slots = directrix
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, value)| slot(value, index + 6))
        .collect::<Vec<_>>();
    let scalar_tokens = direction_slots
        .iter()
        .chain(&directrix_slots)
        .cloned()
        .collect::<Vec<_>>();
    SurfaceParameterRecord {
        surface_id: 1,
        body: vec![0; 12],
        scalar_tokens,
        opaque_spans: vec![SurfaceParameterOpaqueSpan {
            raw: vec![0x00, 0x0c, 0x9a],
            offset: 3,
        }],
        scalar_frames: vec![
            SurfaceParameterScalarFrame {
                offset: 0,
                slots: direction_slots,
            },
            SurfaceParameterScalarFrame {
                offset: 6,
                slots: directrix_slots,
            },
        ],
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Extrusion(
                crate::surface::ExtrusionVariant::TabulatedCylinder,
            ),
        ),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    }
}

#[test]
fn positional_line_extrusion_requires_a_non_degenerate_plane_carrier() {
    let valid = line_extrusion_parameter_record([0.0, 0.0, 1.0], [[0.0; 3], [1.0, 0.0, 0.0]]);
    assert!(valid.line_extrusion_frame().is_some());

    let zero_direction = line_extrusion_parameter_record([0.0; 3], [[0.0; 3], [1.0, 0.0, 0.0]]);
    assert!(zero_direction.line_extrusion_frame().is_none());

    let collapsed_directrix =
        line_extrusion_parameter_record([0.0, 0.0, 1.0], [[0.0; 3], [0.0; 3]]);
    assert!(collapsed_directrix.line_extrusion_frame().is_none());

    let parallel_directions =
        line_extrusion_parameter_record([1.0, 0.0, 0.0], [[0.0; 3], [1.0, 0.0, 0.0]]);
    assert!(parallel_directions.line_extrusion_frame().is_none());
}

#[test]
fn positional_torus_frame_rejects_nonfinite_or_invalid_components() {
    let valid =
        PositionalTorusFrame::new([0.0, 1.0, 2.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], 4.0, 0.5)
            .expect("valid positional torus frame");

    assert!(PositionalTorusFrame::new(
        {
            let mut value = valid.frame().origin();
            value[1] = f64::INFINITY;
            value
        },
        valid.frame().axis(),
        valid.frame().ref_direction(),
        valid.major_radius,
        valid.minor_radius
    )
    .is_none());

    assert!(PositionalTorusFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        valid.frame().ref_direction(),
        0.0,
        valid.minor_radius
    )
    .is_some());

    assert!(PositionalTorusFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        valid.frame().ref_direction(),
        -0.1,
        valid.minor_radius
    )
    .is_none());

    assert!(PositionalTorusFrame::new(
        valid.frame().origin(),
        [0.0, 0.0, 2.0],
        valid.frame().ref_direction(),
        valid.major_radius,
        valid.minor_radius
    )
    .is_none());

    assert!(PositionalTorusFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        [0.0, 0.0, 1.0],
        valid.major_radius,
        valid.minor_radius
    )
    .is_none());

    assert!(PositionalTorusFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        valid.frame().ref_direction(),
        valid.major_radius,
        f64::NAN
    )
    .is_none());
}

#[test]
fn split_cylinder_outline_requires_the_exact_terminal_layout() {
    let body = [1, 2, 0x00, 0x0c, 0x98, 3, 4, 0x0d];
    let slots = [
        (-0.3125, 0, vec![1]),
        (1.3125, 1, vec![2]),
        (0.3125, 5, vec![3]),
        (1.625, 6, vec![4]),
        (-1.0, 7, vec![0x0d]),
    ]
    .into_iter()
    .map(|(value, offset, raw)| SurfaceParameterScalar {
        value: Some(value),
        raw,
        offset,
    })
    .collect::<Vec<_>>();
    assert_eq!(
        split_cylinder_outline_bounds(&body, &slots),
        Some([[-0.3125, 1.3125], [0.3125, 1.625]])
    );

    let mut wrong_orientation = slots.clone();
    wrong_orientation[4].value = Some(1.0);
    assert!(split_cylinder_outline_bounds(&body, &wrong_orientation).is_none());
    let mut wrong_separator = body;
    wrong_separator[4] = 0x99;
    assert!(split_cylinder_outline_bounds(&wrong_separator, &slots).is_none());
}

#[test]
fn tabulated_cylinder_replay_requires_the_immediately_preceding_row() {
    let mut payload = b"srf_array\0\xf8\x02".to_vec();
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 8]);
    payload.extend_from_slice(&[8, 0x22, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&[
        9, 0x13, 0xe2, 0x01, 0x00, 0x03, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 0x04, 0xf7, 32, 0xfb, 0xe2,
        0xf7, 36,
    ]);
    for separator in [
        [0x18, 0xf1, 0xf7, 32, 0xe2].as_slice(),
        [0x18, 0xe2].as_slice(),
        [0x18, 0xe2].as_slice(),
        [0x18, 0xf2, 0xf7, 37, 0xf6, 0xe3].as_slice(),
    ] {
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(separator);
    }

    assert!(tabulated_cylinder_curve_replays(&payload).is_empty());
}

#[test]
fn tabulated_cylinder_replay_retains_its_complete_body() {
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x2c, 4, 0x01, 0, 8]);
    let replay_offset = payload.len();
    payload.extend_from_slice(&[
        9, 0x13, 0xe2, 0x01, 0x00, 0x03, 0x18, 0xe6, 0x0f, 0xe6, 0xf8, 0x04, 0xf7, 32, 0xfb, 0xe2,
        0xf7, 36,
    ]);
    for separator in [
        [0x18, 0xf1, 0xf7, 32, 0xe2].as_slice(),
        [0x18, 0xe2].as_slice(),
        [0x18, 0xe2].as_slice(),
        [0x18, 0xf2, 0xf7, 37, 0xf6, 0xe3].as_slice(),
    ] {
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(&[0x46, 0x08, 0, 0, 0, 0, 0, 0]);
        payload.extend_from_slice(separator);
    }

    let replays = tabulated_cylinder_curve_replays(&payload);
    let [replay] = replays.as_slice() else {
        panic!("one complete replay");
    };
    assert_eq!(replay.body, payload[replay_offset..]);
}

#[test]
fn positional_surface_parameter_lookup_rejects_repeated_identity() {
    let payload = [7, 0x2c, 4, 0x01, 0, 0, 0x0f, 0xe4, 0xe3];
    let records = parameter_records(&payload);
    let [record] = records.as_slice() else {
        panic!("expected one positional parameter record");
    };
    assert_eq!(unique_surface_parameter(&records, 7), Some(record));
    assert!(unique_surface_parameter(&[record.clone(), record.clone()], 7).is_none());
}

#[test]
fn decodes_bounded_untagged_type26_five_coordinate_envelope() {
    let body = [
        0x18, 0x18, 0x01, 0x11, 0x2e, 0xb0, 0x12, 0x47, 0x05, 0x33, 0x2d, 0x2d, 0xff, 0xff, 0xff,
        0xff, 0xff, 0x29, 0x47, 0x05, 0x33, 0x2e, 0x05, 0x33, 0x2d, 0x31, 0xa6, 0x66, 0x66, 0x66,
        0x66, 0x66, 0x18,
    ];
    let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let records = parameter_records(&payload);
    let [record] = records.as_slice() else {
        panic!("one type-26 parameter record");
    };
    let envelope = record
        .type26_five_coordinate_envelope()
        .expect("complete five-coordinate envelope");
    assert_eq!(envelope.offset, 7);
    assert_eq!(envelope.values[0], -2.65);
    assert!((envelope.values[1] + 15.0).abs() < 1.0e-12);
    assert_eq!(envelope.values[2], -2.65);
    assert_eq!(envelope.values[3], 2.65);
    assert!((envelope.values[4] + 17.65).abs() < 1.0e-12);

    payload[6] = 0x17;
    assert!(parameter_records(&payload)[0]
        .type26_five_coordinate_envelope()
        .is_none());
}

#[test]
fn decodes_only_an_exact_terminal_type26_minor_radius_replay() {
    let record = |body: &[u8]| {
        let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
        payload.extend_from_slice(body);
        payload.push(0xe3);
        parameter_records(&payload).remove(0)
    };
    let replay = record(&[0x18, 0x0c, 0x29, 0xc9, 0x99]);
    assert_eq!(
        replay.type26_replayed_minor_radius(0.199_999_999_999_999_98),
        Some(0.199_999_999_999_999_98)
    );
    assert!(replay.type26_replayed_minor_radius(0.2).is_none());
    assert!(crate::surface::SurfaceParameterRecord {
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Cone
        ),
        ..replay.clone()
    }
    .type26_replayed_minor_radius(0.199_999_999_999_999_98)
    .is_none());

    let two_slot_terminal = record(&[0xe4, 0x29, 0xc9, 0x99]);
    assert_eq!(
        two_slot_terminal.type26_replayed_minor_radius(0.199_999_999_999_999_98),
        Some(0.199_999_999_999_999_98)
    );
    let nonterminal_match = record(&[0x29, 0xc9, 0x99, 0xe4]);
    assert!(nonterminal_match
        .type26_replayed_minor_radius(0.199_999_999_999_999_98)
        .is_none());
    let tagged_override = record(&[
        0x18, 0x0d, 0x29, 0xc9, 0x99, 0x00, 0x0e, 0x01, 0x29, 0xdf, 0xff,
    ]);
    assert!(tagged_override
        .type26_replayed_minor_radius(0.199_999_999_999_999_98)
        .is_none());
}

#[test]
fn decodes_terminal_and_control_split_type26_five_coordinate_envelopes() {
    let bodies = [
        vec![
            0xcc, 0x4e, 0xb7, 0xaa, 0xa1, 0x3a, 0x60, 0x12, 0x41, 0x86, 0x5e, 0x2b, 0x2e, 0x79,
            0xa2, 0x91, 0x11, 0x2d, 0x1b, 0xff, 0xff, 0xff, 0xff, 0xf8, 0xf6, 0x2f, 0x14, 0x00,
            0x2f, 0x14, 0x00, 0x2f, 0x24, 0x00, 0x2d, 0x20, 0x00, 0x00, 0x00, 0x00, 0x06, 0x3c,
            0x2f, 0x18, 0x00, 0xf7, 0x1c,
        ],
        vec![
            0x28, 0x7f, 0x7d, 0xdf, 0x28, 0xe6, 0x8d, 0xaf, 0x15, 0x84, 0x41, 0x79, 0x33, 0x6d,
            0x2d, 0xaa, 0x16, 0x48, 0x24, 0x00, 0x2f, 0x14, 0x00, 0xe4, 0x4a, 0x1b, 0xff, 0xff,
            0xff, 0xff, 0xf9, 0x2d, 0x20, 0x00, 0x00, 0x00, 0x00, 0x06, 0x41, 0x2f, 0x18, 0x00,
            0xf7, 0x1c,
        ],
    ];
    let expected = [[5.0, 5.0, 10.0, -8.0, 6.0], [-10.0, 5.0, 1.0, -8.0, 6.0]];
    for (body, expected) in bodies.into_iter().zip(expected) {
        let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
        payload.extend_from_slice(&body);
        payload.push(0xe3);
        let records = parameter_records(&payload);
        let envelope = records[0]
            .type26_five_coordinate_envelope()
            .expect("terminal five-coordinate envelope");
        for (actual, expected) in envelope.values.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1.0e-11);
        }
    }
}

#[test]
fn decodes_direct_and_split_type26_torus_envelopes() {
    let prefix = [
        0x28, 0x8d, 0x07, 0x1b, 0xd2, 0x65, 0x6f, 0x6c, 0x18, 0x94, 0x3f, 0x02, 0x70, 0x16, 0xbe,
        0xfc, 0x00, 0x12, 0x20,
    ];
    let direct_tail = [
        0x47, 0x13, 0xcc, 0x46, 0x31, 0x3d, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x47, 0x13, 0xcc, 0x2e,
        0x13, 0xcc, 0x46, 0x30, 0xbd, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x21,
    ];
    let split_tail = [
        0x47, 0x13, 0xcc, 0x46, 0x31, 0x3d, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x3a, 0xb1, 0x47, 0xba,
        0x2e, 0x13, 0xcc, 0x46, 0x30, 0xbd, 0x70, 0xa3, 0xd7, 0x0a, 0x3e, 0x2e, 0x13, 0xcc,
    ];
    let record = |tail: &[u8]| {
        let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
        payload.extend_from_slice(&prefix);
        payload.extend_from_slice(tail);
        payload.push(0xe3);
        parameter_records(&payload).remove(0)
    };

    let direct = record(&direct_tail)
        .type26_five_coordinate_envelope()
        .expect("direct torus envelope");
    assert!(direct
        .values
        .iter()
        .zip([-4.95, 17.24, -4.95, 4.95, 16.74])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    let split = record(&split_tail)
        .type26_split_coordinate_envelope()
        .expect("split torus envelope");
    assert!(split
        .values
        .iter()
        .zip([-4.95, 17.24, 16.74, 4.95])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
}

#[test]
fn decodes_complete_positional_torus_frame() {
    let body = [
        40, 141, 7, 27, 210, 101, 111, 108, 24, 148, 63, 2, 112, 22, 190, 252, 0, 18, 32, 71, 19,
        204, 70, 49, 61, 112, 163, 215, 10, 62, 71, 19, 204, 46, 19, 204, 70, 48, 189, 112, 163,
        215, 10, 62, 33, 177, 72, 10, 227, 194, 255, 45, 89, 199, 15, 241, 65, 141, 6, 220, 32,
        138, 77, 219, 24, 229, 16, 40, 141, 6, 220, 32, 138, 77, 219, 194, 255, 45, 89, 199, 15,
        241, 24, 228, 70, 48, 189, 112, 163, 215, 10, 62, 24, 46, 17, 204, 14,
    ];
    let mut payload = vec![7, 0x26, 4, 0x01, 0, 0];
    payload.extend(body);
    payload.push(0xe3);
    let record = parameter_records(&payload).remove(0);

    let frame = record
        .positional_torus_frame()
        .expect("complete positional torus frame");
    assert!(frame
        .frame()
        .origin()
        .into_iter()
        .zip([1.0, 16.74, 0.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert!(frame
        .frame()
        .axis()
        .into_iter()
        .zip([0.0, 0.0, 1.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert!(frame
        .frame()
        .ref_direction()
        .into_iter()
        .zip([-0.999_899_554_583_406_1, 0.014_173_240_416_574_131, 0.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert!((frame.major_radius - 4.45).abs() < 1.0e-12);
    assert!((frame.minor_radius - 0.5).abs() < 1.0e-12);

    payload[55] = 0x20;
    assert!(parameter_records(&payload)[0]
        .positional_torus_frame()
        .is_none());
    payload[55] = body[49];
    payload[102] = 0x0d;
    assert!(parameter_records(&payload)[0]
        .positional_torus_frame()
        .is_none());
}

#[test]
fn decodes_repeated_diameter_type24_round_envelopes() {
    let record = |body: &[u8]| {
        let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
        payload.extend_from_slice(body);
        payload.push(0xe3);
        parameter_records(&payload).remove(0)
    };
    let panel = [
        0x15, 0x2d, 0x2b, 0x4d, 0xd8, 0x2f, 0xd7, 0x5e, 0x1f, 0x18, 0x2d, 0x2c, 0x1a, 0xa4, 0xfc,
        0xa4, 0x2a, 0xec, 0x2f, 0x00, 0x00, 0x2d, 0x36, 0x59, 0x99, 0x99, 0x99, 0x99, 0x9a, 0x42,
        0xf7, 0x33, 0x2e, 0x03, 0x33, 0x2e, 0x37, 0xcc, 0x29, 0xf7, 0x33,
    ];
    let prefixed_panel = [
        0x00, 0x15, 0x1c, 0x2d, 0x32, 0x0d, 0x52, 0x7e, 0x52, 0x15, 0x76, 0x18, 0x2d, 0x32, 0x73,
        0xb8, 0xe4, 0xb8, 0x7b, 0xdc, 0x47, 0x03, 0x33, 0x2d, 0x36, 0x59, 0x99, 0x99, 0x99, 0x99,
        0x99, 0x42, 0xf7, 0x33, 0x48, 0x00, 0x00, 0x2e, 0x37, 0xcc, 0x29, 0xf7, 0x33,
    ];
    let separated = [
        0x18, 0x2d, 0x31, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x12, 0x2d, 0x35, 0xa4, 0xa8, 0xc1,
        0x54, 0xc9, 0x87, 0x48, 0x1c, 0x00, 0x2f, 0x22, 0x00, 0x18, 0x48, 0x00, 0x00, 0x2f, 0x2c,
        0x00, 0x2f, 0x10, 0x00,
    ];

    let panel = record(&panel);
    assert!(
        (panel.type24_round_radius().expect("required invariant") - 0.2).abs() < EPS_ROUND_RADIUS
    );
    let frame = panel
        .positional_cylinder_frame()
        .expect("complete repeated-diameter carrier");
    assert_eq!(frame.frame().origin(), [2.2, -22.35, -1.45]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert!((frame.radius - 0.2).abs() < 1.0e-12);
    assert!(
        (frame.length.expect("required invariant").get() - 46.241_026_156_433_854).abs() < 1.0e-12
    );
    assert!(
        (frame.frame().axis()[1] - 46.15 / frame.length.expect("required invariant").get()).abs()
            < EPS_FRAME_COMPONENT
    );
    assert!(
        (frame.frame().axis()[2] - 2.9 / frame.length.expect("required invariant").get()).abs()
            < EPS_FRAME_COMPONENT
    );
    assert!(
        (record(&prefixed_panel)
            .type24_round_radius()
            .expect("required invariant")
            - 0.2)
            .abs()
            < 1.0e-12
    );
    assert!(
        (record(&separated)
            .type24_round_radius()
            .expect("required invariant")
            - 2.0)
            .abs()
            < 1.0e-12
    );
    let replay_separated = [
        24, 45, 82, 36, 168, 193, 84, 201, 135, 18, 45, 89, 164, 168, 193, 84, 201, 135, 47, 34, 0,
        47, 32, 0, 47, 20, 0, 47, 36, 0, 47, 67, 0, 47, 24, 0, 247, 24,
    ];
    let replay_record = record(&replay_separated);
    assert!(replay_record.type24_terminal_corner_envelope().is_some());
    assert!(crate::surface::SurfaceParameterRecord {
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Plane
        ),
        ..replay_record.clone()
    }
    .type24_terminal_corner_envelope()
    .is_none());
    let mut compound_close = replay_separated;
    *compound_close.last_mut().expect("trailer") = 0x17;
    assert_eq!(
        record(&compound_close).type24_terminal_corner_envelope(),
        replay_record.type24_terminal_corner_envelope()
    );
    let replay_frame = replay_record
        .positional_cylinder_frame()
        .expect("replay-trailed repeated-diameter carrier");
    assert_eq!(
        replay_frame,
        PositionalCylinderFrame::new(
            [9.0, 23.0, 5.0],
            [1.0 / 2.0_f64.sqrt(), 0.0, 1.0 / 2.0_f64.sqrt()],
            [0.0, 1.0, 0.0],
            15.0,
            Some(2.0_f64.sqrt())
        )
        .expect("valid positional cylinder frame")
    );
    let selector_corner_interval = [
        0x12, 0x2d, 0x40, 0x7a, 0x35, 0xc4, 0x3e, 0x21, 0x5b, 0x11, 0x2d, 0x44, 0xff, 0xd2, 0xa6,
        0xae, 0x74, 0x2b, 0x46, 0x65, 0x3f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x51, 0xd2, 0x31,
        0x1a, 0xfa, 0xb7, 0x82, 0x48, 0x28, 0x00, 0x46, 0x64, 0x1f, 0xff, 0xff, 0xff, 0xff, 0xfc,
        0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xea, 0x48, 0x08, 0x00,
    ];
    let selector_corner_record = record(&selector_corner_interval);
    assert!(selector_corner_record
        .selector_corner_interval_cylinder_frame()
        .is_some());
    assert!(crate::surface::SurfaceParameterRecord {
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Plane
        ),
        ..selector_corner_record.clone()
    }
    .selector_corner_interval_cylinder_frame()
    .is_none());
    let selector_corner_frame = selector_corner_record
        .positional_cylinder_frame()
        .expect("selector-corner interval carrier");
    assert!((selector_corner_frame.frame().origin()[0] + 161.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!(
        (selector_corner_frame.frame().origin()[1] - 38.329_481_329_444_5).abs()
            < EPS_CYLINDER_GEOMETRY_MIN
    );
    assert!((selector_corner_frame.frame().origin()[2] + 3.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert_eq!(selector_corner_frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(
        selector_corner_frame.frame().ref_direction(),
        [1.0, 0.0, 0.0]
    );
    assert!((selector_corner_frame.radius - 9.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!(selector_corner_frame
        .length
        .map(PositiveLength::get)
        .is_some_and(|length| {
            (length - 9.043_850_235_791_638).abs() < EPS_CYLINDER_GEOMETRY_MIN
        }));
    let mut referenced_controls = selector_corner_interval.to_vec();
    referenced_controls.extend_from_slice(&[0xf7, 0x40]);
    assert!(record(&referenced_controls)
        .positional_cylinder_frame()
        .is_some());
    let mut invalid_control = selector_corner_interval;
    invalid_control[0] = 0x15;
    assert!(record(&invalid_control)
        .positional_cylinder_frame()
        .is_none());
    invalid_control = selector_corner_interval;
    invalid_control[9] = 0x15;
    assert!(record(&invalid_control)
        .positional_cylinder_frame()
        .is_none());
    let prefixed_auxiliary = [
        0x19, 0xd3, 0xae, 0x70, 0x14, 0x6d, 0xb6, 0xde, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad, 0x2a,
        0x4e, 0x12, 0x2d, 0x4f, 0x01, 0x49, 0xdf, 0x84, 0xdb, 0x36, 0x48, 0x58, 0xc0, 0x2d, 0x57,
        0x75, 0x9c, 0xe9, 0x32, 0x3b, 0xfb, 0x48, 0x24, 0x00, 0x48, 0x57, 0x00, 0x2d, 0x59, 0x15,
        0xbb, 0x28, 0x9e, 0x14, 0x6f, 0x48, 0x08, 0x00, 0xf7, 0x40,
    ];
    let prefixed_frame = record(&prefixed_auxiliary)
        .positional_cylinder_frame()
        .expect("selector-prefixed auxiliary repeated-diameter carrier");
    assert!((prefixed_frame.radius - 3.250_923_087_748_478).abs() < 1.0e-12);
    assert_eq!(prefixed_frame.frame().ref_direction(), [0.0, -1.0, 0.0]);
    assert!(prefixed_frame
        .frame()
        .axis()
        .into_iter()
        .zip([
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2
        ])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    let mut alternate_selector = prefixed_auxiliary;
    alternate_selector[0] = 0x32;
    assert!(record(&alternate_selector)
        .positional_cylinder_frame()
        .is_some());
    let mut invalid_selector = prefixed_auxiliary;
    invalid_selector[0] = 0x18;
    assert!(record(&invalid_selector)
        .positional_cylinder_frame()
        .is_none());
    let split_controls = [
        0x14, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad, 0x2a, 0x4f, 0x00, 0x13, 0x1a, 0x2d, 0x4f, 0x01,
        0x49, 0xdf, 0x84, 0xdb, 0x35, 0x48, 0x58, 0xc0, 0x2d, 0x57, 0x75, 0x9c, 0xe9, 0x32, 0x3b,
        0xfc, 0x92, 0xff, 0xff, 0xff, 0xff, 0xff, 0xe8, 0x48, 0x57, 0x00, 0x2d, 0x59, 0x15, 0xbb,
        0x28, 0x9e, 0x14, 0x6e, 0x2f, 0x24, 0x00, 0xf7, 0x40,
    ];
    let split_frame = record(&split_controls)
        .positional_cylinder_frame()
        .expect("split selector-corner interval carrier");
    assert!((split_frame.frame().origin()[0] + 99.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!(
        (split_frame.frame().origin()[1] - 38.329_481_329_444_49).abs() < EPS_CYLINDER_GEOMETRY_MIN
    );
    assert!((split_frame.frame().origin()[2] - 3.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert_eq!(split_frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(split_frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert!((split_frame.radius - 7.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!(split_frame
        .length
        .map(PositiveLength::get)
        .is_some_and(|length| {
            (length - 6.501_846_175_496_936_6).abs() < EPS_CYLINDER_GEOMETRY_MIN
        }));
    let mut invalid_split_controls = split_controls;
    invalid_split_controls[10] = 0x14;
    assert!(record(&invalid_split_controls)
        .positional_cylinder_frame()
        .is_none());
    let prefixed_split_controls = [
        0x00, 0x11, 0x13, 0x2d, 0x41, 0x83, 0x08, 0x72, 0x35, 0x71, 0xa6, 0x14, 0x2d, 0x44, 0xff,
        0xd2, 0xa6, 0xae, 0x74, 0x27, 0x46, 0x64, 0x9f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x52,
        0x56, 0x9a, 0x71, 0xf6, 0x5f, 0xa7, 0x92, 0xff, 0xff, 0xff, 0xff, 0xff, 0xeb, 0x46, 0x64,
        0x1f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xe8, 0x2f,
        0x1c, 0x00, 0xf7, 0x40,
    ];
    assert!(record(&prefixed_split_controls)
        .positional_cylinder_frame()
        .is_some());
    let mut invalid_prefix = prefixed_split_controls;
    invalid_prefix[2] = 0x12;
    assert!(record(&invalid_prefix)
        .positional_cylinder_frame()
        .is_none());
    let positive_integer_extent = [
        0x12, 0x2d, 0x41, 0x83, 0x08, 0x72, 0x35, 0x71, 0xa2, 0x00, 0x11, 0x13, 0x2d, 0x44, 0xff,
        0xd2, 0xa6, 0xae, 0x74, 0x2a, 0x46, 0x64, 0x9f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x52,
        0x56, 0x9a, 0x71, 0xf6, 0x5f, 0xa5, 0x48, 0x1c, 0x00, 0x46, 0x64, 0x1f, 0xff, 0xff, 0xff,
        0xff, 0xfc, 0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xe9, 0xda, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x15,
    ];
    assert!(record(&positive_integer_extent)
        .positional_cylinder_frame()
        .is_some());
    let mut invalid_integer_controls = positive_integer_extent;
    invalid_integer_controls[10] = 0x12;
    assert!(record(&invalid_integer_controls)
        .positional_cylinder_frame()
        .is_none());

    let equal_span = record(&[
        24, 45, 47, 73, 81, 130, 169, 147, 32, 18, 45, 49, 164, 168, 193, 84, 201, 144, 47, 12, 0,
        47, 32, 0, 72, 24, 0, 47, 22, 0, 47, 36, 0, 72, 16, 0,
    ]);
    assert_eq!(
        equal_span.type24_scalar_frame_round_envelope(),
        Some(Type24RoundEnvelope {
            diameter: 2.0,
            extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
        })
    );
    assert!(equal_span.positional_cylinder_frame().is_none());

    let mut inconsistent = separated;
    inconsistent[31..34].copy_from_slice(&[0x2f, 0x12, 0x00]);
    assert!(record(&inconsistent).type24_round_radius().is_none());

    let first_coordinate = [
        0x4c, 0xb7, 0x67, 0xe1, 0x01, 0x3f, 0x80, 0x2d, 0x31, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87,
        0x12, 0x2d, 0x35, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x2f, 0x22, 0x00, 0x2f, 0x43, 0x00,
        0x48, 0x10, 0x00, 0x2d, 0x32, 0x4e, 0xfa, 0x22, 0xce, 0x34, 0xea, 0x2d, 0x47, 0xfc, 0xef,
        0xa2, 0x2c, 0xe3, 0x4f, 0x18,
    ];
    let first_coordinate = record(&first_coordinate);
    let frame = first_coordinate
        .positional_cylinder_frame()
        .expect("complete first-coordinate round carrier");
    assert_eq!(frame.frame().origin(), [9.0, 38.0, -2.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 2.0);
    let length = frame.length.expect("bounded axial span").get();
    let expected_length = 9.308_504_271_834_785_f64.hypot(9.976_063_033_979_35);
    assert!((length - expected_length).abs() < 1.0e-12);
    assert!((frame.frame().axis()[0] - 9.308_504_271_834_785 / length).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.frame().axis()[1] - 9.976_063_033_979_35 / length).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(first_coordinate.type24_round_radius(), Some(2.0));

    let mut wrong_close = first_coordinate.body.clone();
    wrong_close[49] = 0x19;
    assert!(record(&wrong_close).positional_cylinder_frame().is_none());

    let opposite = [
        0x4c, 0xb7, 0x67, 0xe1, 0x01, 0x3f, 0x80, 0x2d, 0x35, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87,
        0x12, 0x2d, 0x39, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x46, 0x32, 0x4e, 0xfa, 0x22, 0xce,
        0x34, 0xea, 0x2f, 0x43, 0x00, 0x48, 0x10, 0x00, 0x48, 0x22, 0x00, 0x2d, 0x47, 0xfc, 0xef,
        0xa2, 0x2c, 0xe3, 0x4f, 0x18,
    ];
    let opposite = record(&opposite)
        .positional_cylinder_frame()
        .expect("opposite first-coordinate round carrier");
    assert_eq!(
        opposite.frame().origin(),
        [-18.308_504_271_834_785, 38.0, -2.0]
    );
    assert_eq!(opposite.radius, 2.0);
    assert!((opposite.length.expect("required invariant").get() - expected_length).abs() < 1.0e-12);

    let segmented = [
        0x18, 0x2d, 0x35, 0xa8, 0xa9, 0xfd, 0x2c, 0xc7, 0xe2, 0x70, 0xbf, 0xe3, 0x4f, 0x05, 0x11,
        0x10, 0x2d, 0x3a, 0xc5, 0x1b, 0xc4, 0x49, 0x39, 0xa9, 0x46, 0x20, 0x38, 0xe3, 0x8e, 0x38,
        0xe3, 0x8e, 0x2f, 0x41, 0x00, 0x18, 0x48, 0x1c, 0x00, 0x2d, 0x42, 0x92, 0x43, 0xe3, 0x8f,
        0xf2, 0x60, 0x9f, 0x71, 0xc7, 0x1c, 0x71, 0xc7, 0x1c, 0xf7, 0x19,
    ];
    let segmented = record(&segmented);
    let frame = segmented
        .positional_cylinder_frame()
        .expect("complete segmented first-coordinate round carrier");
    let diameter = 5.111_111_111_111_111;
    assert_eq!(
        frame.frame().origin(),
        [-8.111_111_111_111_11, 34.0, 0.5 * diameter]
    );
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 0.5 * diameter);
    let expected_length = 1.111_111_111_111_110_7_f64.hypot(3.142_696_805_273_545);
    assert!((frame.length.expect("required invariant").get() - expected_length).abs() < 1.0e-12);
    assert_eq!(segmented.type24_round_radius(), Some(0.5 * diameter));

    let mut wrong_separator = segmented.body.clone();
    wrong_separator[9] = 0x71;
    assert!(record(&wrong_separator)
        .positional_cylinder_frame()
        .is_none());

    let split_coordinate = [
        24, 45, 49, 164, 168, 193, 84, 201, 133, 18, 45, 53, 164, 168, 193, 84, 201, 136, 47, 0, 0,
        47, 34, 0, 52, 240, 0, 47, 28, 0, 47, 44, 0, 47, 16, 0,
    ];
    let split_frame = record(&split_coordinate)
        .positional_cylinder_frame()
        .expect("split first-coordinate round carrier");
    assert_eq!(split_frame.frame().origin(), [2.0, 9.0, 2.0]);
    assert_eq!(
        split_frame.frame().axis(),
        [1.0 / 2.0_f64.sqrt(), 1.0 / 2.0_f64.sqrt(), 0.0]
    );
    assert_eq!(split_frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert!((split_frame.radius - 2.0).abs() < 1.0e-12);
    assert_eq!(
        split_frame.length.map(PositiveLength::get),
        Some(50.0_f64.sqrt())
    );
    assert!(
        (record(&split_coordinate)
            .type24_round_radius()
            .expect("split-coordinate rolling radius")
            - 2.0)
            .abs()
            < 1.0e-12
    );

    let opposite_split = [
        24, 45, 49, 164, 168, 193, 84, 201, 133, 18, 45, 53, 164, 168, 193, 84, 201, 136, 72, 28,
        0, 47, 34, 0, 52, 240, 0, 72, 0, 0, 47, 44, 0, 47, 16, 0,
    ];
    let opposite_frame = record(&opposite_split)
        .positional_cylinder_frame()
        .expect("opposite split first-coordinate round carrier");
    assert_eq!(opposite_frame.frame().origin(), [-7.0, 9.0, 2.0]);
    assert_eq!(opposite_frame.frame().axis(), split_frame.frame().axis());
    assert!((opposite_frame.radius - 2.0).abs() < 1.0e-12);

    let mut incomplete_split = split_coordinate;
    incomplete_split[24] = 0x18;
    assert!(record(&incomplete_split)
        .positional_cylinder_frame()
        .is_none());
}

#[test]
fn summarizes_seven_byte_torus_radius() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x5e\x33\x33\x33\x33\x33\x2c\xe0\x01radius2\0\x29\xc9\x99\xe3";

    assert_eq!(prototype_count(payload), 1);
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());
    let radius1 = records[0].field("radius1").expect("radius1");
    let radius2 = records[0].field("radius2").expect("radius2");
    let SurfaceNamedValue::ScalarSequence(major) = &radius1.value else {
        panic!("radius1 sequence");
    };
    let SurfaceNamedValue::ScalarSequence(minor) = &radius2.value else {
        panic!("radius2 sequence");
    };
    assert!((major[0] - 0.3).abs() < 1.0e-12);
    assert!((minor[0] - 0.2).abs() < 1.0e-12);
}

#[test]
fn scalar_tail_named_marker_does_not_end_prototype_field() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\xe4\xe0\x01radius2\0\x71\xe0\0\0\0\0\0\0\xe0\x01c_pnts\0\xf8\0";
    let records = named_prototype_records(payload, &mut crate::lane_refusal::LaneRefusals::new());
    let radius2 = records[0].field("radius2").expect("radius2 field");

    assert_eq!(radius2.body, [0x71, 0xe0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        radius2.value,
        SurfaceNamedValue::ScalarSequence(vec![f64::from_be_bytes(
            [0x3f, 0xe0, 0, 0, 0, 0, 0, 0,]
        )])
    );
}

mod cones;

#[test]
fn positional_frames_admit_by_the_ir_orthonormal_frame_measurement() {
    use crate::surface::PositionalFrame;
    use cadmpeg_ir::math::Vector3;
    use cadmpeg_ir::units::OrthonormalFrame3;

    let near = 1.0 + 9.0e-10;
    let far = 1.0 + 2.0e-9;
    for (axis, reference) in [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, near], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, far], [1.0, 0.0, 0.0]),
        ([0.6, 0.8 * near, 0.0], [-0.8, 0.6, 0.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 5.0e-10]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 2.0e-9]),
        ([0.0, 0.0, f64::NAN], [1.0, 0.0, 0.0]),
    ] {
        let admitted = OrthonormalFrame3::new(Vector3::from(axis), Vector3::from(reference));
        let held = PositionalFrame::new([1.0, 2.0, 3.0], axis, reference);
        assert_eq!(held.map(|frame| frame.orthonormal_frame()), admitted);
        if let Some(frame) = held {
            assert_eq!(frame.origin(), [1.0, 2.0, 3.0]);
            assert_eq!(frame.axis().map(f64::to_bits), axis.map(f64::to_bits));
            assert_eq!(
                frame.ref_direction().map(f64::to_bits),
                reference.map(f64::to_bits)
            );
            let reversed = frame.with_reversed_axis();
            assert_eq!(
                reversed.axis().map(f64::to_bits),
                axis.map(|component| (-component).to_bits())
            );
            assert_eq!(reversed.ref_direction(), frame.ref_direction());
        }
    }
    assert!(
        PositionalFrame::new([f64::INFINITY, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]).is_none()
    );
}
