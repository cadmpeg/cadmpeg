// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::scalar::PositiveLength;

use crate::scalar;
use crate::surface::cylinder_frame_readers::{
    decode_directrix_lane_axis_aligned_cylinder_frame, decode_local_system_suffix_cylinder_frame,
    decode_positional_cylinder_frame, unique_positional_cylinder_frame,
    unique_terminal_positive_scalar,
};
use crate::surface::{parameter_records, PositionalCylinderFrame};

const EPS_FRAME_COMPONENT: f64 = 1.0e-12;

const DIRECTRIX_LANE_CYLINDER_BODY: [u8; 47] = [
    17, 24, 19, 135, 122, 225, 71, 174, 20, 123, 71, 0, 204, 45, 45, 20, 122, 225, 71, 174, 21, 65,
    169, 153, 153, 153, 153, 153, 160, 46, 0, 204, 45, 48, 163, 215, 10, 61, 112, 164, 134, 174,
    20, 122, 225, 71, 174,
];

#[test]
fn positional_cylinder_frame_rejects_nonfinite_or_nonpositive_components() {
    let valid = PositionalCylinderFrame::new(
        [0.0, 1.0, 2.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        3.0,
        Some(4.0),
    )
    .expect("valid positional cylinder frame");

    assert!(PositionalCylinderFrame::new(
        {
            let mut value = valid.frame().origin();
            value[1] = f64::NAN;
            value
        },
        valid.frame().axis(),
        valid.frame().ref_direction(),
        valid.radius,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        valid.frame().ref_direction(),
        f64::INFINITY,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        [0.0, 0.0, 2.0],
        valid.frame().ref_direction(),
        valid.radius,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        [0.0, 0.0, f64::NAN],
        valid.frame().ref_direction(),
        valid.radius,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        [0.0, 0.0, f64::INFINITY],
        valid.frame().ref_direction(),
        valid.radius,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        [0.0, 1.0, 1.0],
        valid.radius,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        [f64::NAN, 0.0, 0.0],
        valid.radius,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        [f64::NEG_INFINITY, 0.0, 0.0],
        valid.radius,
        valid.length.map(PositiveLength::get)
    )
    .is_none());

    assert!(PositionalCylinderFrame::new(
        valid.frame().origin(),
        valid.frame().axis(),
        valid.frame().ref_direction(),
        valid.radius,
        Some(0.0)
    )
    .is_none());
}

#[test]
fn positional_cylinder_frame_rejects_conflicting_grammar_candidates() {
    let first = PositionalCylinderFrame::new(
        [1.0, 2.0, 3.0],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        2.0,
        Some(8.0),
    )
    .expect("valid positional cylinder frame");
    assert_eq!(
        unique_positional_cylinder_frame(&[first, first]),
        Some(first)
    );

    let conflicting = PositionalCylinderFrame::new(
        first.frame().origin(),
        first.frame().axis(),
        first.frame().ref_direction(),
        3.0,
        first.length().map(PositiveLength::get),
    )
    .expect("valid positional cylinder frame");
    assert_eq!(
        unique_positional_cylinder_frame(&[first, conflicting]),
        None
    );
}

#[test]
fn positional_cylinder_frame_requires_a_complete_consistent_carrier() {
    let negative_x = [
        0x11, 0x18, 0x13, 0x29, 0xd9, 0x99, 0x47, 0x03, 0x33, 0x2d, 0x35, 0x0c, 0xcc, 0xcc, 0xcc,
        0xcc, 0xcd, 0x43, 0xe8, 0x00, 0x48, 0x00, 0x00, 0x2d, 0x36, 0x8c, 0xcc, 0xcc, 0xcc, 0xcc,
        0xcd, 0x19, 0x9a, 0x79, 0x39, 0x4c, 0x9e, 0x8a, 0x0a, 0xf7, 0x19, 0xe3, 0x18, 0xe4, 0x0f,
        0xe4, 0x18, 0xe5, 0x0f, 0x18, 0x47, 0x03, 0x33, 0x2e, 0x35, 0xcc, 0x18, 0x2a, 0xe8, 0x00,
    ];
    let frame = decode_positional_cylinder_frame(&negative_x, &scalar::ScalarCache::default())
        .expect("complete positional cylinder");
    assert!((frame.frame().origin()[0] + 2.4).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.frame().origin()[1] - 21.8).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().origin()[2], 0.0);
    assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 1.0, 0.0]);
    assert!((frame.radius - 0.75).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent").get() - 0.4).abs() < 1.0e-12);

    let positive_x = [
        17, 24, 19, 41, 217, 153, 41, 255, 255, 45, 53, 12, 204, 204, 204, 204, 205, 67, 232, 0,
        46, 3, 51, 45, 54, 140, 204, 204, 204, 204, 205, 25, 154, 121, 57, 76, 158, 138, 10, 227,
        24, 228, 16, 228, 24, 229, 15, 24, 46, 3, 51, 46, 53, 204, 24, 42, 232, 0,
    ];
    let frame = decode_positional_cylinder_frame(&positive_x, &scalar::ScalarCache::default())
        .expect("oppositely oriented positional cylinder");
    assert_eq!(frame.frame().axis(), [-1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, -1.0, 0.0]);

    let compact = [
        17, 24, 19, 41, 251, 51, 67, 248, 0, 47, 49, 128, 66, 235, 51, 42, 248, 0, 47, 51, 0, 41,
        235, 51,
    ];
    let frame = decode_positional_cylinder_frame(&compact, &scalar::ScalarCache::default())
        .expect("complete compact axis-aligned cylinder");
    assert_eq!(frame.frame().origin(), [0.0, 19.0, 0.85]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, -1.0]);
    assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert!((frame.radius - 1.5).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent").get() - 1.7).abs() < 1.0e-12);

    let frame = decode_positional_cylinder_frame(
        &DIRECTRIX_LANE_CYLINDER_BODY,
        &scalar::ScalarCache::default(),
    )
    .expect("complete directrix-lane axis-aligned cylinder");
    assert_eq!(frame.frame().origin(), [0.0, 16.64, 1.73]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, -1.0]);
    assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert!((frame.radius - 2.1).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent").get() - 1.68).abs() < 1.0e-12);

    let forward_trailer = [
        17, 24, 19, 114, 174, 20, 122, 225, 71, 174, 199, 163, 215, 10, 61, 112, 164, 70, 47, 194,
        86, 31, 194, 58, 188, 142, 71, 174, 20, 122, 225, 72, 146, 112, 163, 215, 10, 61, 112, 70,
        43, 138, 4, 52, 61, 28, 4, 46, 9, 51, 247, 23,
    ];
    let frame = decode_positional_cylinder_frame(&forward_trailer, &scalar::ScalarCache::default())
        .expect("complete forward-oriented directrix-lane cylinder");
    assert!((frame.frame().origin()[0] - 0.82).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.frame().origin()[1] + 13.769_563_324_412_964).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.frame().origin()[2] - 2.41).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert!((frame.radius - 2.11).abs() < 1.0e-12);

    let compound_close_trailer = [
        17, 24, 19, 47, 33, 0, 47, 39, 0, 47, 52, 128, 71, 23, 255, 47, 50, 128, 47, 56, 0, 47, 4,
        0, 247, 25,
    ];
    let frame =
        decode_positional_cylinder_frame(&compound_close_trailer, &scalar::ScalarCache::default())
            .expect("complete compound-close directrix-lane cylinder");
    assert_eq!(frame.frame().origin()[0], 15.0);
    assert_eq!(frame.frame().origin()[1], 24.0);
    assert!((frame.frame().origin()[2] + 6.0).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 3.5);
    assert_eq!(frame.length.map(PositiveLength::get), Some(8.5));

    let zero_support = [
        17, 24, 19, 47, 32, 0, 72, 42, 128, 72, 16, 0, 67, 232, 0, 72, 39, 128, 47, 16, 0, 25, 154,
        121, 57, 76, 158, 138, 10, 247, 25, 227, 15, 24, 230, 16, 24, 15, 24, 72, 41, 0, 47, 16, 0,
        24, 42, 232, 0,
    ];
    let frame = decode_positional_cylinder_frame(&zero_support, &scalar::ScalarCache::default())
        .expect("complete zero-support positional cylinder");
    assert_eq!(frame.frame().origin(), [-12.5, 4.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, -1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 0.75);
    assert_eq!(frame.length.map(PositiveLength::get), Some(8.0));

    let signed_zero_support = [
        17, 72, 32, 0, 19, 24, 47, 39, 128, 72, 16, 0, 67, 232, 0, 47, 42, 128, 47, 16, 0, 25, 154,
        121, 57, 76, 158, 138, 10, 247, 25, 227, 16, 24, 230, 15, 24, 15, 24, 47, 41, 0, 47, 16, 0,
        24, 42, 232, 0,
    ];
    let frame =
        decode_positional_cylinder_frame(&signed_zero_support, &scalar::ScalarCache::default())
            .expect("complete signed zero-support positional cylinder");
    assert_eq!(frame.frame().origin(), [12.5, 4.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, -1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 0.75);
    assert_eq!(frame.length.map(PositiveLength::get), Some(8.0));

    let mut inconsistent_signed_length = signed_zero_support;
    inconsistent_signed_length[1..4].copy_from_slice(&[72, 33, 0]);
    assert!(decode_positional_cylinder_frame(
        &inconsistent_signed_length,
        &scalar::ScalarCache::default()
    )
    .is_none());

    let mut inconsistent_signed_origin = signed_zero_support;
    inconsistent_signed_origin[39..42].copy_from_slice(&[47, 40, 0]);
    assert!(decode_positional_cylinder_frame(
        &inconsistent_signed_origin,
        &scalar::ScalarCache::default()
    )
    .is_none());

    let referenced_planar_envelope = [
        17, 24, 19, 47, 48, 0, 71, 17, 204, 47, 48, 0, 50, 195, 162, 112, 229, 160, 63, 250, 46,
        17, 204, 24, 46, 17, 204,
    ];
    let frame = decode_positional_cylinder_frame(
        &referenced_planar_envelope,
        &scalar::ScalarCache::default(),
    )
    .expect("complete referenced planar-envelope cylinder");
    assert_eq!(frame.frame().origin(), [0.0, 0.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, -1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert!((frame.radius - 4.45).abs() < 1.0e-12);
    assert_eq!(frame.length.map(PositiveLength::get), Some(16.0));

    let reversed_referenced_planar_envelope = [
        17, 24, 19, 46, 17, 255, 71, 19, 204, 70, 48, 189, 112, 163, 215, 10, 62, 50, 197, 215, 53,
        172, 2, 203, 123, 46, 19, 204, 70, 40, 122, 225, 71, 174, 20, 125, 46, 19, 204, 247, 25,
    ];
    let frame = decode_positional_cylinder_frame(
        &reversed_referenced_planar_envelope,
        &scalar::ScalarCache::default(),
    )
    .expect("complete reversed referenced planar-envelope cylinder");
    assert!((frame.frame().origin()[0]).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.frame().origin()[1] + 12.24).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().origin()[2], 0.0);
    assert_eq!(frame.frame().axis(), [0.0, -1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert!((frame.radius - 4.95).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent").get() - 4.5).abs() < 1.0e-12);

    let held_axis = [
        17, 24, 19, 15, 70, 68, 166, 102, 102, 102, 102, 102, 16, 67, 224, 0, 70, 67, 166, 102,
        102, 102, 102, 102, 25, 161, 166, 38, 51, 20, 92, 7, 14, 247, 23,
    ];
    let frame = decode_positional_cylinder_frame(&held_axis, &scalar::ScalarCache::default())
        .expect("complete held-axis cylinder");
    assert!((frame.frame().origin()[0] + 40.3).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().origin()[1], 0.0);
    assert!((frame.frame().origin()[2] + 0.5).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert!((frame.radius - 1.0).abs() < 1.0e-12);
    assert_eq!(frame.length, None);

    let first_endpoint_axial_radial = [
        17, 24, 19, 45, 26, 28, 221, 156, 226, 254, 231, 46, 61, 204, 16, 228, 45, 66, 42, 2, 26,
        2, 198, 67, 25, 161, 166, 38, 51, 20, 92, 7, 15, 247, 23,
    ];
    let frame = decode_positional_cylinder_frame(
        &first_endpoint_axial_radial,
        &scalar::ScalarCache::default(),
    )
    .expect("complete first-endpoint axial/radial cylinder");
    assert!((frame.frame().origin()[0] - 29.8).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().origin()[1..], [0.0, 0.0]);
    assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, -1.0]);
    assert!((frame.radius - 1.0).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent").get() - 6.528_189_135_889_739).abs() < 1.0e-12);

    let second_endpoint_axial_radial = [
        17, 24, 19, 45, 26, 27, 232, 154, 196, 109, 12, 70, 66, 41, 227, 121, 190, 244, 8, 66, 239,
        255, 16, 71, 61, 204, 25, 192, 139, 195, 207, 227, 22, 71, 15, 247, 23,
    ];
    let frame = decode_positional_cylinder_frame(
        &second_endpoint_axial_radial,
        &scalar::ScalarCache::default(),
    )
    .expect("complete second-endpoint axial/radial cylinder");
    assert!((frame.frame().origin()[0] + 29.8).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().origin()[1..], [0.0, 0.0]);
    assert_eq!(frame.frame().axis(), [-1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert!((frame.radius - 1.0).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent").get() - 6.527_254_503_477_945).abs() < 1.0e-12);

    let mut inconsistent = negative_x.to_vec();
    inconsistent[58] = 0xd0;
    assert!(
        decode_positional_cylinder_frame(&inconsistent, &scalar::ScalarCache::default()).is_none()
    );
}

#[test]
fn directrix_lane_axis_aligned_cylinder_requires_a_positive_leading_scalar() {
    let cache = scalar::ScalarCache::default();
    let mut zero = DIRECTRIX_LANE_CYLINDER_BODY.to_vec();
    zero.splice(3..10, [0x18]);
    assert!(decode_directrix_lane_axis_aligned_cylinder_frame(&zero, &cache).is_none());

    let mut negative = DIRECTRIX_LANE_CYLINDER_BODY.to_vec();
    negative.splice(3..10, [0xc8, 0xd6, 0xa3, 0x0c, 0, 0, 0]);
    assert!(decode_directrix_lane_axis_aligned_cylinder_frame(&negative, &cache).is_none());
}

#[test]
fn positional_cylinder_frame_decodes_compact_y_axis_envelopes() {
    let direct = [
        0x14, 0x2f, 0x10, 0x00, 0x2d, 0x1f, 0x6a, 0x7a, 0x29, 0x55, 0x38, 0x5e, 0x2f, 0x43, 0x00,
        0x48, 0x29, 0x00, 0x2f, 0x10, 0x00, 0x43, 0xe8, 0x00, 0x48, 0x27, 0x80, 0x2f, 0x43, 0x00,
        0x2a, 0xe8, 0x00,
    ];
    let split = [
        0x12, 0x2f, 0x10, 0x00, 0x14, 0x2f, 0x43, 0x00, 0x2f, 0x27, 0x80, 0x2f, 0x10, 0x00, 0x43,
        0xe8, 0x00, 0x2f, 0x29, 0x00, 0x2f, 0x43, 0x00, 0x2a, 0xe8, 0x00,
    ];
    let cache = scalar::ScalarCache::default();

    assert_eq!(
        decode_positional_cylinder_frame(&direct, &cache),
        Some(
            PositionalCylinderFrame::new(
                [-12.5, 4.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 0.0, 0.0],
                0.75,
                Some(34.0)
            )
            .expect("valid positional cylinder frame")
        )
    );
    assert_eq!(
        decode_positional_cylinder_frame(&split, &cache),
        Some(
            PositionalCylinderFrame::new(
                [12.5, 4.0, 0.0],
                [0.0, 1.0, 0.0],
                [-1.0, 0.0, 0.0],
                0.75,
                Some(34.0)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let mut inconsistent = split;
    inconsistent[20..23].copy_from_slice(&[0x2f, 0x42, 0x00]);
    assert!(decode_positional_cylinder_frame(&inconsistent, &cache).is_none());
    assert!(decode_positional_cylinder_frame(&direct[..direct.len() - 3], &cache).is_none());
}

#[test]
fn positional_cylinder_frame_decodes_signed_radial_envelopes() {
    let cache = scalar::ScalarCache::default();
    let outer_left = [
        17, 72, 40, 0, 19, 72, 33, 0, 72, 49, 0, 47, 54, 0, 47, 4, 0, 72, 42, 0, 47, 56, 0, 47, 24,
        0, 247, 25,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&outer_left, &cache),
        Some(
            PositionalCylinderFrame::new(
                [-15.0, 24.0, 6.0],
                [0.0, 0.0, -1.0],
                [-1.0, 0.0, 0.0],
                2.0,
                Some(12.0)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let middle_left = [
        17, 72, 33, 0, 19, 24, 72, 50, 128, 47, 52, 128, 71, 23, 255, 72, 39, 0, 47, 56, 0, 47, 4,
        0, 247, 25,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&middle_left, &cache),
        Some(
            PositionalCylinderFrame::new(
                [-15.0, 24.0, 2.5],
                [0.0, 0.0, -1.0],
                [-1.0, 0.0, 0.0],
                3.5,
                Some(8.5)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let outer_right = [
        17, 47, 33, 0, 19, 47, 40, 0, 47, 42, 0, 47, 54, 0, 47, 4, 0, 47, 49, 0, 47, 56, 0, 47, 24,
        0,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&outer_right, &cache),
        Some(
            PositionalCylinderFrame::new(
                [15.0, 24.0, -6.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                2.0,
                Some(12.0)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let terminal_zero_negative = [
        17, 72, 89, 0, 19, 24, 72, 117, 104, 72, 104, 16, 72, 89, 0, 72, 115, 56, 72, 101, 224, 24,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&terminal_zero_negative, &cache),
        Some(
            PositionalCylinderFrame::new(
                [-325.0, -175.0, 0.0],
                [0.0, 0.0, -1.0],
                [-1.0, 0.0, 0.0],
                17.5,
                Some(100.0)
            )
            .expect("valid positional cylinder frame")
        )
    );

    assert!(
        decode_positional_cylinder_frame(&outer_left[..outer_left.len() - 2], &cache).is_none()
    );
    let mut inconsistent_radius = outer_right;
    inconsistent_radius[17..20].copy_from_slice(&[47, 50, 0]);
    assert!(decode_positional_cylinder_frame(&inconsistent_radius, &cache).is_none());
}

#[test]
fn positional_cylinder_frame_decodes_signed_axis_aligned_envelopes() {
    let cache = scalar::ScalarCache::default();
    let forward = [
        17, 72, 0, 0, 19, 24, 72, 55, 192, 70, 29, 255, 255, 255, 255, 255, 143, 72, 38, 0, 72, 52,
        64, 70, 21, 255, 255, 255, 255, 255, 143, 72, 34, 128,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&forward, &cache),
        Some(
            PositionalCylinderFrame::new(
                [-22.0, 5.499_999_999_999_9, -9.25],
                [0.0, 1.0, 0.0],
                [-1.0, 0.0, 0.0],
                1.75,
                Some(2.0)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let reversed = [
        17, 72, 0, 0, 19, 24, 47, 52, 64, 70, 29, 255, 255, 255, 255, 255, 143, 72, 38, 0, 47, 55,
        192, 70, 21, 255, 255, 255, 255, 255, 143, 72, 34, 128, 247, 23,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&reversed, &cache),
        Some(
            PositionalCylinderFrame::new(
                [22.0, 7.499_999_999_999_9, -9.25],
                [0.0, -1.0, 0.0],
                [1.0, 0.0, 0.0],
                1.75,
                Some(2.0)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let mut ambiguous_axis = forward;
    ambiguous_axis[20..23].copy_from_slice(&[72, 54, 0]);
    assert!(decode_positional_cylinder_frame(&ambiguous_axis, &cache).is_none());
    assert!(decode_positional_cylinder_frame(&reversed[..reversed.len() - 1], &cache).is_none());
}

#[test]
fn positional_cylinder_frame_decodes_xz_axis_y_radial_envelopes() {
    let cache = scalar::ScalarCache::default();
    let macro_zero = [
        32, 16, 0, 45, 48, 95, 210, 181, 75, 36, 250, 142, 178, 2, 128, 130, 232, 214, 45, 53, 164,
        168, 193, 84, 201, 136, 45, 32, 56, 227, 142, 56, 227, 144, 45, 66, 106, 9, 230, 103, 243,
        189, 52, 240, 0, 47, 34, 0, 45, 66, 170, 9, 230, 103, 243, 189, 160, 19, 88, 48, 38, 146,
        52,
    ];
    let compact_zero = [
        32, 16, 0, 45, 53, 164, 168, 193, 84, 201, 135, 142, 178, 2, 128, 130, 232, 193, 45, 58,
        233, 127, 49, 250, 214, 118, 72, 34, 0, 45, 66, 106, 9, 230, 103, 243, 190, 24, 70, 32, 56,
        227, 142, 56, 227, 144, 45, 66, 170, 9, 230, 103, 243, 189, 160, 19, 89, 194, 152, 51, 188,
    ];
    for body in [macro_zero.as_slice(), compact_zero.as_slice()] {
        let frame = decode_positional_cylinder_frame(body, &cache)
            .expect("complete XZ-axis cylinder frame");
        assert!((frame.radius - 0.25).abs() < 1.0e-12);
        assert!(frame.frame().axis()[1].abs() < EPS_FRAME_COMPONENT);
        assert_eq!(frame.frame().ref_direction(), [0.0, -1.0, 0.0]);
        assert!(frame
            .length
            .map(PositiveLength::get)
            .is_some_and(|length| length > 17.0));
    }

    let mut inconsistent = compact_zero;
    inconsistent[54] = 0x18;
    assert!(decode_positional_cylinder_frame(&inconsistent, &cache).is_none());
}

#[test]
fn positional_cylinder_frame_decodes_symmetric_revolution_envelopes() {
    let cache = scalar::ScalarCache::default();
    let direct = [
        21, 45, 35, 122, 225, 71, 174, 20, 124, 24, 45, 36, 28, 61, 7, 246, 190, 79, 71, 27, 153,
        70, 36, 28, 61, 7, 246, 190, 79, 24, 46, 27, 153, 70, 35, 122, 225, 71, 174, 20, 124, 46,
        27, 153, 247, 25,
    ];
    let replay = [
        23, 45, 35, 122, 225, 71, 174, 20, 124, 21, 45, 36, 28, 61, 7, 246, 190, 79, 71, 27, 153,
        70, 36, 28, 61, 7, 246, 190, 79, 71, 27, 153, 46, 27, 153, 70, 35, 122, 225, 71, 174, 20,
        124, 25, 206, 113, 206, 177, 182, 81, 242, 247, 25,
    ];
    for body in [direct.as_slice(), replay.as_slice()] {
        let frame = decode_positional_cylinder_frame(body, &cache)
            .expect("complete symmetric-revolution cylinder");
        assert_eq!(frame.frame().origin(), [0.0, 0.0, 0.0]);
        assert_eq!(frame.frame().axis(), [0.0, -1.0, 0.0]);
        assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
        assert!((frame.radius - 6.9).abs() < 1.0e-12);
        assert!(frame
            .length
            .map(PositiveLength::get)
            .is_some_and(|length| (length - 19.48).abs() < 1.0e-12));
    }

    let mut mismatched_repetition = replay;
    mismatched_repetition[31..34].copy_from_slice(&[0x2e, 0x1b, 0x99]);
    assert!(decode_positional_cylinder_frame(&mismatched_repetition, &cache).is_none());
    let mut trailing = direct.to_vec();
    trailing.push(0x18);
    assert!(decode_positional_cylinder_frame(&trailing, &cache).is_none());
}

#[test]
fn positional_cylinder_frame_decodes_axial_endpoint_radial_samples() {
    let cache = scalar::ScalarCache::default();
    let radius_three_and_half = [
        143, 30, 205, 113, 196, 112, 70, 24, 153, 33, 34, 156, 96, 224, 107, 14, 145, 174, 119, 80,
        63, 61, 215, 47, 49, 128, 210, 95, 146, 245, 61, 0, 232, 47, 12, 0, 47, 50, 0, 139, 106,
        254, 253, 38, 131, 216, 247, 25,
    ];
    let radius_three = [
        143, 30, 205, 113, 196, 112, 70, 24, 153, 33, 34, 156, 96, 224, 108, 14, 142, 112, 248,
        141, 237, 16, 111, 47, 49, 128, 207, 17, 142, 54, 177, 184, 109, 47, 8, 0, 47, 50, 0, 135,
        37, 34, 214, 139, 43, 42, 247, 25,
    ];
    for (body, expected_radius) in [
        (radius_three_and_half.as_slice(), 3.5),
        (radius_three.as_slice(), 3.0),
    ] {
        let frame = decode_positional_cylinder_frame(body, &cache)
            .expect("complete axial-endpoint radial-sample cylinder");
        assert_eq!(frame.frame().origin(), [0.0, 17.5, 0.0]);
        assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
        assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
        assert!((frame.radius - expected_radius).abs() < 1.0e-12);
        assert!(frame
            .length
            .map(PositiveLength::get)
            .is_some_and(|length| (length - 0.5).abs() < 1.0e-12));
    }

    let mut off_circle = radius_three;
    off_circle[33..36].copy_from_slice(&[0x2f, 0x0a, 0x00]);
    assert!(decode_positional_cylinder_frame(&off_circle, &cache).is_none());
    let mut trailing = radius_three_and_half.to_vec();
    trailing.push(0x18);
    assert!(decode_positional_cylinder_frame(&trailing, &cache).is_none());
}

#[test]
fn positional_cylinder_frame_decodes_signed_axial_radial_envelopes() {
    let cache = scalar::ScalarCache::default();
    let positive_end = [
        17, 66, 201, 153, 19, 24, 46, 61, 204, 72, 22, 0, 228, 47, 62, 0, 25, 200, 68, 116, 134,
        59, 254, 138, 47, 22, 0, 247, 23,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&positive_end, &cache),
        Some(
            PositionalCylinderFrame::new(
                [30.0, 0.0, 5.5],
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                11.0,
                Some(0.199_999_999_999_999_98)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let negative_end = [
        17, 66, 201, 153, 19, 24, 72, 62, 0, 72, 22, 0, 228, 71, 61, 204, 25, 210, 51, 87, 100,
        172, 254, 232, 47, 22, 0, 247, 23,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&negative_end, &cache),
        Some(
            PositionalCylinderFrame::new(
                [-29.799_999_999_999_997, 0.0, 5.5],
                [-1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                11.0,
                Some(0.199_999_999_999_999_98)
            )
            .expect("valid positional cylinder frame")
        )
    );

    let mut wrong_separator = positive_end;
    wrong_separator[12] = 0x10;
    assert!(decode_positional_cylinder_frame(&wrong_separator, &cache).is_none());
    assert!(
        decode_positional_cylinder_frame(&negative_end[..negative_end.len() - 2], &cache).is_none()
    );
}

#[test]
fn positional_cylinder_frame_decodes_precise_center_edge_envelope() {
    let body = [
        24, 44, 139, 97, 240, 181, 224, 8, 18, 45, 62, 3, 108, 62, 22, 188, 4, 72, 36, 0, 46, 31,
        255, 47, 20, 0, 72, 34, 0, 47, 67, 0, 47, 24, 0, 247, 25,
    ];
    let frame = decode_positional_cylinder_frame(&body, &scalar::ScalarCache::default())
        .expect("complete precise center-edge envelope");
    assert_eq!(frame.frame().origin()[0], -10.0);
    assert!((frame.frame().origin()[1] - 7.986_629_6).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().origin()[2], 5.0);
    assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 1.0);
    assert!((frame.length.expect("axial extent").get() - 30.013_370_4).abs() < 1.0e-12);

    let mut unequal_radial_spans = body;
    unequal_radial_spans[32..35].copy_from_slice(&[47, 28, 0]);
    assert!(decode_positional_cylinder_frame(
        &unequal_radial_spans,
        &scalar::ScalarCache::default()
    )
    .is_none());

    let mut distant_coarse_axial_sample = body;
    distant_coarse_axial_sample[20..23].copy_from_slice(&[47, 52, 0]);
    assert_eq!(
        decode_positional_cylinder_frame(
            &distant_coarse_axial_sample,
            &scalar::ScalarCache::default()
        ),
        Some(frame)
    );
}

#[test]
fn positional_cylinder_frame_decodes_precise_held_center_envelope() {
    let body = [
        24, 40, 150, 94, 43, 46, 129, 244, 134, 18, 45, 44, 11, 47, 21, 151, 64, 252, 72, 28, 0,
        47, 20, 0, 228, 47, 28, 0, 47, 24, 0, 228, 247, 25,
    ];
    let frame = decode_positional_cylinder_frame(&body, &scalar::ScalarCache::default())
        .expect("complete precise held-center envelope");
    assert!((frame.frame().origin()[0] - 7.021_843_6).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().origin()[1..], [5.0, 5.0]);
    assert_eq!(frame.frame().axis(), [-1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 1.0);
    assert!((frame.length.expect("axial extent").get() - 14.021_843_6).abs() < 1.0e-12);

    let mut unequal_radius_markers = body;
    unequal_radius_markers[31] = 0xe8;
    assert!(decode_positional_cylinder_frame(
        &unequal_radius_markers,
        &scalar::ScalarCache::default()
    )
    .is_none());

    let mut inconsistent_radial_edge = body;
    inconsistent_radial_edge[28..31].copy_from_slice(&[47, 28, 0]);
    assert!(decode_positional_cylinder_frame(
        &inconsistent_radial_edge,
        &scalar::ScalarCache::default()
    )
    .is_none());
}

#[test]
fn positional_cylinder_frame_decodes_local_system_suffix() {
    let body = [
        90, 178, 14, 217, 114, 169, 0, 45, 53, 168, 169, 253, 44, 199, 226, 120, 172, 103, 5, 97,
        187, 80, 45, 58, 197, 27, 196, 73, 57, 170, 47, 28, 0, 47, 65, 0, 24, 45, 32, 56, 227, 142,
        56, 227, 142, 45, 66, 146, 67, 227, 143, 242, 96, 159, 113, 199, 28, 113, 199, 32, 227, 66,
        227, 51, 66, 233, 153, 24, 41, 233, 153, 66, 227, 51, 24, 229, 15, 47, 40, 0, 47, 65, 0,
        70, 53, 168, 169, 253, 44, 199, 226, 47, 20, 0,
    ];
    let frame = decode_positional_cylinder_frame(&body, &scalar::ScalarCache::default())
        .expect("complete local-system suffix");
    assert_eq!(frame.frame().origin()[0..2], [12.0, 34.0]);
    assert!((frame.frame().origin()[2] + 21.658_843_825_753_03).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert!((frame.frame().ref_direction()[0] + 0.6).abs() < EPS_FRAME_COMPONENT);
    assert!((frame.frame().ref_direction()[1] + 0.8).abs() < EPS_FRAME_COMPONENT);
    assert_eq!(frame.frame().ref_direction()[2], 0.0);
    assert_eq!(frame.radius, 5.0);
    assert_eq!(frame.length, None);
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    assert_eq!(parameter_records(&payload)[0].type24_round_radius(), None);

    assert!(decode_positional_cylinder_frame(
        &body[..body.len() - 3],
        &scalar::ScalarCache::default()
    )
    .is_none());
    let mut ambiguous_terminal = body[..body.len() - 3].to_vec();
    ambiguous_terminal.extend_from_slice(&[0x46, 0, 0, 0, 0, 0, 0, 0xe4]);
    assert!(decode_local_system_suffix_cylinder_frame(
        &ambiguous_terminal,
        &scalar::ScalarCache::default()
    )
    .is_none());
    let mut nonorthogonal = body;
    nonorthogonal[68..71].copy_from_slice(&[41, 227, 51]);
    assert!(
        decode_positional_cylinder_frame(&nonorthogonal, &scalar::ScalarCache::default()).is_none()
    );
}

#[test]
fn terminal_positive_scalar_requires_a_unique_boundary() {
    let unique = [0x46, 0, 0, 0, 0, 0, 0, 0];
    assert_eq!(unique_terminal_positive_scalar(&unique, 0), Some((0, 2.0)));

    let ambiguous = [0x46, 0, 0, 0, 0, 0x2f, 0x10, 0];
    assert!(unique_terminal_positive_scalar(&ambiguous, 0).is_none());
}
