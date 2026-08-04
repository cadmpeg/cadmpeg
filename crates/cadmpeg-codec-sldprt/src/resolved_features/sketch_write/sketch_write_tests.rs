//! Tests for the `sketch_write` module.

use super::super::reference_geometry::{
    minimal_reference_plane_frame, offset_reference_plane_frame_pair,
};
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

    let (offset, reference) = offset_reference_plane_frame_pair(&payload, 27.25).unwrap();
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
