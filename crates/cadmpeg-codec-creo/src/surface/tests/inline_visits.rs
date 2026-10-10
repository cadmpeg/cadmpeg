// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{inline_local_starts, inline_surface_body, inline_surface_suffix_body,
    SurfaceBodyBoundary, SurfaceKind, SurfaceParameterCarrier, SurfaceParameterRecord};

fn check_work<T: PartialEq + std::fmt::Debug>(
    expected: T, run: impl Fn(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    assert_eq!(super::work_output(run), expected);
}

fn record(kind: SurfaceKind, body: Vec<u8>) -> SurfaceParameterRecord {
    SurfaceParameterRecord {
        surface_id: 7,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: SurfaceParameterCarrier::Unresolved(kind),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 6,
    }
}

#[test]
fn inline_local_start_iterator_owns_only_visited_boundaries_and_no_eof() {
    for count in [0_usize, 1, 7, 17, 257] {
        for byte in [0xff, 0xe3] {
            let body = vec![byte; count];
            let mut expected = vec![0];
            if byte == 0xe3 { expected.extend(1..=count); }
            check_work(expected, |ctx| inline_local_starts(ctx, &body).collect::<Result<Vec<_>, _>>());
        }
    }
}

#[test]
fn inline_suffix_routes_admit_each_actual_body_byte_before_framing() {
    for kind in [SurfaceKind::Cylinder, SurfaceKind::Cone, SurfaceKind::TorusOrSphere] {
        for count in [0_usize, 1, 7, 17, 257] {
            for byte in [0xff, 0xe3] {
                let body = vec![byte; count];
                let cache = ScalarCache::default();
                check_work(None, |ctx| {
                    inline_surface_suffix_body(ctx, kind, &body, &cache)
                        .map(|value| value.map(|value| (value.terminal_close, value.carrier)))
                });
                check_work(None, |ctx| {
                    inline_surface_body(ctx, kind, &body, &cache)
                        .map(|value| value.map(|value| (value.terminal_close, value.carrier)))
                });
                let record = record(kind, body);
                check_work(false, |ctx| record.has_inline_non_plane_local_system_suffix(ctx));
            }
        }
    }
}

#[test]
fn inline_envelope_terminal_scan_admits_its_tail_and_executed_fallback() {
    // Nine scalar slots with a separator after the first two, followed by
    // the envelope close. No following byte can start a local frame.
    let envelope = [0x0f, 0xe4, 0x12, 0x2f, 0, 0, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0x0f, 0xe3];
    for count in [0_usize, 1, 7, 17, 257] {
        let mut body = envelope.to_vec();
        body.extend(std::iter::repeat_n(0xff, count));
        // The terminal-candidate walk visits only the tail. With no layout,
        // the local-start fallback then visits the complete body once.
        check_work(None, |ctx| {
            inline_surface_body(ctx, SurfaceKind::Cylinder, &body, &ScalarCache::default())
                .map(|value| value.map(|value| (value.terminal_close, value.carrier)))
        });
    }
}

#[test]
fn inline_suffix_success_leaves_unvisited_frame_bytes_free() {
    // Compact Y frame, three origin scalars (2, 3, 4), unit radius.
    let local = [0x18, 0x10, 0x18, 0xe5, 0x10, 0x0f, 0x18, 0xe4,
        0x2f, 0, 0, 0x2e, 8, 0, 0x2f, 0x10, 0, 0x0f];
    let initial = record(SurfaceKind::Cylinder, local.to_vec());
    check_work(true, |ctx| initial.has_inline_non_plane_local_system_suffix(ctx));
    for count in [0_usize, 1, 7, 17, 257] {
        let mut body = vec![0xff; count];
        body.push(0xe3);
        body.extend_from_slice(&local);
        let record = record(SurfaceKind::Cylinder, body);
        check_work(true, |ctx| record.has_inline_non_plane_local_system_suffix(ctx));
    }
}

#[test]
fn inline_empty_and_other_family_routes_preserve_original_refusal() {
    for kind in [SurfaceKind::Plane, SurfaceKind::Spline, SurfaceKind::Fillet] {
        let record = record(kind, Vec::new());
        check_work(false, |ctx| record.has_inline_non_plane_local_system_suffix(ctx));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let original = ctx.charge_work_limit(1, "original").expect_err("fuse");
    let mut starts = inline_local_starts(&ctx, &[]);
    assert!(matches!(starts.next(), Some(Err(CodecError::ResourceLimit(actual))) if actual == original));
    assert!(starts.next().is_none());
    drop(starts);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
}
