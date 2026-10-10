// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{
    cone_half_angle_before_close, first_compound_close, plane_local_system_compound_close,
    surface_body_compound_close, ExtrusionVariant, SurfaceKind,
};

fn assert_work_bound(
    body: &[u8],
    work: u64,
    expected: Option<usize>,
    run: impl Fn(&DecodeContext<'_>) -> Result<Option<usize>, CodecError>,
) {
    for cap in 0..=work + 1 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(body, &arena, &policy).expect("root");
        let original = if cap < work {
            let CodecError::ResourceLimit(limit) = run(&ctx).expect_err("scan refuses before step") else {
                panic!("resource refusal required");
            };
            assert_eq!((limit.dimension, limit.used, limit.additional),
                (ResourceDimension::WorkUnits, cap, 1));
            limit
        } else {
            assert_eq!(run(&ctx).expect("exact scan bound"), expected);
            let limit = ctx.charge_work_limit(cap - work + 1, "after close scan")
                .expect_err("scan consumed exactly its source-derived bound");
            assert_eq!((limit.dimension, limit.used, limit.additional),
                (ResourceDimension::WorkUnits, work, cap - work + 1));
            limit
        };
        for _ in 0..2 {
            assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn cone_close_scan_counts_only_possible_starts_and_stops_at_second_layout() {
    const ANGLE_CLOSE: [u8; 8] = [0x71, 0, 0, 0, 0, 0, 0, 0xe3];
    for prefix in [0, 1, 7, 17, 257] {
        let mut body = vec![0xed; prefix];
        body.extend_from_slice(&ANGLE_CLOSE);
        // Only len-7 starts can own the seven-byte token and its close.
        assert_work_bound(&body, u64_from_index(prefix + 1), Some(prefix + 7), |ctx| {
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
        assert_work_bound(&body, u64_from_index(prefix + 9), None, |ctx| {
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
        assert_work_bound(&body, u64_from_index(prefix + 1), None, |ctx| {
            surface_body_compound_close(ctx, SurfaceKind::Cylinder, &body, &cache)
        });
        body.push(0xe3);
        body.extend_from_slice(&[0xed; 31]);
        assert_work_bound(&body, u64_from_index(prefix + 2), Some(prefix + 8), |ctx| {
            surface_body_compound_close(ctx, SurfaceKind::Cylinder, &body, &cache)
        });
    }
    assert_work_bound(&[0x46, 0xe3], 2, Some(1), |ctx| {
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
        assert_work_bound(&body, u64_from_index(2 * prefix + 5), Some(prefix + 3), |ctx| {
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
        let work = count.saturating_sub(3) + 2 * count;
        assert_work_bound(&body, u64_from_index(work), None, |ctx| {
            plane_local_system_compound_close(ctx, &body, 0, body.len(), &cache)
        });
    }
}

#[test]
fn absent_surface_close_routes_are_free_and_preserve_original_refusal() {
    let cache = ScalarCache::default();
    for count in 0..8 {
        let body = [0x71; 7];
        assert_work_bound(&body[..count], 0, None, |ctx| {
            Ok(cone_half_angle_before_close(ctx, &body[..count])?.map(|layout| layout.end))
        });
    }
    for kind in [SurfaceKind::Plane, SurfaceKind::Cylinder, SurfaceKind::Cone,
        SurfaceKind::TorusOrSphere, SurfaceKind::Spline, SurfaceKind::Fillet,
        SurfaceKind::Extrusion(ExtrusionVariant::Linear),
        SurfaceKind::Extrusion(ExtrusionVariant::TabulatedCylinder)] {
        assert_work_bound(&[], 0, None, |ctx| surface_body_compound_close(ctx, kind, &[], &cache));
    }
    for (start, end) in [(0, 0), (1, 0), (0, 1)] {
        assert_work_bound(&[], 0, None, |ctx| first_compound_close(ctx, &[], start, end));
    }
    assert_work_bound(&[], 0, None, |ctx| {
        plane_local_system_compound_close(ctx, &[], 0, 0, &cache)
    });
}
