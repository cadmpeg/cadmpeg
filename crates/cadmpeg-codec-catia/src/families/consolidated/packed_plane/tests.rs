// SPDX-License-Identifier: Apache-2.0

use super::packed_plane;
use cadmpeg_core::decode::View;
use cadmpeg_ir::geometry::{analytic::PlaneSurface, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;

use crate::wire::records::consolidated_records;

fn payload(control: u16, origin: [f64; 3], u: Option<f64>, v: [f64; 2]) -> Vec<u8> {
    let mut bytes = vec![0x08];
    bytes.extend_from_slice(&control.to_le_bytes());
    for value in origin {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    if let Some(value) = u {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in v {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![
        0xb2,
        0x03,
        0x27,
        u8::try_from(payload.len()).expect("bounded plane payload"),
        0x05,
    ];
    bytes.extend_from_slice(payload);
    bytes
}

fn parse(bytes: &[u8]) -> Option<PlaneSurface> {
    let records = consolidated_records(bytes);
    let [record] = records.as_slice() else {
        return None;
    };
    packed_plane(View::over_retained(bytes), record)
}

#[test]
fn packed_plane_controls_preserve_the_origin_and_both_parameter_axes() {
    let cases = [
        (0x1968, None, Point3::new(9.0, 9.6, 13.8)),
        (0x1960, Some(-1.0), Point3::new(-5.0, 9.6, 13.8)),
        (0x1c38, None, Point3::new(8.6, 11.8, 12.0)),
        (0x1c30, Some(-1.0), Point3::new(8.6, 11.8, -2.0)),
    ];
    let admission = cadmpeg_ir::eval::admission::EvaluationAdmission::Standard;
    for (control, u, expected) in cases {
        let bytes = frame(&payload(control, [2.0, 3.0, 5.0], u, [0.6, 0.8]));
        let plane = parse(&bytes).expect("complete packed plane");
        assert!(plane.origin().get().distance(Point3::new(2.0, 3.0, 5.0)) < 1.0e-12);
        let point = cadmpeg_ir::eval::decode::surface_point(
            admission,
            &SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)),
            7.0,
            11.0,
        )
        .expect("plane chart point")
        .get();
        assert!(point.distance(expected) < 1.0e-12);
    }
}

#[test]
fn packed_plane_requires_a_complete_recognized_payload() {
    let complete = payload(0x1968, [2.0, 3.0, 5.0], None, [0.6, 0.8]);
    for length in 0..complete.len() {
        assert!(parse(&frame(&complete[..length])).is_none());
    }
    let mut extra = complete.clone();
    extra.push(0);
    assert!(parse(&frame(&extra)).is_none());
    let mut wrong_marker = complete;
    wrong_marker[0] = 0xb4;
    assert!(parse(&frame(&wrong_marker)).is_none());
    assert!(parse(&frame(&payload(0x1969, [2.0, 3.0, 5.0], None, [0.6, 0.8],))).is_none());
    assert!(parse(&frame(&payload(0x1960, [2.0, 3.0, 5.0], None, [0.6, 0.8],))).is_none());
}

#[test]
fn packed_plane_requires_finite_coordinates_and_unit_directions() {
    for origin in [[f64::NAN, 3.0, 5.0], [2.0, f64::INFINITY, 5.0]] {
        assert!(parse(&frame(&payload(0x1968, origin, None, [0.6, 0.8]))).is_none());
    }
    for u in [0.0, -2.0, f64::NAN, f64::INFINITY] {
        assert!(parse(&frame(&payload(
            0x1960,
            [2.0, 3.0, 5.0],
            Some(u),
            [0.6, 0.8],
        )))
        .is_none());
    }
    for v in [
        [0.0, 0.0],
        [0.5, 0.8],
        [f64::NAN, 0.8],
        [0.6, f64::INFINITY],
    ] {
        assert!(parse(&frame(&payload(0x1968, [2.0, 3.0, 5.0], None, v))).is_none());
    }
}

#[test]
fn packed_plane_uses_its_b27_source_frame() {
    let plane_payload = payload(0x1968, [2.0, 3.0, 5.0], None, [0.6, 0.8]);
    let mut bytes = frame(&plane_payload);
    let frame_end = bytes.len();
    bytes.extend_from_slice(&[0xb2, 0x03, 0x18, 1, 0x05, 0x05]);
    let records = consolidated_records(&bytes);
    assert_eq!(records.len(), 2);
    let source = View::over_retained(&bytes);
    assert!(packed_plane(source, &records[0]).is_some());
    assert!(packed_plane(source, &records[1]).is_none());
    assert!(packed_plane(
        source
            .child(0, frame_end - 1)
            .expect("truncated source view"),
        &records[0],
    )
    .is_none());
    let mut wrong_class = frame(&plane_payload);
    wrong_class[2] = 0x28;
    assert!(parse(&wrong_class).is_none());
    let mut a_family = vec![0xa5, 0x03, 0x27];
    a_family.extend_from_slice(
        &u32::try_from(plane_payload.len())
            .expect("bounded payload")
            .to_le_bytes(),
    );
    a_family.push(0x05);
    a_family.extend_from_slice(&plane_payload);
    assert!(parse(&a_family).is_none());
}

#[test]
fn packed_plane_uses_each_b_family_header_width() {
    let plane_payload = payload(0x1968, [2.0, 3.0, 5.0], None, [0.6, 0.8]);
    for (lead, header) in [
        (0xb2, vec![0x05]),
        (0xb3, vec![0x04, 0x40]),
        (0xb4, vec![0x08, 0x00, 0x01]),
    ] {
        let mut bytes = vec![
            lead,
            0x03,
            0x27,
            u8::try_from(plane_payload.len()).expect("bounded plane payload"),
        ];
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(&plane_payload);
        assert!(parse(&bytes).is_some());
    }
}
