// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::*;

fn line_extrusion_parameter_record(
    direction: [f64; 3],
    directrix: [[f64; 3]; 2],
) -> SurfaceParameterRecord {
    let mut values = direction.into_iter().collect::<Vec<_>>();
    values.extend(directrix.into_iter().flatten());
    let slot = |value, offset| SurfaceParameterScalar {
        value: Some(value),
        raw: vec![0x18],
        offset,
        length: 1,
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
        scalar_values: values,
        scalar_tokens: scalar_tokens.clone(),
        opaque_spans: vec![SurfaceParameterOpaqueSpan {
            raw: vec![0x00, 0x0c, 0x9a],
            offset: 3,
            length: 3,
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
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    }
}

#[test]
fn positional_line_extrusion_requires_a_non_degenerate_plane_carrier() {
    let valid = line_extrusion_parameter_record([0.0, 0.0, 1.0], [[0.0; 3], [1.0, 0.0, 0.0]]);
    assert!(valid.line_extrusion_frame(0x2c).is_some());

    let zero_direction = line_extrusion_parameter_record([0.0; 3], [[0.0; 3], [1.0, 0.0, 0.0]]);
    assert!(zero_direction.line_extrusion_frame(0x2c).is_none());

    let collapsed_directrix =
        line_extrusion_parameter_record([0.0, 0.0, 1.0], [[0.0; 3], [0.0; 3]]);
    assert!(collapsed_directrix.line_extrusion_frame(0x2c).is_none());

    let parallel_directions =
        line_extrusion_parameter_record([1.0, 0.0, 0.0], [[0.0; 3], [1.0, 0.0, 0.0]]);
    assert!(parallel_directions.line_extrusion_frame(0x2c).is_none());
}

#[test]
fn positional_cone_frame_rejects_nonfinite_or_invalid_components() {
    let valid = PositionalConeFrame {
        apex: [0.0, 1.0, 2.0],
        axis: [0.0, 1.0, 0.0],
        ref_direction: [1.0, 0.0, 0.0],
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    assert!(valid.is_valid());

    let mut nonfinite_apex = valid;
    nonfinite_apex.apex[1] = f64::NAN;
    assert!(!nonfinite_apex.is_valid());

    let mut zero_angle = valid;
    zero_angle.half_angle = 0.0;
    assert!(!zero_angle.is_valid());

    let mut non_unit_axis = valid;
    non_unit_axis.axis = [0.0, 2.0, 0.0];
    assert!(!non_unit_axis.is_valid());

    let mut non_orthogonal_reference = valid;
    non_orthogonal_reference.ref_direction = [0.0, 1.0, 0.0];
    assert!(!non_orthogonal_reference.is_valid());

    let mut right_angle = valid;
    right_angle.half_angle = std::f64::consts::FRAC_PI_2;
    assert!(!right_angle.is_valid());
}

#[test]
fn positional_torus_frame_rejects_nonfinite_or_invalid_components() {
    let valid = PositionalTorusFrame {
        center: [0.0, 1.0, 2.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        major_radius: 4.0,
        minor_radius: 0.5,
    };
    assert!(valid.is_valid());

    let mut nonfinite_center = valid;
    nonfinite_center.center[1] = f64::INFINITY;
    assert!(!nonfinite_center.is_valid());

    let mut zero_major = valid;
    zero_major.major_radius = 0.0;
    assert!(zero_major.is_valid());

    let mut negative_major = valid;
    negative_major.major_radius = -0.1;
    assert!(!negative_major.is_valid());

    let mut non_unit_axis = valid;
    non_unit_axis.axis = [0.0, 0.0, 2.0];
    assert!(!non_unit_axis.is_valid());

    let mut non_orthogonal_reference = valid;
    non_orthogonal_reference.ref_direction = [0.0, 0.0, 1.0];
    assert!(!non_orthogonal_reference.is_valid());

    let mut nonfinite_minor = valid;
    nonfinite_minor.minor_radius = f64::NAN;
    assert!(!nonfinite_minor.is_valid());
}

#[test]
fn positional_cylinder_frame_rejects_nonfinite_or_nonpositive_components() {
    let valid = PositionalCylinderFrame {
        origin: [0.0, 1.0, 2.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 3.0,
        length: Some(4.0),
    };
    assert!(valid.is_valid());

    let mut nonfinite_origin = valid;
    nonfinite_origin.origin[1] = f64::NAN;
    assert!(!nonfinite_origin.is_valid());

    let mut nonfinite_radius = valid;
    nonfinite_radius.radius = f64::INFINITY;
    assert!(!nonfinite_radius.is_valid());

    let mut non_unit_axis = valid;
    non_unit_axis.axis = [0.0, 0.0, 2.0];
    assert!(!non_unit_axis.is_valid());

    let mut non_orthogonal_reference = valid;
    non_orthogonal_reference.ref_direction = [0.0, 1.0, 1.0];
    assert!(!non_orthogonal_reference.is_valid());

    let mut nonpositive_length = valid;
    nonpositive_length.length = Some(0.0);
    assert!(!nonpositive_length.is_valid());
}

#[test]
fn positional_cylinder_frame_rejects_conflicting_grammar_candidates() {
    let first = PositionalCylinderFrame {
        origin: [1.0, 2.0, 3.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        length: Some(8.0),
    };
    assert_eq!(
        unique_positional_cylinder_frame(&[first, first]),
        Some(first)
    );

    let mut conflicting = first;
    conflicting.radius = 3.0;
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
    assert!((frame.origin[0] + 2.4).abs() < 1.0e-12);
    assert!((frame.origin[1] - 21.8).abs() < 1.0e-12);
    assert_eq!(frame.origin[2], 0.0);
    assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 1.0, 0.0]);
    assert!((frame.radius - 0.75).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent") - 0.4).abs() < 1.0e-12);

    let positive_x = [
        17, 24, 19, 41, 217, 153, 41, 255, 255, 45, 53, 12, 204, 204, 204, 204, 205, 67, 232, 0,
        46, 3, 51, 45, 54, 140, 204, 204, 204, 204, 205, 25, 154, 121, 57, 76, 158, 138, 10, 227,
        24, 228, 16, 228, 24, 229, 15, 24, 46, 3, 51, 46, 53, 204, 24, 42, 232, 0,
    ];
    let frame = decode_positional_cylinder_frame(&positive_x, &scalar::ScalarCache::default())
        .expect("oppositely oriented positional cylinder");
    assert_eq!(frame.axis, [-1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, -1.0, 0.0]);

    let compact = [
        17, 24, 19, 41, 251, 51, 67, 248, 0, 47, 49, 128, 66, 235, 51, 42, 248, 0, 47, 51, 0, 41,
        235, 51,
    ];
    let frame = decode_positional_cylinder_frame(&compact, &scalar::ScalarCache::default())
        .expect("complete compact axis-aligned cylinder");
    assert_eq!(frame.origin, [0.0, 19.0, 0.85]);
    assert_eq!(frame.axis, [0.0, 0.0, -1.0]);
    assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert!((frame.radius - 1.5).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent") - 1.7).abs() < 1.0e-12);

    let directrix_lane = [
        17, 24, 19, 135, 122, 225, 71, 174, 20, 123, 71, 0, 204, 45, 45, 20, 122, 225, 71, 174, 21,
        65, 169, 153, 153, 153, 153, 153, 160, 46, 0, 204, 45, 48, 163, 215, 10, 61, 112, 164, 134,
        174, 20, 122, 225, 71, 174,
    ];
    let frame = decode_positional_cylinder_frame(&directrix_lane, &scalar::ScalarCache::default())
        .expect("complete directrix-lane axis-aligned cylinder");
    assert_eq!(frame.origin, [0.0, 16.64, 1.73]);
    assert_eq!(frame.axis, [0.0, 0.0, -1.0]);
    assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert!((frame.radius - 2.1).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent") - 1.68).abs() < 1.0e-12);

    let forward_trailer = [
        17, 24, 19, 114, 174, 20, 122, 225, 71, 174, 199, 163, 215, 10, 61, 112, 164, 70, 47, 194,
        86, 31, 194, 58, 188, 142, 71, 174, 20, 122, 225, 72, 146, 112, 163, 215, 10, 61, 112, 70,
        43, 138, 4, 52, 61, 28, 4, 46, 9, 51, 247, 23,
    ];
    let frame = decode_positional_cylinder_frame(&forward_trailer, &scalar::ScalarCache::default())
        .expect("complete forward-oriented directrix-lane cylinder");
    assert!((frame.origin[0] - 0.82).abs() < 1.0e-12);
    assert!((frame.origin[1] + 13.769_563_324_412_964).abs() < 1.0e-12);
    assert!((frame.origin[2] - 2.41).abs() < 1.0e-12);
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert!((frame.radius - 2.11).abs() < 1.0e-12);

    let compound_close_trailer = [
        17, 24, 19, 47, 33, 0, 47, 39, 0, 47, 52, 128, 71, 23, 255, 47, 50, 128, 47, 56, 0, 47, 4,
        0, 247, 25,
    ];
    let frame =
        decode_positional_cylinder_frame(&compound_close_trailer, &scalar::ScalarCache::default())
            .expect("complete compound-close directrix-lane cylinder");
    assert_eq!(frame.origin[0], 15.0);
    assert_eq!(frame.origin[1], 24.0);
    assert!((frame.origin[2] + 6.0).abs() < 1.0e-12);
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 3.5);
    assert_eq!(frame.length, Some(8.5));

    let zero_support = [
        17, 24, 19, 47, 32, 0, 72, 42, 128, 72, 16, 0, 67, 232, 0, 72, 39, 128, 47, 16, 0, 25, 154,
        121, 57, 76, 158, 138, 10, 247, 25, 227, 15, 24, 230, 16, 24, 15, 24, 72, 41, 0, 47, 16, 0,
        24, 42, 232, 0,
    ];
    let frame = decode_positional_cylinder_frame(&zero_support, &scalar::ScalarCache::default())
        .expect("complete zero-support positional cylinder");
    assert_eq!(frame.origin, [-12.5, 4.0, 0.0]);
    assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 0.75);
    assert_eq!(frame.length, Some(8.0));

    let signed_zero_support = [
        17, 72, 32, 0, 19, 24, 47, 39, 128, 72, 16, 0, 67, 232, 0, 47, 42, 128, 47, 16, 0, 25, 154,
        121, 57, 76, 158, 138, 10, 247, 25, 227, 16, 24, 230, 15, 24, 15, 24, 47, 41, 0, 47, 16, 0,
        24, 42, 232, 0,
    ];
    let frame =
        decode_positional_cylinder_frame(&signed_zero_support, &scalar::ScalarCache::default())
            .expect("complete signed zero-support positional cylinder");
    assert_eq!(frame.origin, [12.5, 4.0, 0.0]);
    assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
    assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 0.75);
    assert_eq!(frame.length, Some(8.0));

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
    assert_eq!(frame.origin, [0.0, 0.0, 0.0]);
    assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert!((frame.radius - 4.45).abs() < 1.0e-12);
    assert_eq!(frame.length, Some(16.0));

    let reversed_referenced_planar_envelope = [
        17, 24, 19, 46, 17, 255, 71, 19, 204, 70, 48, 189, 112, 163, 215, 10, 62, 50, 197, 215, 53,
        172, 2, 203, 123, 46, 19, 204, 70, 40, 122, 225, 71, 174, 20, 125, 46, 19, 204, 247, 25,
    ];
    let frame = decode_positional_cylinder_frame(
        &reversed_referenced_planar_envelope,
        &scalar::ScalarCache::default(),
    )
    .expect("complete reversed referenced planar-envelope cylinder");
    assert!((frame.origin[0]).abs() < 1.0e-12);
    assert!((frame.origin[1] + 12.24).abs() < 1.0e-12);
    assert_eq!(frame.origin[2], 0.0);
    assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
    assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert!((frame.radius - 4.95).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent") - 4.5).abs() < 1.0e-12);

    let held_axis = [
        17, 24, 19, 15, 70, 68, 166, 102, 102, 102, 102, 102, 16, 67, 224, 0, 70, 67, 166, 102,
        102, 102, 102, 102, 25, 161, 166, 38, 51, 20, 92, 7, 14, 247, 23,
    ];
    let frame = decode_positional_cylinder_frame(&held_axis, &scalar::ScalarCache::default())
        .expect("complete held-axis cylinder");
    assert!((frame.origin[0] + 40.3).abs() < 1.0e-12);
    assert_eq!(frame.origin[1], 0.0);
    assert!((frame.origin[2] + 0.5).abs() < 1.0e-12);
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
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
    assert!((frame.origin[0] - 29.8).abs() < 1.0e-12);
    assert_eq!(frame.origin[1..], [0.0, 0.0]);
    assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 0.0, -1.0]);
    assert!((frame.radius - 1.0).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent") - 6.528_189_135_889_739).abs() < 1.0e-12);

    let second_endpoint_axial_radial = [
        17, 24, 19, 45, 26, 27, 232, 154, 196, 109, 12, 70, 66, 41, 227, 121, 190, 244, 8, 66, 239,
        255, 16, 71, 61, 204, 25, 192, 139, 195, 207, 227, 22, 71, 15, 247, 23,
    ];
    let frame = decode_positional_cylinder_frame(
        &second_endpoint_axial_radial,
        &scalar::ScalarCache::default(),
    )
    .expect("complete second-endpoint axial/radial cylinder");
    assert!((frame.origin[0] + 29.8).abs() < 1.0e-12);
    assert_eq!(frame.origin[1..], [0.0, 0.0]);
    assert_eq!(frame.axis, [-1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
    assert!((frame.radius - 1.0).abs() < 1.0e-12);
    assert!((frame.length.expect("axial extent") - 6.527_254_503_477_945).abs() < 1.0e-12);

    let mut inconsistent = negative_x.to_vec();
    inconsistent[58] = 0xd0;
    assert!(
        decode_positional_cylinder_frame(&inconsistent, &scalar::ScalarCache::default()).is_none()
    );
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
        Some(PositionalCylinderFrame {
            origin: [-12.5, 4.0, 0.0],
            axis: [0.0, 1.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 0.75,
            length: Some(34.0),
        })
    );
    assert_eq!(
        decode_positional_cylinder_frame(&split, &cache),
        Some(PositionalCylinderFrame {
            origin: [12.5, 4.0, 0.0],
            axis: [0.0, 1.0, 0.0],
            ref_direction: [-1.0, 0.0, 0.0],
            radius: 0.75,
            length: Some(34.0),
        })
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
        Some(PositionalCylinderFrame {
            origin: [-15.0, 24.0, 6.0],
            axis: [0.0, 0.0, -1.0],
            ref_direction: [-1.0, 0.0, 0.0],
            radius: 2.0,
            length: Some(12.0),
        })
    );

    let middle_left = [
        17, 72, 33, 0, 19, 24, 72, 50, 128, 47, 52, 128, 71, 23, 255, 72, 39, 0, 47, 56, 0, 47, 4,
        0, 247, 25,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&middle_left, &cache),
        Some(PositionalCylinderFrame {
            origin: [-15.0, 24.0, 2.5],
            axis: [0.0, 0.0, -1.0],
            ref_direction: [-1.0, 0.0, 0.0],
            radius: 3.5,
            length: Some(8.5),
        })
    );

    let outer_right = [
        17, 47, 33, 0, 19, 47, 40, 0, 47, 42, 0, 47, 54, 0, 47, 4, 0, 47, 49, 0, 47, 56, 0, 47, 24,
        0,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&outer_right, &cache),
        Some(PositionalCylinderFrame {
            origin: [15.0, 24.0, -6.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 2.0,
            length: Some(12.0),
        })
    );

    let terminal_zero_negative = [
        17, 72, 89, 0, 19, 24, 72, 117, 104, 72, 104, 16, 72, 89, 0, 72, 115, 56, 72, 101, 224, 24,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&terminal_zero_negative, &cache),
        Some(PositionalCylinderFrame {
            origin: [-325.0, -175.0, 0.0],
            axis: [0.0, 0.0, -1.0],
            ref_direction: [-1.0, 0.0, 0.0],
            radius: 17.5,
            length: Some(100.0),
        })
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
        Some(PositionalCylinderFrame {
            origin: [-22.0, 5.499_999_999_999_9, -9.25],
            axis: [0.0, 1.0, 0.0],
            ref_direction: [-1.0, 0.0, 0.0],
            radius: 1.75,
            length: Some(2.0),
        })
    );

    let reversed = [
        17, 72, 0, 0, 19, 24, 47, 52, 64, 70, 29, 255, 255, 255, 255, 255, 143, 72, 38, 0, 47, 55,
        192, 70, 21, 255, 255, 255, 255, 255, 143, 72, 34, 128, 247, 23,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&reversed, &cache),
        Some(PositionalCylinderFrame {
            origin: [22.0, 7.499_999_999_999_9, -9.25],
            axis: [0.0, -1.0, 0.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius: 1.75,
            length: Some(2.0),
        })
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
        assert!(frame.axis[1].abs() < 1.0e-12);
        assert_eq!(frame.ref_direction, [0.0, -1.0, 0.0]);
        assert!(frame.length.is_some_and(|length| length > 17.0));
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
        assert_eq!(frame.origin, [0.0, 0.0, 0.0]);
        assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
        assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
        assert!((frame.radius - 6.9).abs() < 1.0e-12);
        assert!(frame
            .length
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
        assert_eq!(frame.origin, [0.0, 17.5, 0.0]);
        assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
        assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
        assert!((frame.radius - expected_radius).abs() < 1.0e-12);
        assert!(frame
            .length
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
        Some(PositionalCylinderFrame {
            origin: [30.0, 0.0, 5.5],
            axis: [-1.0, 0.0, 0.0],
            ref_direction: [0.0, 0.0, 1.0],
            radius: 11.0,
            length: Some(0.199_999_999_999_999_98),
        })
    );

    let negative_end = [
        17, 66, 201, 153, 19, 24, 72, 62, 0, 72, 22, 0, 228, 71, 61, 204, 25, 210, 51, 87, 100,
        172, 254, 232, 47, 22, 0, 247, 23,
    ];
    assert_eq!(
        decode_positional_cylinder_frame(&negative_end, &cache),
        Some(PositionalCylinderFrame {
            origin: [-29.799_999_999_999_997, 0.0, 5.5],
            axis: [-1.0, 0.0, 0.0],
            ref_direction: [0.0, 0.0, 1.0],
            radius: 11.0,
            length: Some(0.199_999_999_999_999_98),
        })
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
    assert_eq!(frame.origin[0], -10.0);
    assert!((frame.origin[1] - 7.986_629_6).abs() < 1.0e-12);
    assert_eq!(frame.origin[2], 5.0);
    assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 1.0);
    assert!((frame.length.expect("axial extent") - 30.013_370_4).abs() < 1.0e-12);

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
    assert!((frame.origin[0] - 7.021_843_6).abs() < 1.0e-12);
    assert_eq!(frame.origin[1..], [5.0, 5.0]);
    assert_eq!(frame.axis, [-1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 1.0);
    assert!((frame.length.expect("axial extent") - 14.021_843_6).abs() < 1.0e-12);

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
    assert_eq!(frame.origin[0..2], [12.0, 34.0]);
    assert!((frame.origin[2] + 21.658_843_825_753_03).abs() < 1.0e-12);
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert!((frame.ref_direction[0] + 0.6).abs() < 1.0e-12);
    assert!((frame.ref_direction[1] + 0.8).abs() < 1.0e-12);
    assert_eq!(frame.ref_direction[2], 0.0);
    assert_eq!(frame.radius, 5.0);
    assert_eq!(frame.length, None);
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    assert_eq!(
        parameter_records(&payload)[0].type24_round_radius(0x24),
        None
    );

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
        length: 1,
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
fn positional_cone_frame_requires_complete_support_apex_and_angle() {
    let body = [
        197, 251, 126, 24, 209, 212, 112, 107, 81, 235, 133, 30, 184, 70, 125, 251, 126, 24, 209,
        212, 112, 123, 0, 68, 204, 99, 17, 228, 72, 66, 64, 192, 170, 175, 125, 232, 45, 177, 195,
        0, 68, 204, 99, 17, 220, 70, 66, 1, 69, 135, 177, 98, 82, 120, 170, 175, 125, 232, 45, 187,
        65, 200, 122, 225, 71, 174, 20, 128, 227, 24, 228, 15, 24, 15, 24, 16, 24, 228, 70, 66,
        129, 71, 174, 20, 122, 225, 25, 194, 145, 29, 33, 143, 32, 210, 52, 233, 0, 116, 33, 251,
        84, 68, 45, 5,
    ];
    let frame = decode_positional_cone_frame(&body, &scalar::ScalarCache::default())
        .expect("complete positional cone");
    assert_eq!(frame.apex, [37.01, 0.0, 0.0]);
    assert_eq!(frame.axis, [-1.0, -0.0, -0.0]);
    assert_eq!(frame.ref_direction, [-0.0, -0.0, -1.0]);
    assert!((frame.half_angle - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12);

    let angle = terminal_cone_half_angle_layout(&body).expect("terminal half-angle");
    let mut local_system_body = vec![0xf9, 0x04, 0x03];
    local_system_body.extend_from_slice(&body[..angle.start]);
    let prototype = SurfacePrototypeRecord {
        declared_family: "cone".to_string(),
        family: SurfacePrototypeFamily::Cone,
        parameters: vec![
            SurfaceNamedParameter {
                name: "local_sys".to_string(),
                value: SurfaceNamedValue::Opaque(local_system_body.clone()),
                body: local_system_body,
                offset: 0,
                value_offset: 0,
            },
            SurfaceNamedParameter {
                name: "half_angle".to_string(),
                value: SurfaceNamedValue::ScalarSequence(vec![angle.value]),
                body: body[angle.start..].to_vec(),
                offset: 0,
                value_offset: 0,
            },
        ],
        offset: 0,
    };
    assert_eq!(prototype_cone_frame(&prototype), Some(frame));

    let mut incomplete = body.to_vec();
    incomplete.remove(86);
    assert!(decode_positional_cone_frame(&incomplete, &scalar::ScalarCache::default()).is_none());
}

#[test]
fn positional_cone_frame_decodes_complete_planar_envelopes() {
    let unreferenced = [
        21, 70, 34, 171, 89, 29, 204, 62, 140, 24, 70, 28, 153, 105, 188, 41, 208, 189, 71, 27,
        153, 70, 40, 122, 225, 71, 174, 20, 126, 24, 46, 27, 153, 70, 36, 28, 61, 7, 246, 190, 80,
        46, 27, 153,
    ];
    let referenced = [
        23, 70, 34, 171, 89, 29, 204, 62, 140, 21, 70, 28, 153, 105, 188, 41, 208, 189, 71, 27,
        153, 70, 40, 122, 225, 71, 174, 20, 126, 71, 27, 153, 46, 27, 153, 70, 36, 28, 61, 7, 246,
        190, 80, 25, 206, 113, 206, 177, 182, 81, 244, 247, 44,
    ];
    for body in [&unreferenced[..], &referenced[..]] {
        let frame = decode_positional_cone_frame(body, &scalar::ScalarCache::default())
            .expect("complete planar-envelope cone");
        assert_eq!(frame.apex[0], 0.0);
        assert!((frame.apex[1] + 19.389_817_409_565_175).abs() < 1.0e-12);
        assert_eq!(frame.apex[2], 0.0);
        assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
        assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
        assert!((frame.half_angle - 0.636_540_466_818_335).abs() < 1.0e-12);
    }

    let mut inconsistent = unreferenced;
    inconsistent[43] = 0x98;
    assert!(decode_positional_cone_frame(&inconsistent, &scalar::ScalarCache::default()).is_none());
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
        .type26_five_coordinate_envelope(0x26)
        .expect("complete five-coordinate envelope");
    assert_eq!(envelope.offset, 7);
    assert_eq!(envelope.values[0], -2.65);
    assert!((envelope.values[1] + 15.0).abs() < 1.0e-12);
    assert_eq!(envelope.values[2], -2.65);
    assert_eq!(envelope.values[3], 2.65);
    assert!((envelope.values[4] + 17.65).abs() < 1.0e-12);

    payload[6] = 0x17;
    assert!(parameter_records(&payload)[0]
        .type26_five_coordinate_envelope(0x26)
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
        replay.type26_replayed_minor_radius(0x26, 0.199_999_999_999_999_98),
        Some(0.199_999_999_999_999_98)
    );
    assert!(replay.type26_replayed_minor_radius(0x26, 0.2).is_none());
    assert!(replay
        .type26_replayed_minor_radius(0x25, 0.199_999_999_999_999_98)
        .is_none());

    let two_slot_terminal = record(&[0xe4, 0x29, 0xc9, 0x99]);
    assert_eq!(
        two_slot_terminal.type26_replayed_minor_radius(0x26, 0.199_999_999_999_999_98),
        Some(0.199_999_999_999_999_98)
    );
    let nonterminal_match = record(&[0x29, 0xc9, 0x99, 0xe4]);
    assert!(nonterminal_match
        .type26_replayed_minor_radius(0x26, 0.199_999_999_999_999_98)
        .is_none());
    let tagged_override = record(&[
        0x18, 0x0d, 0x29, 0xc9, 0x99, 0x00, 0x0e, 0x01, 0x29, 0xdf, 0xff,
    ]);
    assert!(tagged_override
        .type26_replayed_minor_radius(0x26, 0.199_999_999_999_999_98)
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
            .type26_five_coordinate_envelope(0x26)
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
        .type26_five_coordinate_envelope(0x26)
        .expect("direct torus envelope");
    assert!(direct
        .values
        .iter()
        .zip([-4.95, 17.24, -4.95, 4.95, 16.74])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    let split = record(&split_tail)
        .type26_split_coordinate_envelope(0x26)
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
        .positional_torus_frame
        .expect("complete positional torus frame");
    assert!(frame
        .center
        .into_iter()
        .zip([1.0, 16.74, 0.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert!(frame
        .axis
        .into_iter()
        .zip([0.0, 0.0, 1.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert!(frame
        .ref_direction
        .into_iter()
        .zip([-0.999_899_554_583_406_1, 0.014_173_240_416_574_131, 0.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert!((frame.major_radius - 4.45).abs() < 1.0e-12);
    assert!((frame.minor_radius - 0.5).abs() < 1.0e-12);

    payload[55] = 0x20;
    assert!(parameter_records(&payload)[0]
        .positional_torus_frame
        .is_none());
    payload[55] = body[49];
    payload[102] = 0x0d;
    assert!(parameter_records(&payload)[0]
        .positional_torus_frame
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
    assert!((panel.type24_round_radius(0x24).expect("required invariant") - 0.2).abs() < 1.0e-12);
    let frame = panel
        .positional_cylinder_frame
        .expect("complete repeated-diameter carrier");
    assert_eq!(frame.origin, [2.2, -22.35, -1.45]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert!((frame.radius - 0.2).abs() < 1.0e-12);
    assert!((frame.length.expect("required invariant") - 46.241_026_156_433_854).abs() < 1.0e-12);
    assert!((frame.axis[1] - 46.15 / frame.length.expect("required invariant")).abs() < 1.0e-12);
    assert!((frame.axis[2] - 2.9 / frame.length.expect("required invariant")).abs() < 1.0e-12);
    assert!(
        (record(&prefixed_panel)
            .type24_round_radius(0x24)
            .expect("required invariant")
            - 0.2)
            .abs()
            < 1.0e-12
    );
    assert!(
        (record(&separated)
            .type24_round_radius(0x24)
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
    assert!(replay_record
        .type24_terminal_corner_envelope(0x24)
        .is_some());
    assert!(replay_record
        .type24_terminal_corner_envelope(0x22)
        .is_none());
    let mut compound_close = replay_separated;
    *compound_close.last_mut().expect("trailer") = 0x17;
    assert_eq!(
        record(&compound_close).type24_terminal_corner_envelope(0x24),
        replay_record.type24_terminal_corner_envelope(0x24)
    );
    let replay_frame = replay_record
        .positional_cylinder_frame
        .expect("replay-trailed repeated-diameter carrier");
    assert_eq!(
        replay_frame,
        PositionalCylinderFrame {
            origin: [9.0, 23.0, 5.0],
            axis: [1.0 / 2.0_f64.sqrt(), 0.0, 1.0 / 2.0_f64.sqrt()],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 15.0,
            length: Some(2.0_f64.sqrt()),
        }
    );
    let selector_corner_interval = [
        0x12, 0x2d, 0x40, 0x7a, 0x35, 0xc4, 0x3e, 0x21, 0x5b, 0x11, 0x2d, 0x44, 0xff, 0xd2, 0xa6,
        0xae, 0x74, 0x2b, 0x46, 0x65, 0x3f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x51, 0xd2, 0x31,
        0x1a, 0xfa, 0xb7, 0x82, 0x48, 0x28, 0x00, 0x46, 0x64, 0x1f, 0xff, 0xff, 0xff, 0xff, 0xfc,
        0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xea, 0x48, 0x08, 0x00,
    ];
    let selector_corner_record = record(&selector_corner_interval);
    assert!(selector_corner_record
        .selector_corner_interval_cylinder_frame(0x24)
        .is_some());
    assert!(selector_corner_record
        .selector_corner_interval_cylinder_frame(0x22)
        .is_none());
    let selector_corner_frame = selector_corner_record
        .positional_cylinder_frame
        .expect("selector-corner interval carrier");
    assert!((selector_corner_frame.origin[0] + 161.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!(
        (selector_corner_frame.origin[1] - 38.329_481_329_444_5).abs() < EPS_CYLINDER_GEOMETRY_MIN
    );
    assert!((selector_corner_frame.origin[2] + 3.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert_eq!(selector_corner_frame.axis, [0.0, 1.0, 0.0]);
    assert_eq!(selector_corner_frame.ref_direction, [1.0, 0.0, 0.0]);
    assert!((selector_corner_frame.radius - 9.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!(selector_corner_frame.length.is_some_and(|length| {
        (length - 9.043_850_235_791_638).abs() < EPS_CYLINDER_GEOMETRY_MIN
    }));
    let mut referenced_controls = selector_corner_interval.to_vec();
    referenced_controls.extend_from_slice(&[0xf7, 0x40]);
    assert!(record(&referenced_controls)
        .positional_cylinder_frame
        .is_some());
    let mut invalid_control = selector_corner_interval;
    invalid_control[0] = 0x15;
    assert!(record(&invalid_control).positional_cylinder_frame.is_none());
    invalid_control = selector_corner_interval;
    invalid_control[9] = 0x15;
    assert!(record(&invalid_control).positional_cylinder_frame.is_none());
    let prefixed_auxiliary = [
        0x19, 0xd3, 0xae, 0x70, 0x14, 0x6d, 0xb6, 0xde, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad, 0x2a,
        0x4e, 0x12, 0x2d, 0x4f, 0x01, 0x49, 0xdf, 0x84, 0xdb, 0x36, 0x48, 0x58, 0xc0, 0x2d, 0x57,
        0x75, 0x9c, 0xe9, 0x32, 0x3b, 0xfb, 0x48, 0x24, 0x00, 0x48, 0x57, 0x00, 0x2d, 0x59, 0x15,
        0xbb, 0x28, 0x9e, 0x14, 0x6f, 0x48, 0x08, 0x00, 0xf7, 0x40,
    ];
    let prefixed_frame = record(&prefixed_auxiliary)
        .positional_cylinder_frame
        .expect("selector-prefixed auxiliary repeated-diameter carrier");
    assert!((prefixed_frame.radius - 3.250_923_087_748_478).abs() < 1.0e-12);
    assert_eq!(prefixed_frame.ref_direction, [0.0, -1.0, 0.0]);
    assert!(prefixed_frame
        .axis
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
        .positional_cylinder_frame
        .is_some());
    let mut invalid_selector = prefixed_auxiliary;
    invalid_selector[0] = 0x18;
    assert!(record(&invalid_selector)
        .positional_cylinder_frame
        .is_none());
    let split_controls = [
        0x14, 0x2d, 0x4b, 0xc1, 0x0d, 0x60, 0xad, 0x2a, 0x4f, 0x00, 0x13, 0x1a, 0x2d, 0x4f, 0x01,
        0x49, 0xdf, 0x84, 0xdb, 0x35, 0x48, 0x58, 0xc0, 0x2d, 0x57, 0x75, 0x9c, 0xe9, 0x32, 0x3b,
        0xfc, 0x92, 0xff, 0xff, 0xff, 0xff, 0xff, 0xe8, 0x48, 0x57, 0x00, 0x2d, 0x59, 0x15, 0xbb,
        0x28, 0x9e, 0x14, 0x6e, 0x2f, 0x24, 0x00, 0xf7, 0x40,
    ];
    let split_frame = record(&split_controls)
        .positional_cylinder_frame
        .expect("split selector-corner interval carrier");
    assert!((split_frame.origin[0] + 99.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!((split_frame.origin[1] - 38.329_481_329_444_49).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!((split_frame.origin[2] - 3.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert_eq!(split_frame.axis, [0.0, 1.0, 0.0]);
    assert_eq!(split_frame.ref_direction, [1.0, 0.0, 0.0]);
    assert!((split_frame.radius - 7.0).abs() < EPS_CYLINDER_GEOMETRY_MIN);
    assert!(split_frame.length.is_some_and(|length| {
        (length - 6.501_846_175_496_936_6).abs() < EPS_CYLINDER_GEOMETRY_MIN
    }));
    let mut invalid_split_controls = split_controls;
    invalid_split_controls[10] = 0x14;
    assert!(record(&invalid_split_controls)
        .positional_cylinder_frame
        .is_none());
    let prefixed_split_controls = [
        0x00, 0x11, 0x13, 0x2d, 0x41, 0x83, 0x08, 0x72, 0x35, 0x71, 0xa6, 0x14, 0x2d, 0x44, 0xff,
        0xd2, 0xa6, 0xae, 0x74, 0x27, 0x46, 0x64, 0x9f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x52,
        0x56, 0x9a, 0x71, 0xf6, 0x5f, 0xa7, 0x92, 0xff, 0xff, 0xff, 0xff, 0xff, 0xeb, 0x46, 0x64,
        0x1f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xe8, 0x2f,
        0x1c, 0x00, 0xf7, 0x40,
    ];
    assert!(record(&prefixed_split_controls)
        .positional_cylinder_frame
        .is_some());
    let mut invalid_prefix = prefixed_split_controls;
    invalid_prefix[2] = 0x12;
    assert!(record(&invalid_prefix).positional_cylinder_frame.is_none());
    let positive_integer_extent = [
        0x12, 0x2d, 0x41, 0x83, 0x08, 0x72, 0x35, 0x71, 0xa2, 0x00, 0x11, 0x13, 0x2d, 0x44, 0xff,
        0xd2, 0xa6, 0xae, 0x74, 0x2a, 0x46, 0x64, 0x9f, 0xff, 0xff, 0xff, 0xff, 0xfc, 0x2d, 0x52,
        0x56, 0x9a, 0x71, 0xf6, 0x5f, 0xa5, 0x48, 0x1c, 0x00, 0x46, 0x64, 0x1f, 0xff, 0xff, 0xff,
        0xff, 0xfc, 0x2d, 0x54, 0x14, 0xff, 0x8c, 0x32, 0xe0, 0xe9, 0xda, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x15,
    ];
    assert!(record(&positive_integer_extent)
        .positional_cylinder_frame
        .is_some());
    let mut invalid_integer_controls = positive_integer_extent;
    invalid_integer_controls[10] = 0x12;
    assert!(record(&invalid_integer_controls)
        .positional_cylinder_frame
        .is_none());

    let equal_span = record(&[
        24, 45, 47, 73, 81, 130, 169, 147, 32, 18, 45, 49, 164, 168, 193, 84, 201, 144, 47, 12, 0,
        47, 32, 0, 72, 24, 0, 47, 22, 0, 47, 36, 0, 72, 16, 0,
    ]);
    assert_eq!(
        equal_span.type24_scalar_frame_round_envelope(0x24),
        Some(Type24RoundEnvelope {
            diameter: 2.0,
            extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
        })
    );
    assert!(equal_span.positional_cylinder_frame.is_none());

    let mut inconsistent = separated;
    inconsistent[31..34].copy_from_slice(&[0x2f, 0x12, 0x00]);
    assert!(record(&inconsistent).type24_round_radius(0x24).is_none());

    let first_coordinate = [
        0x4c, 0xb7, 0x67, 0xe1, 0x01, 0x3f, 0x80, 0x2d, 0x31, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87,
        0x12, 0x2d, 0x35, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x2f, 0x22, 0x00, 0x2f, 0x43, 0x00,
        0x48, 0x10, 0x00, 0x2d, 0x32, 0x4e, 0xfa, 0x22, 0xce, 0x34, 0xea, 0x2d, 0x47, 0xfc, 0xef,
        0xa2, 0x2c, 0xe3, 0x4f, 0x18,
    ];
    let first_coordinate = record(&first_coordinate);
    let frame = first_coordinate
        .positional_cylinder_frame
        .expect("complete first-coordinate round carrier");
    assert_eq!(frame.origin, [9.0, 38.0, -2.0]);
    assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 2.0);
    let length = frame.length.expect("bounded axial span");
    let expected_length = 9.308_504_271_834_785_f64.hypot(9.976_063_033_979_35);
    assert!((length - expected_length).abs() < 1.0e-12);
    assert!((frame.axis[0] - 9.308_504_271_834_785 / length).abs() < 1.0e-12);
    assert!((frame.axis[1] - 9.976_063_033_979_35 / length).abs() < 1.0e-12);
    assert_eq!(first_coordinate.type24_round_radius(0x24), Some(2.0));

    let mut wrong_close = first_coordinate.body.clone();
    wrong_close[49] = 0x19;
    assert!(record(&wrong_close).positional_cylinder_frame.is_none());

    let opposite = [
        0x4c, 0xb7, 0x67, 0xe1, 0x01, 0x3f, 0x80, 0x2d, 0x35, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87,
        0x12, 0x2d, 0x39, 0xa4, 0xa8, 0xc1, 0x54, 0xc9, 0x87, 0x46, 0x32, 0x4e, 0xfa, 0x22, 0xce,
        0x34, 0xea, 0x2f, 0x43, 0x00, 0x48, 0x10, 0x00, 0x48, 0x22, 0x00, 0x2d, 0x47, 0xfc, 0xef,
        0xa2, 0x2c, 0xe3, 0x4f, 0x18,
    ];
    let opposite = record(&opposite)
        .positional_cylinder_frame
        .expect("opposite first-coordinate round carrier");
    assert_eq!(opposite.origin, [-18.308_504_271_834_785, 38.0, -2.0]);
    assert_eq!(opposite.radius, 2.0);
    assert!((opposite.length.expect("required invariant") - expected_length).abs() < 1.0e-12);

    let segmented = [
        0x18, 0x2d, 0x35, 0xa8, 0xa9, 0xfd, 0x2c, 0xc7, 0xe2, 0x70, 0xbf, 0xe3, 0x4f, 0x05, 0x11,
        0x10, 0x2d, 0x3a, 0xc5, 0x1b, 0xc4, 0x49, 0x39, 0xa9, 0x46, 0x20, 0x38, 0xe3, 0x8e, 0x38,
        0xe3, 0x8e, 0x2f, 0x41, 0x00, 0x18, 0x48, 0x1c, 0x00, 0x2d, 0x42, 0x92, 0x43, 0xe3, 0x8f,
        0xf2, 0x60, 0x9f, 0x71, 0xc7, 0x1c, 0x71, 0xc7, 0x1c, 0xf7, 0x19,
    ];
    let segmented = record(&segmented);
    let frame = segmented
        .positional_cylinder_frame
        .expect("complete segmented first-coordinate round carrier");
    let diameter = 5.111_111_111_111_111;
    assert_eq!(frame.origin, [-8.111_111_111_111_11, 34.0, 0.5 * diameter]);
    assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius, 0.5 * diameter);
    let expected_length = 1.111_111_111_111_110_7_f64.hypot(3.142_696_805_273_545);
    assert!((frame.length.expect("required invariant") - expected_length).abs() < 1.0e-12);
    assert_eq!(segmented.type24_round_radius(0x24), Some(0.5 * diameter));

    let mut wrong_separator = segmented.body.clone();
    wrong_separator[9] = 0x71;
    assert!(record(&wrong_separator).positional_cylinder_frame.is_none());

    let split_coordinate = [
        24, 45, 49, 164, 168, 193, 84, 201, 133, 18, 45, 53, 164, 168, 193, 84, 201, 136, 47, 0, 0,
        47, 34, 0, 52, 240, 0, 47, 28, 0, 47, 44, 0, 47, 16, 0,
    ];
    let split_frame = record(&split_coordinate)
        .positional_cylinder_frame
        .expect("split first-coordinate round carrier");
    assert_eq!(split_frame.origin, [2.0, 9.0, 2.0]);
    assert_eq!(
        split_frame.axis,
        [1.0 / 2.0_f64.sqrt(), 1.0 / 2.0_f64.sqrt(), 0.0]
    );
    assert_eq!(split_frame.ref_direction, [0.0, 0.0, 1.0]);
    assert!((split_frame.radius - 2.0).abs() < 1.0e-12);
    assert_eq!(split_frame.length, Some(50.0_f64.sqrt()));
    assert!(
        (record(&split_coordinate)
            .type24_round_radius(0x24)
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
        .positional_cylinder_frame
        .expect("opposite split first-coordinate round carrier");
    assert_eq!(opposite_frame.origin, [-7.0, 9.0, 2.0]);
    assert_eq!(opposite_frame.axis, split_frame.axis);
    assert!((opposite_frame.radius - 2.0).abs() < 1.0e-12);

    let mut incomplete_split = split_coordinate;
    incomplete_split[24] = 0x18;
    assert!(record(&incomplete_split)
        .positional_cylinder_frame
        .is_none());
}

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
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };

    assert_eq!(
        record.type24_round_edge_envelope(0x24),
        Some(Type24RoundEdgeEnvelope {
            parameter_interval: [
                f64::from_be_bytes([0x3f, 0xcb, 0, 0, 0, 0, 0, 0]),
                f64::from_be_bytes([0x3f, 0xe0, 0, 0, 0, 0, 0, 0]),
            ],
            vertices: [[0.0, 1.0, 2.0], [-1.0, 2.0, 0.0]],
            generated_entity_reference: Some(0x17),
        })
    );
    assert!(record.type24_round_edge_envelope(0x25).is_none());
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
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let envelope = parameter
        .type24_round_edge_envelope(0x24)
        .expect("complete model-reference-shell round envelope");

    assert_eq!(envelope.parameter_interval, [0.0, 1.0]);
    assert_eq!(envelope.vertices, [[2.0, -3.0, 0.0], [4.0, -5.0, 1.0]]);

    let mut truncated = parameter;
    truncated.body.remove(7);
    assert!(truncated.type24_round_edge_envelope(0x24).is_none());
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
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let envelope = parameter
        .type24_round_edge_envelope(0x24)
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
    let expected = PositionalCylinderFrame {
        origin: [4.0, 5.0, 2.0],
        axis: [0.0, 0.0, 1.0],
        ref_direction: [1.0, 0.0, 0.0],
        radius: 2.0,
        length: Some(4.0),
    };
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
        .positional_cylinder_frame
        .expect("complete square-radial carrier");
    assert_eq!(frame.origin, [-94.5, -93.837_702_082_688_25, -7.5]);
    assert_eq!(frame.axis, [0.0, -1.0, 0.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
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
        .positional_cylinder_frame
        .expect("control-terminated square-radial carrier");
    assert!(frame
        .origin
        .into_iter()
        .zip([-27.643_2, -14.0, 43.0])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 1.0, 0.0]);
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
        .positional_cylinder_frame
        .expect("complete six-slot square-radial carrier");
    assert!(frame
        .origin
        .into_iter()
        .zip([-29.9, -7.4, -3.2])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
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
        .positional_cylinder_frame
        .expect("complete nine-slot square-radial carrier");
    assert!(frame
        .origin
        .into_iter()
        .zip([-9.5, 8.0, 5.5])
        .all(|(actual, expected)| (actual - expected).abs() < 1.0e-12));
    assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
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
        .positional_cylinder_frame
        .expect("complete single-diameter carrier");
    assert_eq!(frame.origin, [-192.5, -4.0, 27.5]);
    assert_eq!(
        frame.axis,
        [2.0 / 5.0_f64.sqrt(), 0.0, 1.0 / 5.0_f64.sqrt()]
    );
    assert_eq!(frame.ref_direction, [0.0, 1.0, 0.0]);
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
    assert!(collision.positional_cylinder_frame.is_none());

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
        .positional_cylinder_frame
        .expect("complete zero-axial square-radial carrier");
    assert!((frame.origin[0] - 29.8).abs() < 1.0e-12);
    assert!((frame.origin[1] - 5.6).abs() < 1.0e-12);
    assert!((frame.origin[2] - 7.9).abs() < 1.0e-12);
    assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, -1.0, 0.0]);
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
        .positional_cylinder_frame
        .expect("complete signed-DICT repeated-diameter carrier");

    assert_eq!(frame.origin, [-42.3, 1.25, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
    assert!((frame.radius - 0.3).abs() < 1.0e-12);
    let length = 85.1_f64.hypot(0.5);
    assert!((frame.length.expect("required invariant") - length).abs() < 1.0e-12);
    assert!((frame.axis[0] - 85.1 / length).abs() < 1.0e-12);
    assert!((frame.axis[1] - 0.5 / length).abs() < 1.0e-12);
    assert_eq!(frame.axis[2], 0.0);
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
        .positional_cylinder_frame
        .expect("complete prefixed repeated-diameter carrier");

    assert_eq!(frame.origin[0], -42.3);
    assert!((frame.origin[1] + 1.75).abs() < 1.0e-12);
    assert_eq!(frame.origin[2], 0.0);
    assert_eq!(frame.ref_direction, [0.0, 0.0, 1.0]);
    assert!((frame.radius - 0.3).abs() < 1.0e-12);
    let length = 85.1_f64.hypot(0.5);
    assert!((frame.length.expect("required invariant") - length).abs() < 1.0e-12);
    assert!((frame.axis[0] - 85.1 / length).abs() < 1.0e-12);
    assert!((frame.axis[1] - 0.5 / length).abs() < 1.0e-12);
    assert_eq!(frame.axis[2], 0.0);

    let mut wrong_prefix = body;
    wrong_prefix[1] = 0xbb;
    assert!(record(&wrong_prefix).positional_cylinder_frame.is_none());
    let mut wrong_separator = body;
    wrong_separator[13] = 0x13;
    assert!(record(&wrong_separator).positional_cylinder_frame.is_none());
    assert!(record(&body[..body.len() - 7])
        .positional_cylinder_frame
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
        .positional_cylinder_frame
        .expect("complete held-coordinate round carrier");

    assert_eq!(frame.origin, [34.0, 5.0, 10.0]);
    assert_eq!(frame.axis, [1.0, 0.0, 0.0]);
    assert_eq!(frame.ref_direction, [0.0, 1.0, 0.0]);
    assert_eq!(frame.radius, 1.0);
    assert_eq!(frame.length, Some(4.0));
    assert_eq!(base_record.type24_round_radius(0x24), Some(1.0));

    let replay_body = [
        24, 45, 79, 146, 110, 151, 141, 79, 224, 120, 172, 103, 5, 97, 187, 80, 45, 84, 73, 55, 75,
        198, 167, 240, 72, 34, 0, 47, 65, 0, 47, 16, 0, 47, 34, 0, 47, 67, 0, 47, 24, 0, 247, 24,
    ];
    let replay = record(&replay_body);
    assert_eq!(
        replay.positional_cylinder_frame,
        Some(PositionalCylinderFrame {
            origin: [34.0, 5.0, 9.0],
            axis: [1.0, 0.0, 0.0],
            ref_direction: [0.0, 1.0, 0.0],
            radius: 1.0,
            length: Some(4.0),
        })
    );
    assert_eq!(replay.type24_round_radius(0x24), Some(1.0));
    assert_eq!(
        record(&replay_body[..replay_body.len() - 2]).positional_cylinder_frame,
        replay.positional_cylinder_frame,
    );

    let mut broken_replay = replay_body;
    broken_replay[43] = 0x19;
    assert!(record(&broken_replay).positional_cylinder_frame.is_none());

    let mut wrong_control = body;
    wrong_control[25] = 0x25;
    assert!(record(&wrong_control).positional_cylinder_frame.is_none());
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
            .type24_round_radius(0x24)
            .expect("required invariant")
            - 0.3)
            .abs()
            < 1.0e-12
    );

    let mut replay_terminated = terminal.to_vec();
    replay_terminated.extend_from_slice(&[0xf7, 0x17]);
    assert!(
        (record(&replay_terminated)
            .type24_round_radius(0x24)
            .expect("required invariant")
            - 0.3)
            .abs()
            < 1.0e-12
    );

    let mut trailing_payload = terminal.to_vec();
    trailing_payload.push(0x18);
    assert!(record(&trailing_payload)
        .type24_round_radius(0x24)
        .is_none());
    let coordinate_terminal = [
        0x18, 0x2d, 0x45, 0x30, 0x89, 0xa0, 0x27, 0x52, 0x54, 0x12, 0x46, 0x16, 0xd9, 0xc0, 0xeb,
        0x43, 0x76, 0xac,
    ];
    assert!(record(&coordinate_terminal)
        .type24_round_radius(0x24)
        .is_none());
    assert!(record(&terminal).type24_round_radius(0x22).is_none());
}

#[test]
fn summarizes_seven_byte_torus_radius() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\x5e\x33\x33\x33\x33\x33\x2c\xe0\x01radius2\0\x29\xc9\x99\xe3";

    assert!(matches!(
        prototypes(payload).as_slice(),
        [SurfacePrototype {
            kind: SurfaceKind::TorusOrSphere,
            radius: Some(major),
            radius2: Some(minor),
            ..
        }] if (*major - 0.3).abs() < 1.0e-12 && (*minor - 0.2).abs() < 1.0e-12
    ));
}

#[test]
fn scalar_tail_named_marker_does_not_end_prototype_field() {
    let payload = b"srf_prim_ptr(torus)\0\xe0\x01radius1\0\xe4\xe0\x01radius2\0\x71\xe0\0\0\0\0\0\0\xe0\x01c_pnts\0\xf8\0";
    let records = named_prototype_records(payload);
    let radius2 = records[0].field("radius2").expect("radius2 field");

    assert_eq!(radius2.body, [0x71, 0xe0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(
        radius2.value,
        SurfaceNamedValue::ScalarSequence(vec![f64::from_be_bytes(
            [0x3f, 0xe0, 0, 0, 0, 0, 0, 0,]
        )])
    );
}
