// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::parameter_records;
use crate::scalar;
use crate::surface::decode_inline_four_bound_cylinder_envelope;
use crate::surface::decode_inline_referenced_cylinder_envelope;
use crate::surface::decode_inline_selector_cylinder_envelope;
use crate::surface::decode_positional_cone_frame;
use crate::surface::inline_close;
use crate::surface::inline_surface_body;
use crate::surface::InlineSurfaceCarrier;
use crate::surface::SurfaceBodyBoundary;
use crate::surface::SurfaceKind;
use crate::surface::SurfaceParameterRecord;
use cadmpeg_ir::scalar::PositiveLength;

#[test]
fn inline_close_refuses_nonfinite_values() {
    for value in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN] {
        assert!(!inline_close(value, 1.0));
        assert!(!inline_close(1.0, value));
        assert!(!inline_close(value, value));
    }
}

fn push_inline_test_scalar(bytes: &mut Vec<u8>, value: f64) {
    match cadmpeg_core::convert::truncate_f64_to_i32(value).expect("fixture scalar fits i32") {
        -1 => bytes.push(0x0d),
        -4..=-2 => {
            let raw = value.to_be_bytes();
            assert_eq!(raw[0], 0xc0, "test coordinate must be negative");
            bytes.push(0x2d);
            bytes.extend_from_slice(&raw[1..]);
        }
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

fn push_inline_test_first_directrix(bytes: &mut Vec<u8>, value: f64) {
    let raw = value.to_be_bytes();
    assert_eq!(raw[0], 0x40, "test directrix coordinate must be positive");
    bytes.push(0x2d);
    bytes.extend_from_slice(&raw[1..]);
}

fn push_inline_test_signed_first_directrix(bytes: &mut Vec<u8>, value: f64) {
    if value > 0.0 {
        push_inline_test_first_directrix(bytes, value);
    } else if value < 0.0 {
        let raw = value.to_be_bytes();
        assert_eq!(raw[0], 0xc0, "test directrix coordinate must be negative");
        bytes.push(0x46);
        bytes.extend_from_slice(&raw[1..]);
    } else {
        bytes.push(0x0f);
    }
}

#[test]
fn referenced_inline_cylinder_envelope_uses_outer_axial_bounds() {
    let mut body = vec![0x32, 0, 0, 0, 0, 0, 0, 0];
    for value in [2.0, 4.0, 8.0] {
        push_inline_test_first_directrix(&mut body, value);
    }
    for value in [-1.0, 3.0, 5.0, 1.0, 3.0, 5.0] {
        push_inline_test_scalar(&mut body, value);
    }
    body.push(0xe3);

    let envelope = decode_inline_referenced_cylinder_envelope(
        SurfaceKind::Cylinder,
        &body,
        &scalar::ScalarCache::default(),
    )
    .expect("referenced inline envelope");
    assert_eq!(envelope.axial, [2.0, 8.0]);
    assert_eq!(
        envelope.corners,
        [
            [Some(-1.0), Some(3.0), Some(5.0)],
            [Some(1.0), Some(3.0), Some(5.0)]
        ]
    );
    assert_eq!(envelope.close, body.len() - 1);
}

#[test]
fn referenced_inline_compact_x_cylinder_accepts_oblique_trim_containment() {
    let mut body = vec![0x32, 0, 0, 0, 0, 0, 0, 0];
    for value in [2.0, 4.0, 6.0] {
        push_inline_test_first_directrix(&mut body, value);
    }
    for value in [-2.0, 2.0, -4.0, -2.0, 3.0, -3.0] {
        push_inline_test_signed_first_directrix(&mut body, value);
    }
    body.push(0xe3);
    body.extend_from_slice(&[0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4]);
    for value in [-4.0, 3.0, -4.0] {
        push_inline_test_scalar(&mut body, value);
    }
    body.extend_from_slice(&[0xe4, 0xe3]);

    let InlineSurfaceCarrier::Cylinder { frame, .. } = crate::decode::with_service_context(|ctx| {
        inline_surface_body(
            ctx,
            SurfaceKind::Cylinder,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("decode the inline surface body")
    .and_then(|body| body.carrier)
    .expect("compact X frame resolves from contained oblique-trim evidence") else {
        panic!("referenced inline body resolves a cylinder");
    };
    assert_eq!(frame.frame().origin(), [-4.0, 3.0, -4.0]);
    assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, -1.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(4.0));
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

fn push_inline_test_negative_coordinate(bytes: &mut Vec<u8>, value: f64) {
    let raw = value.to_be_bytes();
    assert_eq!(raw[0], 0xc0, "test coordinate must be negative");
    bytes.push(0x2d);
    bytes.extend_from_slice(&raw[1..]);
}

fn inline_11_10_13_cylinder_row(first_bound: f64, center: f64, second_bound: f64) -> Vec<u8> {
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0, 0x11, 0x10, 0x13, 0x18];
    push_inline_test_negative_coordinate(&mut payload, first_bound);
    payload.push(0x10);
    push_inline_test_negative_coordinate(&mut payload, center);
    push_inline_test_negative_coordinate(&mut payload, second_bound);
    payload.extend_from_slice(&[0x19, 0, 0, 0, 0, 0, 0, 0, 0x0e, 0xf7, 0x17, 0xe3]);
    payload.extend_from_slice(&[0x10, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x10]);
    push_inline_test_negative_coordinate(&mut payload, -4.0);
    payload.push(0x18);
    push_inline_test_negative_coordinate(&mut payload, -4.0);
    payload.extend_from_slice(&[0x0f, 0xe3]);
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
        .positional_cylinder_frame()
        .expect("witnessed inline cylinder");
    assert_eq!(cylinder_frame.frame().origin(), [2.0, 2.0, 4.0]);
    assert_eq!(cylinder_frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(cylinder_frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert_eq!(cylinder_frame.radius.get(), 2.0);
    assert_eq!(cylinder_frame.length.map(PositiveLength::get), Some(2.0));
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
    let cone_frame = cone.positional_cone_frame().expect("witnessed inline cone");
    assert_eq!(cone_frame.frame().origin(), [4.0, 4.0, 4.0]);
    assert_eq!(cone_frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(cone_frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert_eq!(
        cone_frame.half_angle.get().get(),
        std::f64::consts::FRAC_PI_4
    );

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
        .positional_torus_frame()
        .expect("witnessed inline torus");
    assert_eq!(torus_frame.frame().origin(), [5.0, 5.0, 4.0]);
    assert_eq!(torus_frame.major_radius.get(), 4.0);
    assert_eq!(torus_frame.minor_radius.get(), 1.0);

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
        .positional_torus_frame()
        .expect("witnessed inline sphere");
    assert_eq!(sphere_frame.frame().origin(), [2.0, 2.0, 4.0]);
    assert_eq!(sphere_frame.major_radius.get(), 0.0);
    assert_eq!(sphere_frame.minor_radius.get(), 2.0);
}

#[test]
fn compact_y_image_uses_the_unique_envelope_axis_witness() {
    let mut local_system = vec![0x18, 0x10, 0x18, 0xe5, 0x10, 0x0f, 0x18, 0xe4];
    push_inline_test_scalar(&mut local_system, 2.0);
    push_inline_test_scalar(&mut local_system, 0.0);
    push_inline_test_scalar(&mut local_system, 5.0);
    let cylinder = inline_non_plane_record(
        0x24,
        [2.0, 5.0],
        [[1.0, 2.0, 4.0], [2.0, 5.0, 5.0]],
        &local_system,
        &[0x0f],
    );

    let frame = cylinder
        .positional_cylinder_frame()
        .expect("envelope-witnessed compact Y cylinder");
    assert_eq!(frame.frame().origin(), [2.0, 0.0, 5.0]);
    assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(3.0));
}

#[test]
fn selector_envelope_places_a_compact_y_cylinder() {
    let build = |placeholder_slots: &[usize]| {
        let mut body = vec![0x12];
        push_inline_test_signed_first_directrix(&mut body, 4.0);
        body.push(0x14);
        push_inline_test_signed_first_directrix(&mut body, -4.0);
        for (index, value) in [2.0, 4.0, 2.0, -2.0, -4.0, -2.0].into_iter().enumerate() {
            if placeholder_slots.contains(&index) {
                body.extend_from_slice(&[0xda, 0, 0, 0, 0, 0, 1]);
            } else {
                push_inline_test_signed_first_directrix(&mut body, value);
            }
        }
        body.extend_from_slice(&[0xf7, 0x40, 0xe3]);
        body.extend_from_slice(&[0x0f, 0x18, 0xe6, 0x0f, 0x18, 0x10, 0x18]);
        for value in [0.0, 0.0, 0.0] {
            push_inline_test_scalar(&mut body, value);
        }
        body.extend_from_slice(&[0x2f, 0x00, 0x00, 0xe3]);
        body
    };
    let decode = |body: &[u8]| {
        crate::decode::with_service_context(|ctx| {
            inline_surface_body(
                ctx,
                SurfaceKind::Cylinder,
                body,
                &scalar::ScalarCache::default(),
            )
        })
        .expect("decode the inline surface body")
        .and_then(|body| body.carrier)
        .and_then(|carrier| match carrier {
            InlineSurfaceCarrier::Cylinder { frame, .. } => Some(frame),
            _ => None,
        })
    };
    let frame = decode(&build(&[])).expect("complete selector envelope");
    assert_eq!(decode(&build(&[5])), Some(frame));
    for placeholder_slots in [&[1][..], &[3, 5][..]] {
        assert!(decode_inline_selector_cylinder_envelope(
            SurfaceKind::Cylinder,
            &build(placeholder_slots),
            &scalar::ScalarCache::default(),
        )
        .is_none());
    }
    assert_eq!(frame.frame().origin(), [0.0, 0.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius.get(), 2.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(8.0));
}

#[test]
fn selector_envelope_decodes_bare_zero_and_oblique_outline() {
    let mut body = vec![0x11, 0x18, 0x13];
    push_inline_test_first_directrix(&mut body, 4.0);
    for value in [5.0, 4.0] {
        push_inline_test_first_directrix(&mut body, value);
    }
    body.push(0x18);
    for value in [2.0, 2.0, 4.0] {
        push_inline_test_first_directrix(&mut body, value);
    }
    body.push(0xe3);
    body.extend_from_slice(&[0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f]);
    for value in [3.0, 3.0, 0.0] {
        push_inline_test_scalar(&mut body, value);
    }
    body.extend_from_slice(&[0x2f, 0x00, 0x00, 0xe3]);

    let InlineSurfaceCarrier::Cylinder { frame, .. } = crate::decode::with_service_context(|ctx| {
        inline_surface_body(
            ctx,
            SurfaceKind::Cylinder,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("decode the inline surface body")
    .and_then(|body| body.carrier)
    .expect("bare-zero selector envelope") else {
        panic!("selector envelope resolves a cylinder");
    };
    assert_eq!(frame.frame().origin(), [-3.0, -3.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius.get(), 2.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(4.0));
}

#[test]
fn selector_placeholder_resolves_from_one_radial_extreme() {
    let mut body = vec![0x11, 0x18, 0x13];
    push_inline_test_first_directrix(&mut body, 4.0);
    body.extend_from_slice(&[0xda, 0, 0, 0, 0, 0, 1]);
    push_inline_test_first_directrix(&mut body, 5.0);
    body.push(0x18);
    for value in [2.0, 3.0, 4.0] {
        push_inline_test_first_directrix(&mut body, value);
    }
    body.push(0xe3);
    body.extend_from_slice(&[0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f]);
    for value in [4.0, 4.0, 0.0] {
        push_inline_test_scalar(&mut body, value);
    }
    body.extend_from_slice(&[0x2f, 0x00, 0x00, 0xe3]);

    let InlineSurfaceCarrier::Cylinder { frame, .. } = crate::decode::with_service_context(|ctx| {
        inline_surface_body(
            ctx,
            SurfaceKind::Cylinder,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("decode the inline surface body")
    .and_then(|body| body.carrier)
    .expect("radius-witnessed selector placeholder") else {
        panic!("selector placeholder resolves a cylinder");
    };
    assert_eq!(frame.frame().origin(), [-4.0, -4.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.radius.get(), 2.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(4.0));
}

#[test]
fn compact_axis_image_selects_equal_spans_and_stored_axis_branch() {
    let mut local_system = vec![0x18, 0x0f, 0x18, 0x0f, 0x18, 0xe6, 0x0f];
    push_inline_test_scalar(&mut local_system, 2.0);
    push_inline_test_scalar(&mut local_system, 2.0);
    push_inline_test_scalar(&mut local_system, 3.0);
    let cylinder = inline_non_plane_record(
        0x24,
        [-2.0, -4.0],
        [[1.0, 1.0, -1.0], [3.0, 3.0, 1.0]],
        &local_system,
        &[0x0f],
    );

    let frame = cylinder
        .positional_cylinder_frame()
        .expect("compact image selects the Z span and stored axis branch");
    assert_eq!(frame.frame().origin(), [2.0, 2.0, 3.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(2.0));
}

#[test]
fn four_bound_inline_envelope_accepts_oblique_axial_containment() {
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    push_inline_test_scalar(&mut payload, 0.0);
    push_inline_test_first_directrix(&mut payload, 2.0);
    push_inline_test_scalar(&mut payload, 1.0);
    push_inline_test_first_directrix(&mut payload, 6.0);
    for value in [-1.0, 1.0, 4.0, 1.0, 3.0, 6.0] {
        push_inline_test_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4]);
    for value in [4.0, 2.0, 5.0] {
        push_inline_test_scalar(&mut payload, value);
    }
    payload.extend_from_slice(&[0x0f, 0xe3]);

    let body = &payload[6..];
    let InlineSurfaceCarrier::Cylinder { frame, .. } = crate::decode::with_service_context(|ctx| {
        inline_surface_body(
            ctx,
            SurfaceKind::Cylinder,
            body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("decode the inline surface body")
    .and_then(|body| body.carrier)
    .expect("contained four-bound envelope resolves one carrier") else {
        panic!("cylinder grammar resolves a cylinder carrier");
    };
    assert_eq!(frame.frame().origin(), [-4.0, 2.0, 5.0]);
    assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, -1.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(4.0));
}

#[test]
fn four_bound_inline_envelope_accepts_an_endpoint_anchored_oblique_trim() {
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    push_inline_test_scalar(&mut payload, 0.0);
    push_inline_test_first_directrix(&mut payload, 2.0);
    let u_high_offset = payload.len();
    push_inline_test_scalar(&mut payload, 1.0);
    push_inline_test_first_directrix(&mut payload, 6.0);
    for value in [-3.0, 1.0, 4.0, 2.0, 3.0, 6.0] {
        push_inline_test_scalar(&mut payload, value);
    }
    payload.extend_from_slice(&[0xf7, 0x17]);
    payload.push(0xe3);
    payload.extend_from_slice(&[0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4]);
    for value in [4.0, 2.0, 5.0] {
        push_inline_test_scalar(&mut payload, value);
    }
    payload.extend_from_slice(&[0x0f, 0xe3]);

    let body = &payload[6..];
    let InlineSurfaceCarrier::Cylinder { frame, .. } = crate::decode::with_service_context(|ctx| {
        inline_surface_body(
            ctx,
            SurfaceKind::Cylinder,
            body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("decode the inline surface body")
    .and_then(|body| body.carrier)
    .expect("four-bound envelope and compact frame resolve one carrier") else {
        panic!("cylinder grammar resolves a cylinder carrier");
    };
    assert_eq!(frame.frame().origin(), [-4.0, 2.0, 5.0]);
    assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, -1.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(4.0));

    let mut degenerate_u_interval = payload;
    degenerate_u_interval[u_high_offset] = 0x0f;
    assert!(decode_inline_four_bound_cylinder_envelope(
        SurfaceKind::Cylinder,
        &degenerate_u_interval[6..],
        &scalar::ScalarCache::default(),
    )
    .is_none());
}

#[test]
fn four_bound_inline_envelope_decodes_directrix_dict_outline() {
    let mut body = Vec::new();
    push_inline_test_scalar(&mut body, 0.0);
    body.push(0x18);
    push_inline_test_scalar(&mut body, 1.0);
    push_inline_test_first_directrix(&mut body, 3.0);

    let first_x = 5.75_f64.to_be_bytes();
    assert_eq!(first_x[..2], [0x40, 0x17]);
    body.push(0xa2);
    body.extend_from_slice(&first_x[2..]);
    for value in [5.0, 5.0, 3.75, 3.0, 2.0] {
        push_inline_test_first_directrix(&mut body, value);
    }
    body.extend_from_slice(&[0xf7, 0x17, 0xe3]);
    body.extend_from_slice(&[0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f]);
    for value in [4.75, 4.0, 5.0] {
        push_inline_test_first_directrix(&mut body, value);
    }
    body.extend_from_slice(&[0x0f, 0xe3]);

    let InlineSurfaceCarrier::Cylinder { frame, .. } = crate::decode::with_service_context(|ctx| {
        inline_surface_body(
            ctx,
            SurfaceKind::Cylinder,
            &body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("decode the inline surface body")
    .and_then(|body| body.carrier)
    .expect("directrix-DICT outline and compact frame resolve one carrier") else {
        panic!("inline body resolves a cylinder carrier");
    };
    assert_eq!(frame.frame().origin(), [-4.75, -4.0, -5.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length.map(PositiveLength::get), Some(3.0));
}

#[test]
fn inline_envelope_rejects_lane_aliases_by_geometry() {
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0];
    push_inline_test_scalar(&mut payload, 0.0);
    push_inline_test_first_directrix(&mut payload, 2.0);
    push_inline_test_scalar(&mut payload, 1.0);
    push_inline_test_first_directrix(&mut payload, 6.0);
    for value in [-3.0, 1.0, 2.0, 2.0, 3.0, 2.0] {
        push_inline_test_scalar(&mut payload, value);
    }
    payload.push(0xe3);
    payload.extend_from_slice(&[0x18, 0xe4, 0x0f, 0x18, 0x0f, 0x18, 0x10, 0x18, 0xe4]);
    push_inline_test_scalar(&mut payload, -4.0);
    push_inline_test_scalar(&mut payload, 2.0);
    payload.extend_from_slice(&[0xd3, 0, 0, 0, 0, 0, 0]);
    payload.extend_from_slice(&[0xe4, 0xe3]);

    let body = &payload[6..];
    let InlineSurfaceCarrier::Cylinder { frame, .. } = crate::decode::with_service_context(|ctx| {
        inline_surface_body(
            ctx,
            SurfaceKind::Cylinder,
            body,
            &scalar::ScalarCache::default(),
        )
    })
    .expect("decode the inline surface body")
    .and_then(|body| body.carrier)
    .expect("outline containment rejects the alternate scalar lane") else {
        panic!("inline body resolves a cylinder carrier");
    };
    let expected_z = f64::from_be_bytes([0xc0, 0x01, 0, 0, 0, 0, 0, 0]).abs();
    assert_eq!(frame.frame().origin()[..2], [-4.0, 2.0]);
    assert!((frame.frame().origin()[2] - expected_z).abs() <= f64::EPSILON);
    assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius.get(), 1.0);
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
        .positional_torus_frame()
        .expect("explicit local-system suffix torus");
    assert_eq!(torus_frame.frame().origin(), [-7.0, 8.0, 5.0]);
    assert_eq!(torus_frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(torus_frame.frame().ref_direction(), [0.8, 0.6, 0.0]);
    assert_eq!(torus_frame.major_radius.get(), 3.0);
    assert_eq!(torus_frame.minor_radius.get(), 1.0);
    assert!(crate::decode::with_service_context(
        |ctx| torus.has_inline_non_plane_local_system_suffix(ctx)
    )
    .expect("check that the surface has an inline non-plane local system suffix"));
    assert_eq!(torus.boundary, SurfaceBodyBoundary::CompoundClose);

    let compact = [
        0x18, 0x10, 0x18, 0xe5, 0x10, 0x0f, 0x18, 0xe4, 0x2f, 0x00, 0x00, 0x2e, 0x08, 0x00, 0x2f,
        0x10, 0x00,
    ];
    let cylinder = parameter_records(&local_system_suffix_row(0x24, &compact, &[0x0f])).remove(0);
    let cylinder_frame = cylinder
        .positional_cylinder_frame()
        .expect("compact local-system suffix cylinder");
    assert_eq!(cylinder_frame.frame().origin(), [2.0, 3.0, 4.0]);
    assert_eq!(cylinder_frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(cylinder_frame.frame().ref_direction(), [0.0, 0.0, 1.0]);
    assert_eq!(cylinder_frame.radius.get(), 1.0);
    assert_eq!(cylinder_frame.length, None);
    assert!(crate::decode::with_service_context(
        |ctx| cylinder.has_inline_non_plane_local_system_suffix(ctx)
    )
    .expect("check that the surface has an inline non-plane local system suffix"));
}

#[test]
fn cylinder_inline_suffix_uses_the_11_10_13_placement_witness() {
    let record = parameter_records(&inline_11_10_13_cylinder_row(-3.0, -4.0, -5.0)).remove(0);
    let frame = record
        .positional_cylinder_frame()
        .expect("placement-witnessed inline cylinder");
    assert_eq!(frame.frame().origin(), [-4.0, 0.0, -4.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length, None);

    let mut alternate_replay = inline_11_10_13_cylinder_row(-3.0, -4.0, -5.0);
    let replay_offset = alternate_replay
        .windows(2)
        .position(|window| window == [0xf7, 0x17])
        .expect("replay trailer");
    alternate_replay[replay_offset + 1] = 0x40;
    assert!(parameter_records(&alternate_replay)
        .remove(0)
        .positional_cylinder_frame()
        .is_some());

    let inconsistent = parameter_records(&inline_11_10_13_cylinder_row(-2.0, -4.0, -5.0)).remove(0);
    assert!(inconsistent.positional_cylinder_frame().is_none());
}

#[test]
fn cylinder_inline_suffix_uses_the_held_axis_placement_witness() {
    let mut payload = vec![7, 0x24, 4, 0x01, 0, 0, 0x11, 0x18, 0x13];
    for value in [0.0, -5.0] {
        push_inline_test_signed_first_directrix(&mut payload, value);
    }
    payload.push(0x10);
    push_inline_test_signed_first_directrix(&mut payload, -2.0);
    push_inline_test_signed_first_directrix(&mut payload, -3.0);
    payload.extend_from_slice(&[0x19, 0, 0, 0, 0, 0, 0, 0]);
    push_inline_test_signed_first_directrix(&mut payload, -2.0);
    payload.extend_from_slice(&[0xf7, 0x17, 0xe3]);
    payload.extend_from_slice(&[0x0f, 0x18, 0xe5, 0x0f, 0x18, 0xe5, 0x0f]);
    for value in [-4.0, 0.0, -2.0] {
        push_inline_test_scalar(&mut payload, value);
    }
    payload.extend_from_slice(&[0x0f, 0xe3]);

    let record = parameter_records(&payload).remove(0);
    let frame = record
        .positional_cylinder_frame()
        .expect("held-axis-witnessed inline cylinder");
    assert_eq!(frame.frame().origin(), [-4.0, 0.0, -2.0]);
    assert_eq!(frame.frame().axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius.get(), 1.0);
    assert_eq!(frame.length, None);
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
        .positional_cone_frame()
        .expect("compact Y-axis cone carrier");
    assert_eq!(frame.frame().origin(), [1.0, 8.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [-1.0, 0.0, 0.0]);
    assert_eq!(frame.half_angle.get().get(), std::f64::consts::FRAC_PI_4);
}

#[test]
fn inline_cone_accepts_a_complete_support_apex_operand_after_its_envelope() {
    let support_apex = |apex| {
        let mut body = vec![0x18, 0xe4, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xe4];
        push_inline_test_negative_coordinate(&mut body, apex);
        body.extend_from_slice(&[0x19, 0, 0, 0, 0, 0, 0, 0, 0x21, 0xfb, 0x54]);
        body
    };
    let first_support_apex = support_apex(-4.0);
    let record = inline_non_plane_record(
        0x25,
        [2.0, 4.0],
        [[-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]],
        &first_support_apex,
        &[0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x18],
    );

    let frame = record
        .positional_cone_frame()
        .expect("envelope-delimited support-apex cone");
    assert_eq!(frame.frame().origin(), [-4.0, 0.0, 0.0]);
    assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, -1.0]);
    assert_eq!(frame.half_angle.get().get(), std::f64::consts::FRAC_PI_4);

    let mut compound_replay = vec![0x99, 0xe3];
    compound_replay.extend_from_slice(&first_support_apex);
    compound_replay.extend_from_slice(&[0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x18, 0xe3, 0x98]);
    assert_eq!(
        decode_positional_cone_frame(&compound_replay, &scalar::ScalarCache::default()),
        Some(frame)
    );

    let mut ambiguous = compound_replay;
    ambiguous.pop();
    ambiguous.extend_from_slice(&support_apex(-5.0));
    ambiguous.extend_from_slice(&[0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x18, 0xe3]);
    assert!(decode_positional_cone_frame(&ambiguous, &scalar::ScalarCache::default()).is_none());
}

fn complete_support_apex_cone_operand(
    apex: f64,
    reference_payload: [u8; 7],
    angle: f64,
) -> Vec<u8> {
    let mut body = vec![0x18, 0xe4, 0x18, 0x18, 0x18, 0x18, 0x18, 0x18, 0xe4];
    push_inline_test_negative_coordinate(&mut body, apex);
    body.push(0x19);
    body.extend_from_slice(&reference_payload);
    body.extend_from_slice(&[0x20, 0, 0]);
    let raw = angle.to_be_bytes();
    let prefix = match raw[..2] {
        [0x3f, 0xe9] => 0x74,
        [0x3f, 0xf0] => 0x7b,
        _ => panic!("unsupported test cone angle"),
    };
    body.push(prefix);
    body.extend_from_slice(&raw[2..]);
    body
}

fn support_apex_cone_record(body: &[u8]) -> SurfaceParameterRecord {
    let mut payload = vec![7, 0x25, 4, 0x01, 0, 0];
    payload.extend_from_slice(body);
    payload.extend_from_slice(&[0xe3, 0x98]);
    let records = parameter_records(&payload);
    assert_eq!(records.len(), 1);
    records.into_iter().next().unwrap()
}

#[test]
fn complete_cone_operand_bounds_model_reference_payload_closes() {
    let references = [
        [0; 7],
        [0, 0, 0, 0, 0, 0xe3, 0],
        [0xe0, 0, b'a', 0, 0, 0xe3, 0],
    ];
    for reference in references {
        let mut body = vec![0x7b, 0, 0, 0, 0, 0, 0, 0xe3];
        body.extend_from_slice(&complete_support_apex_cone_operand(
            -4.0,
            reference,
            std::f64::consts::FRAC_PI_4,
        ));
        let record = support_apex_cone_record(&body);
        assert_eq!(record.body, body);
        assert_eq!(record.boundary, SurfaceBodyBoundary::CompoundClose);
        let frame = record.positional_cone_frame().unwrap();
        assert_eq!(frame.frame().origin(), [-4.0, 0.0, 0.0]);
        assert_eq!(frame.frame().axis(), [1.0, 0.0, 0.0]);
        assert_eq!(frame.frame().ref_direction(), [0.0, 0.0, -1.0]);
        assert_eq!(frame.half_angle().get().get(), std::f64::consts::FRAC_PI_4);
        assert_eq!(
            decode_positional_cone_frame(&body, &scalar::ScalarCache::default()),
            Some(frame)
        );
    }
}

#[test]
fn complete_cone_operand_framing_stops_before_an_adjacent_positional_row() {
    let first = complete_support_apex_cone_operand(-4.0, [0; 7], 1.0);
    let second = complete_support_apex_cone_operand(-5.0, [0; 7], 1.0);
    let mut payload = vec![7, 0x25, 4, 0x01, 0, 0];
    payload.extend_from_slice(&first);
    payload.extend_from_slice(&[0xe3, 8, 0x25, 4, 0x01, 0, 0]);
    payload.extend_from_slice(&second);
    payload.push(0xe3);
    let records = parameter_records(&payload);
    assert_eq!(records.len(), 2);
    for ((record, expected_body), expected_apex) in
        records.iter().zip([&first, &second]).zip([-4.0, -5.0])
    {
        assert_eq!(&record.body, expected_body);
        assert_eq!(
            record.positional_cone_frame().unwrap().frame().origin(),
            [expected_apex, 0.0, 0.0]
        );
    }
}

#[test]
fn complete_cone_operand_framing_cannot_claim_a_later_named_production() {
    let owned = complete_support_apex_cone_operand(-4.0, [0; 7], 1.0);
    let foreign = complete_support_apex_cone_operand(-5.0, [0; 7], 1.0);
    for prefix in [&owned[..], &[0x99][..]] {
        let mut body = prefix.to_vec();
        body.extend_from_slice(b"\xe3\xe0\0srf_prim_ptr(cone)\0\xe3");
        body.extend_from_slice(&foreign);
        let record = support_apex_cone_record(&body);
        if prefix == &owned[..] {
            assert_eq!(record.body, owned);
            assert_eq!(
                record.positional_cone_frame().unwrap().frame().origin(),
                [-4.0, 0.0, 0.0]
            );
        } else {
            assert!(record.positional_cone_frame().is_none());
        }
        assert!(!record
            .body
            .windows(foreign.len())
            .any(|bytes| bytes == foreign.as_slice()));
    }
}

#[test]
fn complete_cone_operand_framing_leaves_the_terminal_contour_to_its_owner() {
    let owned = complete_support_apex_cone_operand(-4.0, [0; 7], 1.0);
    let foreign = complete_support_apex_cone_operand(-5.0, [0; 7], 1.0);
    let contour = [0x80, 0x80, 1, 0x0f, 0x0f, 0x0f, 0x0f, 0xe1];
    let mut body = owned.clone();
    body.push(0xe3);
    body.extend_from_slice(&contour);
    body.push(0xe3);
    body.extend_from_slice(&foreign);
    let record = support_apex_cone_record(&body);
    assert_eq!(record.body, owned);
    assert!(record.positional_cone_frame().is_none());
    assert!(decode_positional_cone_frame(&body, &scalar::ScalarCache::default()).is_none());
    let mut payload = vec![7, 0x25, 4, 0x01, 0, 0];
    payload.extend_from_slice(&body);
    payload.push(0xe3);
    let contours = super::contour_records(&payload);
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0].surface_id, 7);
    assert_eq!(contours[0].curve_header_id, 128);
    assert_eq!(contours[0].body, contour);
}

#[test]
fn complete_cone_operand_bounds_apex_and_angle_payload_closes() {
    let apex = f64::from_bits(0xc010_0000_00e3_0000);
    let angle = f64::from_bits(0x3ff0_0000_00e3_0000);
    let body = complete_support_apex_cone_operand(apex, [0; 7], angle);
    let record = support_apex_cone_record(&body);
    assert_eq!(record.body, body);
    let frame = record.positional_cone_frame().unwrap();
    assert_eq!(frame.frame().origin(), [apex, 0.0, 0.0]);
    assert_eq!(frame.half_angle().get().get(), angle);
}

#[test]
fn complete_cone_operands_reconcile_compound_and_terminal_carriers() {
    let first = complete_support_apex_cone_operand(-4.0, [0; 7], 1.0);
    for second_apex in [-4.0, -5.0] {
        let second = complete_support_apex_cone_operand(second_apex, [0, 0, 0, 0, 0, 0xe3, 0], 1.0);
        for operands in [[&first, &second], [&second, &first]] {
            let mut body = operands[0].clone();
            body.push(0xe3);
            body.extend_from_slice(operands[1]);
            let direct = decode_positional_cone_frame(&body, &scalar::ScalarCache::default());
            let record = support_apex_cone_record(&body);
            assert_eq!(record.body, body);
            assert_eq!(record.positional_cone_frame(), direct);
            assert_eq!(direct.is_some(), second_apex == -4.0);
            if let Some(frame) = direct {
                assert_eq!(frame.frame().origin(), [-4.0, 0.0, 0.0]);
                assert_eq!(frame.half_angle().get().get(), 1.0);
            }
        }
    }
}

#[test]
fn cone_operand_agreement_requires_one_order_independent_component_range() {
    let operands = [-4.0, -4.0 - 0.75e-12, -4.0 - 1.5e-12]
        .map(|apex| complete_support_apex_cone_operand(apex, [0; 7], 1.0));
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let mut body = Vec::new();
        for (index, operand) in order.into_iter().enumerate() {
            if index != 0 {
                body.push(0xe3);
            }
            body.extend_from_slice(&operands[operand]);
        }
        assert!(decode_positional_cone_frame(&body, &scalar::ScalarCache::default()).is_none());
        assert!(support_apex_cone_record(&body)
            .positional_cone_frame()
            .is_none());
    }
}

#[test]
fn planar_and_support_apex_cone_operands_require_agreement() {
    let prefix = legacy_planar_cone_suffix_row(&[], &[]);
    let prefix = &prefix[6..prefix.len() - 1];
    for angle in [
        std::f64::consts::FRAC_PI_4,
        f64::from_bits(std::f64::consts::FRAC_PI_4.to_bits() + 1),
        1.0,
    ] {
        let mut operand = complete_support_apex_cone_operand(-2.0, [0; 7], angle);
        operand[..9].copy_from_slice(&[0x10, 0x10, 0x0d, 0x10, 0x10, 0x10, 0x0d, 0x10, 0x10]);
        let mut body = prefix.to_vec();
        body.extend_from_slice(&operand);
        let direct = decode_positional_cone_frame(&body, &scalar::ScalarCache::default());
        let record = support_apex_cone_record(&body);
        assert_eq!(record.positional_cone_frame(), direct);
        assert_eq!(direct.is_some(), angle != 1.0);
        if let Some(frame) = direct {
            assert_eq!(frame.frame().origin(), [0.0, -2.0, 0.0]);
            assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
            assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
            assert_eq!(frame.half_angle().get().get(), std::f64::consts::FRAC_PI_4);
        }
    }
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
        .positional_cone_frame()
        .expect("legacy cone witness carrier");
    assert_eq!(frame.frame().origin(), [0.0, -2.0, 0.0]);
    assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.half_angle.get().get(), std::f64::consts::FRAC_PI_4);
}

#[test]
fn complete_planar_cone_witness_blocks_conflicting_suffix_angle() {
    let local_system = [
        0x10, 0x18, 0xe6, 0x10, 0x18, 0x10, 0x18, 0x2f, 0x00, 0x00, 0x2f, 0x00, 0x00, 0x18,
    ];
    let conflicting_angle = 1.0_f64;
    let angle_bits = conflicting_angle.to_be_bytes();
    assert_eq!(&angle_bits[..2], &[0x3f, 0xf0]);
    let mut suffix = vec![0x7b];
    suffix.extend_from_slice(&angle_bits[2..]);
    let record =
        parameter_records(&legacy_planar_cone_suffix_row(&local_system, &suffix)).remove(0);

    assert_eq!(record.kind(), SurfaceKind::Cone);
    assert_eq!(
        record
            .cone_half_angle_override()
            .unwrap()
            .radians
            .get()
            .get(),
        conflicting_angle
    );
    assert!(record.positional_cone_frame().is_none());
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

    assert!(record.positional_cylinder_frame().is_none());
    assert!(record
        .body
        .windows(INLINE_TEST_LOCAL_SYSTEM_Z.len())
        .any(|window| { window == INLINE_TEST_LOCAL_SYSTEM_Z }));
    assert_eq!(record.boundary, SurfaceBodyBoundary::CompoundClose);
}

fn owned_radius_test_record(major: u8, minor: u8) -> SurfaceParameterRecord {
    let mut frame = Vec::new();
    for value in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0] {
        push_inline_test_scalar(&mut frame, value);
    }
    parameter_records(&local_system_suffix_row(0x26, &frame, &[major, minor])).remove(0)
}

#[test]
fn owned_type26_radius_replay_preserves_generic_and_carrier_lanes() {
    let cache = scalar::ScalarCache::default();
    assert_eq!(
        scalar::decode_inline_surface_suffix_scalar(&[0x0d], 0, &cache),
        Some((-1.0, 1))
    );
    assert_eq!(
        scalar::decode_in_surface_row_lane(&[0x0e], 0, &cache),
        Some((-0.5, 1))
    );
    for (token, radius) in [(0x0d, 0.25), (0x0e, 0.5), (0x0f, 1.0)] {
        let record = owned_radius_test_record(0x0e, token);
        assert_eq!(record.type26_replayed_minor_radius(radius), Some(radius));
        assert_eq!(record.type26_replayed_minor_radius(radius + 0.01), None);
        for invalid in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            assert_eq!(record.type26_replayed_minor_radius(invalid), None);
        }
        if token == 0x0d {
            assert!(matches!(
                record.carrier,
                crate::surface::SurfaceParameterCarrier::Unresolved(_)
            ));
        }
    }
    assert_eq!(
        owned_radius_test_record(0x18, 0x0d).type26_replayed_minor_radius(0.25),
        Some(0.25)
    );
    assert_eq!(
        owned_radius_test_record(0x0e, 0x18).type26_replayed_minor_radius(0.25),
        None
    );
}

#[test]
fn type26_radius_slot_reads_the_full_width_2d_form_as_positive() {
    let cache = scalar::ScalarCache::default();
    let major = [0x2d, 0x20, 0, 0, 0, 0, 0, 0x34];
    let expected = f64::from_be_bytes([0x40, 0x20, 0, 0, 0, 0, 0, 0x34]);
    assert_eq!(
        scalar::decode_type26_radius_slot(&major, 0, &cache),
        Some((expected, major.len()))
    );
    let mut frame = Vec::new();
    for value in [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 4.0] {
        push_inline_test_scalar(&mut frame, value);
    }
    let mut suffix = major.to_vec();
    suffix.extend_from_slice(&[0x2f, 0x14, 0x00]);
    let record = parameter_records(&local_system_suffix_row(0x26, &frame, &suffix)).remove(0);
    assert_eq!(record.type26_replayed_minor_radius(5.0), Some(5.0));
    assert_eq!(record.type26_replayed_minor_radius(4.0), None);
}

#[test]
fn owned_type26_radius_requires_complete_owned_frame_and_suffix() {
    for body in [vec![0x0d], vec![0x0e, 0x0d], vec![0xe4, 0x0f, 0x0d]] {
        let record = parameter_records(&local_system_suffix_row(0x26, &body, &[])).remove(0);
        assert_eq!(record.type26_replayed_minor_radius(0.25), None);
    }
    let record = owned_radius_test_record(0x0e, 0x0d);
    let wrong_kind = SurfaceParameterRecord {
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(SurfaceKind::Cone),
        ..record.clone()
    };
    assert_eq!(wrong_kind.type26_replayed_minor_radius(0.25), None);
    let mut trailing = record.body.clone();
    trailing.push(0x18);
    let record = parameter_records(&local_system_suffix_row(0x26, &trailing, &[])).remove(0);
    assert_eq!(record.type26_replayed_minor_radius(0.25), None);
}

#[test]
fn owned_radius_consensus_withholds_conflicting_pair_offsets_and_saturation() {
    use crate::surface::InlineType26MinorRadius;
    let mut coherent = InlineType26MinorRadius::Absent;
    coherent.observe(0.5, 0.25, 20, 22);
    coherent.observe(0.5, 0.25, 20, 22);
    assert!(matches!(coherent, InlineType26MinorRadius::Unique { .. }));
    for (major, minor, start, end) in [
        (1.0, 0.25, 20, 22),
        (0.5, 0.5, 20, 22),
        (0.5, 0.25, 19, 22),
        (0.5, 0.25, 20, 23),
    ] {
        let mut evidence = coherent;
        evidence.observe(major, minor, start, end);
        evidence.observe(0.5, 0.25, 20, 22);
        assert_eq!(evidence, InlineType26MinorRadius::Withheld);
    }
    for capacity in [
        scalar::MAX_INLINE_EXPLICIT_FRAME_CANDIDATES,
        scalar::MAX_INLINE_ORIGIN_CANDIDATES,
    ] {
        let mut evidence = coherent;
        evidence.saturate(capacity - 1, capacity);
        assert_eq!(evidence, coherent);
        evidence.saturate(capacity, capacity);
        evidence.observe(0.5, 0.25, 20, 22);
        assert_eq!(evidence, InlineType26MinorRadius::Withheld);
    }
}

#[test]
fn compact_cone_origin_selected_prefix_retains_unresolved_and_ambiguous_state() {
    let make = |reference| {
        vec![
            0x18, 0x10, 0x18, 0x10, 0x18, 0xe6, 0x10, 0x18, reference, 0xc6, 0x0f, 0xaf, 0xbf,
            0xd9, 0x73, 0x04, 0x2f, 0x34, 0x00, 0x74, 0x21, 0xfb, 0x54, 0x44, 0x2d, 0x19, 0xe3,
        ]
    };
    crate::decode::with_service_context(|ctx| {
        let cache = scalar::ScalarCache::default();
        for reference in [0x19, 0x32] {
            let mut bytes = make(reference);
            for payload_close in [false, true] {
                if payload_close {
                    bytes[11] = 0xe3;
                }
                let layout = inline_surface_body(ctx, SurfaceKind::Cone, &bytes, &cache)
                    .expect("bounded scan")
                    .expect("owned suffix");
                assert_eq!(layout.terminal_close, Some(bytes.len() - 1));
                assert_eq!(layout.withheld_cone_origin_start, Some(0));
                assert!(layout.carrier.is_none());
            }
        }
        let full = make(0x19);
        let mut nonreference = full.clone();
        nonreference[8] = 0x28;
        let mut missing_zero = full.clone();
        missing_zero[7] = 0x10;
        for bytes in [
            full[..full.len() - 1].to_vec(),
            full[..12].to_vec(),
            nonreference,
            missing_zero,
        ] {
            let layout =
                inline_surface_body(ctx, SurfaceKind::Cone, &bytes, &cache).expect("bounded scan");
            assert!(layout
                .and_then(|layout| layout.withheld_cone_origin_start)
                .is_none());
        }
        let mut known = full[..7].to_vec();
        known.extend([0x18, 0x18, 0x18]);
        known.extend(&full[19..]);
        let layout = inline_surface_body(ctx, SurfaceKind::Cone, &known, &cache)
            .expect("known scan")
            .expect("known suffix");
        assert!(layout.carrier.is_some());
        assert!(layout.withheld_cone_origin_start.is_none());
        for parts in [
            vec![full.clone(), known.clone()],
            vec![known.clone(), known.clone(), full.clone()],
        ] {
            let bytes = parts.concat();
            let layout = inline_surface_body(ctx, SurfaceKind::Cone, &bytes, &cache)
                .expect("ambiguous scan")
                .expect("veto survives ambiguity");
            assert!(layout.carrier.is_none());
            assert!(layout.withheld_cone_origin_start.is_some());
            assert!(layout.terminal_close.is_none());
        }
    });
}
