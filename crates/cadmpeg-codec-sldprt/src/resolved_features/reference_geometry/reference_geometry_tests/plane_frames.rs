//! Tests for the `reference_geometry` module's plane-frame pairs.

use super::super::super::CLASS_MARKER;
use super::super::{
    constraint_reference_plane_frame, minimal_reference_plane_frame,
    offset_reference_plane_frame_pair,
};
use crate::layout::constructed_reference_plane_matrix_frame as matrix_plane;
use crate::layout::constructed_reference_plane_minimal_frame as minimal_plane;
use cadmpeg_ir::math::{Point3, Vector3};
#[test]
fn offset_plane_frame_pair_accepts_complete_matrix_frames() {
    let sine = 0.390_731_128_489_273_27_f64;
    let cosine = 0.920_504_853_452_440_5_f64;
    let frame = |distance: f64| {
        let mut bytes = [0; 121];
        for (offset, value) in [
            (0, -sine * distance / 1000.0),
            (8, 0.0),
            (16, cosine * distance / 1000.0),
            (24, -sine),
            (32, 0.0),
            (40, cosine),
            (49, cosine),
            (57, 0.0),
            (65, -sine),
            (73, 0.0),
            (81, 1.0),
            (89, 0.0),
            (97, sine),
            (105, 0.0),
            (113, cosine),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes[48] = 1;
        bytes
    };
    let mut payload = frame(27.25).to_vec();
    payload.extend([0; 13]);
    payload.extend(frame(0.0));

    let (offset, reference) = offset_reference_plane_frame_pair(
        &payload,
        cadmpeg_ir::scalar::Length::new(27.25).unwrap(),
    )
    .unwrap();
    assert_eq!(offset.0, Point3::new(-sine * 27.25, 0.0, cosine * 27.25));
    assert_eq!(reference.0, Point3::new(0.0, 0.0, 0.0));
    assert_eq!(offset.1, reference.1);
    assert_eq!(offset.2, reference.2);
}

#[test]
fn minimal_reference_plane_validates_its_redundant_offset_tail() {
    let root = 13;
    let mut payload = vec![0; root + 81];
    let distance = -0.052_f64;
    for (relative, value) in [
        (0, 0.0_f64),
        (8, 0.0),
        (16, distance),
        (24, 0.0),
        (32, 0.0),
        (40, 1.0),
        (57, -0.0),
        (65, -distance),
        (73, 1.0),
    ] {
        payload[root + relative..root + relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[root + 56] = 0x80;
    assert_eq!(
        minimal_reference_plane_frame(&payload),
        Some((
            Point3::new(0.0, 0.0, -52.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ))
    );

    payload[root + 65..root + 73].copy_from_slice(&0.051f64.to_le_bytes());
    assert_eq!(minimal_reference_plane_frame(&payload), None);
}

fn class_payload(class: &str, frame: &[u8]) -> (Vec<u8>, usize) {
    let mut payload = vec![0xaa; 7];
    let root = payload.len();
    payload.extend(CLASS_MARKER);
    payload.extend(u16::try_from(class.len()).unwrap().to_le_bytes());
    payload.extend(class.as_bytes());
    payload.extend_from_slice(frame);
    (payload, root)
}

fn matrix_frame(marker: u8) -> [u8; matrix_plane::LEN] {
    let mut frame = [0; matrix_plane::LEN];
    for (offset, value) in [(0, 0.0125_f64), (24, 1.0), (65, 1.0), (73, 1.0), (105, 1.0)] {
        frame[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame[48] = marker;
    frame
}

#[test]
fn line_perpendicular_and_line_point_plane_data_read_a_matrix_frame() {
    for class in [
        "moConstraintCoincLinePerpPlaneRefplaneData_c",
        "moPerLinePtRefPlnData_c",
    ] {
        let (payload, root) = class_payload(class, &matrix_frame(1));
        assert_eq!(
            constraint_reference_plane_frame(&payload, root, class),
            Some((
                Point3::new(12.5, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )),
            "{class}"
        );
    }
    let class = "moConstraintCoincLinePerpPlaneRefplaneData_c";
    let (payload, root) = class_payload(class, &matrix_frame(0));
    assert_eq!(
        constraint_reference_plane_frame(&payload, root, class),
        None
    );
}

fn minimal_frame(
    origin: [f64; 3],
    normal: [f64; 3],
    translation: [f64; 3],
) -> [u8; minimal_plane::LEN] {
    let mut frame = [0; minimal_plane::LEN];
    for (index, value) in origin.into_iter().chain(normal).enumerate() {
        frame[8 * index..8 * index + 8].copy_from_slice(&value.to_le_bytes());
    }
    for (index, value) in translation.into_iter().chain([1.0]).enumerate() {
        let start = minimal_plane::TRANSLATION + 8 * index;
        frame[start..start + 8].copy_from_slice(&value.to_le_bytes());
    }
    frame
}

#[test]
fn a_line_point_plane_reads_its_81_byte_frame() {
    let class = "moPerLinePtRefPlnData_c";
    let origin = [-0.75, 0.35, -0.250_000_000_000_000_06];
    let negated = origin.map(|value: f64| -value);
    let up = [0.0, 0.0, 1.0];
    let read = |frame: &[u8]| {
        let (payload, root) = class_payload(class, frame);
        constraint_reference_plane_frame(&payload, root, class)
    };
    let expected = Some((
        Point3::new(origin[0] * 1000.0, origin[1] * 1000.0, origin[2] * 1000.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    ));
    let frame = minimal_frame(origin, up, negated);
    assert_eq!(read(&frame), expected);
    let mut followed = frame.to_vec();
    followed.extend([0; 40]);
    assert_eq!(read(&followed), expected);

    assert_eq!(read(&minimal_frame(origin, [0.0, 1.0, 0.0], negated)), None);
    assert_eq!(
        read(&minimal_frame(origin, up, [-0.0, -0.0, negated[2]])),
        None
    );
    let mut scaled = frame;
    scaled[minimal_plane::SCALE..minimal_plane::SCALE + 8].copy_from_slice(&2.0_f64.to_le_bytes());
    assert_eq!(read(&scaled), None);
    let mut marked = frame;
    marked[minimal_plane::FRAME_MARKER] = 1;
    assert_eq!(read(&marked), None);
    assert_eq!(
        read(&minimal_frame(
            [f64::NAN, 0.35, 0.0],
            up,
            [f64::NAN, -0.35, -0.0]
        )),
        None
    );
    assert_eq!(
        read(&minimal_frame([1e307, 0.0, 0.0], up, [-1e307, -0.0, -0.0])),
        None
    );
}
