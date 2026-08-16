//! Frame-layout ownership regressions.

use super::{compact_reference_plane_frame_candidates, explicit_reference_plane_frame};
use crate::layout::constructed_reference_plane_matrix_frame as matrix_plane;
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn matrix_reference_plane_owns_overlapping_compact_scan_window() {
    let mut payload = [0_u8; matrix_plane::LEN].to_vec();
    for (relative, value) in [
        (24, 1.0_f64),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, -1.0),
        (81, 0.0),
        (89, 0.0),
        (97, 0.0),
        (105, -1.0),
        (113, 0.0),
    ] {
        payload[relative..relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[48] = 1;

    assert!(compact_reference_plane_frame_candidates(&payload)
        .iter()
        .any(|(offset, _)| *offset == 33));
    assert_eq!(
        explicit_reference_plane_frame(&payload),
        Ok(Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        )))
    );
}
