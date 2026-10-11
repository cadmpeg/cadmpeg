//! Frame-layout ownership regressions.

use super::{
    explicit_reference_plane_frame, for_each_angled_reference_plane_frame,
    for_each_compact_reference_plane_frame,
};
use crate::layout::constructed_reference_plane_matrix_frame as matrix_plane;
use cadmpeg_ir::math::{Point3, Vector3};

fn has_compact_reference_plane_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    expected_offset: usize,
) -> bool {
    let mut found = false;
    for_each_compact_reference_plane_frame(ctx, payload, |offset, _| {
        if offset == expected_offset {
            found = true;
            Ok(false)
        } else {
            Ok(true)
        }
    })
    .expect("compact plane frame scan fits service policy");
    found
}

fn has_angled_reference_plane_candidate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    payload: &[u8],
    expected_offset: usize,
) -> bool {
    let mut found = false;
    for_each_angled_reference_plane_frame(ctx, payload, |offset, _| {
        if offset == expected_offset {
            found = true;
            Ok(false)
        } else {
            Ok(true)
        }
    })
    .expect("angled plane frame scan fits service policy");
    found
}

#[test]
fn matrix_reference_plane_owns_overlapping_compact_scan_window() {
    let ctx = cadmpeg_test_support::service_decode_context();
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

    assert!(has_compact_reference_plane_candidate(&ctx, &payload, 33));
    assert_eq!(
        explicit_reference_plane_frame(&ctx, &payload).unwrap(),
        Ok(Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, -1.0, 0.0),
        )))
    );
}

#[test]
fn matrix_reference_plane_owns_overlapping_angled_scan_window() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut payload = [0_u8; 226].to_vec();
    for (relative, value) in [
        (24, 1.0_f64),
        (32, 0.0),
        (40, 0.0),
        (49, 0.0),
        (57, 0.0),
        (65, 1.0),
        (73, 1.0),
        (81, 0.0),
        (89, 0.0),
        (97, 0.0),
        (105, 1.0),
        (113, 0.0),
    ] {
        payload[relative..relative + 8].copy_from_slice(&value.to_le_bytes());
    }
    payload[48] = 1;
    payload[121] = 1;
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    for (offset, value) in [
        (122, diagonal),
        (130, diagonal),
        (138, 0.0),
        (146, 0.0),
        (154, 0.0),
        (162, 1.0),
        (170, -diagonal),
        (178, diagonal),
        (186, 0.0),
        (218, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    assert!(has_angled_reference_plane_candidate(&ctx, &payload, 105));
    assert_eq!(
        explicit_reference_plane_frame(&ctx, &payload).unwrap(),
        Ok(Some((
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        )))
    );
}
