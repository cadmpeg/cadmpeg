// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{
    cone_half_angle_before_close, first_compound_close, plane_local_system_compound_close,
    surface_body_compound_close, ExtrusionVariant, SurfaceKind,
};

fn assert_work_bound(
    expected: Option<usize>,
    run: impl Fn(&DecodeContext<'_>) -> Result<Option<usize>, CodecError>,
) {
    assert_eq!(super::work_output(run), expected);
}

#[test]
fn cone_close_scan_counts_only_possible_starts_and_stops_at_second_layout() {
    const ANGLE_CLOSE: [u8; 8] = [0x71, 0, 0, 0, 0, 0, 0, 0xe3];
    for prefix in [0, 1, 7, 17, 257] {
        let mut body = vec![0xed; prefix];
        body.extend_from_slice(&ANGLE_CLOSE);
        // Only len-7 starts can own the seven-byte token and its close.
        assert_work_bound(Some(prefix + 7), |ctx| {
            Ok(cone_half_angle_before_close(ctx, &body)?.map(|layout| {
                assert_eq!(layout.start, prefix);
                assert_eq!(layout.value.get().get(), 0.6875);
                layout.end
            }))
        });
        body.extend_from_slice(&ANGLE_CLOSE);
        body.extend_from_slice(&[0xed; 31]);
        // A second layout starts eight bytes after the first. Ambiguity stops
        // there, before the later suffix can run any scalar dispatch.
        assert_work_bound(None, |ctx| {
            Ok(cone_half_angle_before_close(ctx, &body)?.map(|layout| layout.end))
        });
    }
}

#[test]
fn surface_close_scan_counts_dispatches_and_preserves_scalar_owned_close_bytes() {
    const SCALAR: [u8; 8] = [0x46, 0xe3, 0, 0, 0, 0, 0, 0];
    let cache = ScalarCache::default();
    for prefix in [0, 1, 7, 17, 257] {
        let mut body = vec![0xed; prefix];
        body.extend_from_slice(&SCALAR);
        assert_work_bound(None, |ctx| {
            surface_body_compound_close(ctx, SurfaceKind::Cylinder, &body, &cache)
        });
        body.push(0xe3);
        body.extend_from_slice(&[0xed; 31]);
        assert_work_bound(Some(prefix + 8), |ctx| {
            surface_body_compound_close(ctx, SurfaceKind::Cylinder, &body, &cache)
        });
    }
    assert_work_bound(Some(1), |ctx| {
        surface_body_compound_close(ctx, SurfaceKind::Cylinder, &[0x46, 0xe3], &cache)
    });
}

#[test]
fn outline_pair_close_scan_preserves_precedence_and_exact_visited_window_bound() {
    for prefix in [0, 1, 7, 17, 257] {
        let mut body = vec![0xed; prefix];
        body.extend_from_slice(&[0, 0x0c, 0x98, 0xe3, 0xe3]);
        // The window scan visits prefix+1 starts. The token scan visits the
        // prefix, two one-byte integers, the two-byte 98 e3 integer, and e3.
        assert_work_bound(Some(prefix + 3), |ctx| {
            first_compound_close(ctx, &body, 0, body.len())
        });
    }
}

#[test]
fn plane_close_fallback_counts_each_candidate_without_terminal_probe() {
    let cache = ScalarCache::default();
    for count in [0usize, 1, 2, 3, 7, 17, 257] {
        let body = vec![0xed; count];
        // No marker or close exists. Windows visit max(count-3,0), PSB visits
        // count unknown tokens, and fallback visits exactly count candidates.
        assert_work_bound(None, |ctx| {
            plane_local_system_compound_close(ctx, &body, 0, body.len(), &cache)
        });
    }
}

#[test]
fn absent_surface_close_routes_are_free_and_preserve_original_refusal() {
    let cache = ScalarCache::default();
    for count in 0..8 {
        let body = [0x71; 7];
        assert_work_bound(None, |ctx| {
            Ok(cone_half_angle_before_close(ctx, &body[..count])?.map(|layout| layout.end))
        });
    }
    for kind in [
        SurfaceKind::Plane,
        SurfaceKind::Cylinder,
        SurfaceKind::Cone,
        SurfaceKind::TorusOrSphere,
        SurfaceKind::Spline,
        SurfaceKind::Fillet,
        SurfaceKind::Extrusion(ExtrusionVariant::Linear),
        SurfaceKind::Extrusion(ExtrusionVariant::TabulatedCylinder),
    ] {
        assert_work_bound(None, |ctx| {
            surface_body_compound_close(ctx, kind, &[], &cache)
        });
    }
    for (start, end) in [(0, 0), (1, 0), (0, 1)] {
        assert_work_bound(None, |ctx| first_compound_close(ctx, &[], start, end));
    }
    assert_work_bound(None, |ctx| {
        plane_local_system_compound_close(ctx, &[], 0, 0, &cache)
    });
}
