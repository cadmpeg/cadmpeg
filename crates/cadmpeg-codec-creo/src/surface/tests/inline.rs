// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::super::*;

fn push_inline_test_scalar(bytes: &mut Vec<u8>, value: f64) {
    match value as i32 {
        -1 => bytes.push(0x0d),
        -7 => bytes.extend_from_slice(&[0x48, 0x1c, 0x00]),
        0 => bytes.push(0x0f),
        1 => bytes.push(0xe4),
        2 => bytes.extend_from_slice(&[0x2f, 0x00, 0x00]),
        3 => bytes.extend_from_slice(&[0x2e, 0x08, 0x00]),
        4 => bytes.extend_from_slice(&[0x2f, 0x10, 0x00]),
        5 => bytes.extend_from_slice(&[0x2f, 0x14, 0x00]),
        6 => bytes.extend_from_slice(&[0x2f, 0x18, 0x00]),
        7 => bytes.extend_from_slice(&[0x2f, 0x1c, 0x00]),
        8 => bytes.extend_from_slice(&[0x2f, 0x20, 0x00]),
        other => panic!("unsupported inline test scalar {other}"),
    }
}

fn push_inline_test_subunit(bytes: &mut Vec<u8>, prefix: u8, value: f64) {
    let raw = value.to_be_bytes();
    assert_eq!(raw[0], 0x3f, "test subunit must be in the positive range");
    bytes.push(prefix);
    bytes.extend_from_slice(&raw[1..]);
}

fn inline_non_plane_row(
    type_byte: u8,
    axial: [f64; 2],
    corners: [[f64; 3]; 2],
    local_system: &[u8],
    suffix: &[u8],
) -> Vec<u8> {
    let mut payload = vec![7, type_byte, 4, 0x01, 0, 0];
    push_inline_test_scalar(&mut payload, 0.0);
    push_inline_test_scalar(&mut payload, axial[0]);
    payload.push(0x12);
    push_inline_test_scalar(&mut payload, axial[1]);
    for corner in corners {
        for value in corner {
            push_inline_test_scalar(&mut payload, value);
        }
    }
    payload.push(0xe3);
    payload.extend_from_slice(local_system);
    payload.extend_from_slice(suffix);
    payload.push(0xe3);
    payload
}

fn inline_non_plane_record(
    type_byte: u8,
    axial: [f64; 2],
    corners: [[f64; 3]; 2],
    local_system: &[u8],
    suffix: &[u8],
) -> SurfaceParameterRecord {
    let mut records = parameter_records(&inline_non_plane_row(
        type_byte,
        axial,
        corners,
        local_system,
        suffix,
    ));
    assert_eq!(records.len(), 1);
    records.remove(0)
}

fn local_system_suffix_row(type_byte: u8, local_system: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut payload = vec![7, type_byte, 4, 0x01, 0, 0];
    payload.extend_from_slice(local_system);
    payload.extend_from_slice(suffix);
    payload.push(0xe3);
    payload
}

fn legacy_planar_cone_suffix_row(local_system: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut payload = vec![7, 0x25, 4, 0x01, 0, 0, 0x17];
    push_inline_test_scalar(&mut payload, 7.0);
    payload.push(0x15);
    push_inline_test_scalar(&mut payload, 3.0);
    payload.extend_from_slice(&[0x48, 0x1c, 0x00]);
    push_inline_test_scalar(&mut payload, 1.0);
    payload.extend_from_slice(&[0x48, 0x1c, 0x00]);
    push_inline_test_scalar(&mut payload, 7.0);
    push_inline_test_scalar(&mut payload, 5.0);
    payload.extend_from_slice(&[0x19, 0, 0, 0, 0, 0, 0, 0, 0xf7, 0x2c, 0xe3]);
    payload.extend_from_slice(local_system);
    payload.extend_from_slice(suffix);
    payload.push(0xe3);
    payload
}

const INLINE_TEST_LOCAL_SYSTEM_Z: [u8; 7] = [0x10, 0x18, 0xe5, 0x10, 0x18, 0xe5, 0x10];

#[test]
fn decodes_inline_non_plane_analytic_carriers_from_witnessed_bodies() {
    let cylinder = inline_non_plane_record(
        0x24,
        [2.0, 4.0],
        [[0.0, 0.0, 6.0], [1.0, 1.0, 8.0]],
        &[
            &INLINE_TEST_LOCAL_SYSTEM_Z[..],
            &[0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x2f, 0x10, 0x00],
        ]
        .concat(),
        &[0x2f, 0x00, 0x00],
    );
    let cylinder_frame = cylinder
        .positional_cylinder_frame
        .expect("witnessed inline cylinder");
    assert_eq!(cylinder_frame.origin, [2.0, 2.0, 4.0]);
    assert_eq!(cylinder_frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(cylinder_frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert_eq!(cylinder_frame.radius, 2.0);
    assert_eq!(cylinder_frame.length, Some(2.0));
    assert_eq!(cylinder.boundary, SurfaceBodyBoundary::CompoundClose);

    let cone = inline_non_plane_record(
        0x25,
        [2.0, 4.0],
        [[0.0, 0.0, 6.0], [1.0, 1.0, 8.0]],
        &[
            &INLINE_TEST_LOCAL_SYSTEM_Z[..],
            &[0x2f, 0x10, 0x00, 0x2f, 0x10, 0x00, 0x2f, 0x10, 0x00],
        ]
        .concat(),
        &[0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x18],
    );
    let cone_frame = cone.positional_cone_frame.expect("witnessed inline cone");
    assert_eq!(cone_frame.apex, [4.0, 4.0, 4.0]);
    assert_eq!(cone_frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(cone_frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert_eq!(cone_frame.half_angle, std::f64::consts::FRAC_PI_4);

    let torus = inline_non_plane_record(
        0x26,
        [2.0, 4.0],
        [[0.0, 0.0, 6.0], [1.0, 1.0, 8.0]],
        &[
            &INLINE_TEST_LOCAL_SYSTEM_Z[..],
            &[0x2f, 0x14, 0x00, 0x2f, 0x14, 0x00, 0x2f, 0x10, 0x00],
        ]
        .concat(),
        &[0x2f, 0x10, 0x00, 0xe4],
    );
    let torus_frame = torus
        .positional_torus_frame
        .expect("witnessed inline torus");
    assert_eq!(torus_frame.center, [5.0, 5.0, 4.0]);
    assert_eq!(torus_frame.major_radius, 4.0);
    assert_eq!(torus_frame.minor_radius, 1.0);

    let sphere = inline_non_plane_record(
        0x26,
        [2.0, 4.0],
        [[0.0, 0.0, 6.0], [1.0, 1.0, 8.0]],
        &[
            &INLINE_TEST_LOCAL_SYSTEM_Z[..],
            &[0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x2f, 0x10, 0x00],
        ]
        .concat(),
        &[0x18, 0x2f, 0x00, 0x00],
    );
    let sphere_frame = sphere
        .positional_torus_frame
        .expect("witnessed inline sphere");
    assert_eq!(sphere_frame.center, [2.0, 2.0, 4.0]);
    assert_eq!(sphere_frame.major_radius, 0.0);
    assert_eq!(sphere_frame.minor_radius, 2.0);
}

#[test]
fn decodes_local_system_suffix_frames_without_an_axial_envelope() {
    let mut explicit = Vec::new();
    push_inline_test_subunit(&mut explicit, 0x41, 0.8);
    push_inline_test_subunit(&mut explicit, 0x41, 0.6);
    explicit.push(0x18);
    push_inline_test_subunit(&mut explicit, 0x28, 0.6);
    push_inline_test_subunit(&mut explicit, 0x41, 0.8);
    explicit.extend_from_slice(&[0x18, 0xe5, 0x0f]);
    push_inline_test_scalar(&mut explicit, -7.0);
    push_inline_test_scalar(&mut explicit, 8.0);
    push_inline_test_scalar(&mut explicit, 5.0);
    let torus = parameter_records(&local_system_suffix_row(
        0x26,
        &explicit,
        &[0x2e, 0x08, 0x00, 0x0f],
    ))
    .remove(0);
    let torus_frame = torus
        .positional_torus_frame
        .expect("explicit local-system suffix torus");
    assert_eq!(torus_frame.center, [-7.0, 8.0, 5.0]);
    assert_eq!(torus_frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(torus_frame.ref_direction, [0.8, 0.6, 0.0]);
    assert_eq!(torus_frame.major_radius, 3.0);
    assert_eq!(torus_frame.minor_radius, 1.0);
    assert!(torus.has_inline_non_plane_local_system_suffix(0x26));
    assert_eq!(torus.boundary, SurfaceBodyBoundary::CompoundClose);

    let compact = [
        0x18, 0x10, 0x18, 0xe5, 0x10, 0x0f, 0x18, 0xe4, 0x2f, 0x00, 0x00, 0x2e, 0x08, 0x00, 0x2f,
        0x10, 0x00,
    ];
    let cylinder = parameter_records(&local_system_suffix_row(0x24, &compact, &[0x0f])).remove(0);
    let cylinder_frame = cylinder
        .positional_cylinder_frame
        .expect("compact local-system suffix cylinder");
    assert_eq!(cylinder_frame.origin, [2.0, 3.0, 4.0]);
    assert_eq!(cylinder_frame.axis, [1.0, 0.0, 0.0]);
    assert_eq!(cylinder_frame.ref_direction, [0.0, 1.0, 0.0]);
    assert_eq!(cylinder_frame.radius, 1.0);
    assert_eq!(cylinder_frame.length, None);
    assert!(cylinder.has_inline_non_plane_local_system_suffix(0x24));
}

#[test]
fn decodes_compact_y_axis_cone_with_a_nonzero_origin() {
    let local_system = [
        0x10, 0x18, 0xe6, 0x10, 0x18, 0x10, 0x18, 0xe4, 0x46, 0x20, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x18,
    ];
    let record = parameter_records(&local_system_suffix_row(
        0x25,
        &local_system,
        &[0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x18],
    ))
    .remove(0);

    let frame = record
        .positional_cone_frame
        .expect("compact Y-axis cone carrier");
    assert_eq!(frame.apex, [1.0, 8.0, 0.0]);
    assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
    assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert_eq!(frame.half_angle, std::f64::consts::FRAC_PI_4);
}

#[test]
fn legacy_planar_cone_envelope_witness_resolves_inline_suffix_origin() {
    let local_system = [
        0x10, 0x18, 0xe6, 0x10, 0x18, 0x10, 0x18, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x18,
    ];
    let record = parameter_records(&legacy_planar_cone_suffix_row(
        &local_system,
        &[0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x23],
    ))
    .remove(0);

    let frame = record
        .positional_cone_frame
        .expect("legacy cone witness carrier");
    assert_eq!(frame.apex, [0.0, -2.0, 0.0]);
    assert_eq!(frame.axis, [0.0, 1.0, 0.0]);
    assert_eq!(frame.ref_direction, [1.0, 0.0, 0.0]);
    assert_eq!(frame.half_angle, std::f64::consts::FRAC_PI_4);
}

#[test]
fn retains_a_structurally_complete_inline_row_when_center_sign_is_ambiguous() {
    let record = inline_non_plane_record(
        0x24,
        [2.0, 4.0],
        [[-1.0, -1.0, 6.0], [1.0, 1.0, 8.0]],
        &[
            &INLINE_TEST_LOCAL_SYSTEM_Z[..],
            &[0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x2f, 0x10, 0x00],
        ]
        .concat(),
        &[0x2f, 0x14, 0x00],
    );

    assert!(record.positional_cylinder_frame.is_none());
    assert!(record
        .body
        .windows(INLINE_TEST_LOCAL_SYSTEM_Z.len())
        .any(|window| { window == INLINE_TEST_LOCAL_SYSTEM_Z }));
    assert_eq!(record.boundary, SurfaceBodyBoundary::CompoundClose);
}
