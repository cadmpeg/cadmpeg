// SPDX-License-Identifier: Apache-2.0

use super::super::single_plane_extrusion_extent_and_direction;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};

#[test]
fn degenerate_variable_plane_span_preserves_original_refusal_without_visiting_rows() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let plane = ([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]);
    assert_eq!(super::super::extrusion_span(&ctx, [0.0; 3], [0.0; 3], [plane])
        .expect("no variable traversal"), None);
    let original = ctx.charge_work_limit(1, "prior variable span refusal").expect_err("refusal");
    assert!(matches!(super::super::extrusion_span(&ctx, [0.0; 3], [0.0; 3], [plane]),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(matches!(super::super::extrusion_extent_and_direction(
        &ctx, [0.0; 3], [0.0; 3], [plane]),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn single_plane_extent_is_fixed_work_for_both_offset_signs() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for sign in [-1.0, 1.0] {
        let result = single_plane_extrusion_extent_and_direction(
            &ctx, [1.0, 2.0, 3.0], [0.0, 0.0, 2.0],
            ([1.0, 2.0, 3.0 + sign * 6.0], [0.0, 0.0, -3.0]),
        ).expect("one plane");
        assert_eq!(result, Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(6.0)
                            .expect("six-unit extent"),
                    },
                    draft: None,
                },
            },
            [0.0 * sign, 0.0 * sign, sign],
        )));
    }
}

#[test]
fn single_plane_extent_keeps_degenerate_and_overflow_rejections() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for (profile, direction, plane) in [
        ([0.0; 3], [0.0; 3], ([0.0, 0.0, 2.0], [0.0, 0.0, 1.0])),
        ([0.0; 3], [0.0, 0.0, 1.0], ([0.0, 0.0, 2.0], [0.0; 3])),
        ([0.0; 3], [0.0, 0.0, 1.0], ([0.0, 0.0, 2.0], [1.0, 0.0, 0.0])),
        ([0.0; 3], [0.0, 0.0, 1.0], ([0.0; 3], [0.0, 0.0, 1.0])),
        ([0.0, 0.0, -f64::MAX], [0.0, 0.0, 1.0],
            ([0.0, 0.0, f64::MAX], [0.0, 0.0, 1.0])),
    ] {
        assert_eq!(single_plane_extrusion_extent_and_direction(&ctx, profile, direction, plane)
            .expect("bounded rejection"), None);
    }
}

#[test]
fn single_plane_extent_keeps_original_refusal_before_degenerate_returns() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let original = ctx.charge_work_limit(1, "prior plane refusal").expect_err("refusal");
    for direction in [[0.0; 3], [0.0, 0.0, 1.0]] {
        assert!(matches!(single_plane_extrusion_extent_and_direction(
            &ctx, [0.0; 3], direction, ([0.0, 0.0, 2.0], [0.0, 0.0, 1.0])),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    }
}
