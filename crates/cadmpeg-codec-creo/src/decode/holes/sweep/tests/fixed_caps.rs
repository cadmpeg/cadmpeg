// SPDX-License-Identifier: Apache-2.0

use super::super::{circular_sweep_cylinder_from_cap_outlines, CapOutline};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

const PLANES: [(u32, [f64; 3], [f64; 3]); 2] = [
    (1, [0.0; 3], [0.0, 0.0, 1.0]),
    (2, [0.0, 0.0, 2.0], [0.0, 0.0, -1.0]),
];

fn cap(index: usize, radius: f64) -> CapOutline {
    let (surface_id, origin, normal) = PLANES[index];
    CapOutline {
        surface_id,
        origin,
        normal,
        corners: [[-radius, -radius, origin[2]], [radius, radius, origin[2]]],
    }
}

#[test]
fn two_cap_selection_is_fixed_work_and_preserves_disagreement() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for outlines in [[Some(cap(0, 1.0)), None], [None, Some(cap(1, 1.0))],
        [Some(cap(0, 1.0)), Some(cap(1, 1.0))]] {
        let cylinder = circular_sweep_cylinder_from_cap_outlines(&ctx, PLANES, outlines)
            .expect("bounded selection").expect("unit cylinder");
        assert_eq!(cylinder.radius().get(), 1.0);
    }
    assert!(circular_sweep_cylinder_from_cap_outlines(&ctx, PLANES, [None, None])
        .expect("no cap").is_none());
    assert!(circular_sweep_cylinder_from_cap_outlines(&ctx, PLANES,
        [Some(cap(0, 1.0)), Some(cap(1, 2.0))]).expect("different radii").is_none());
}

#[test]
fn fixed_cap_selection_preserves_original_refusal_on_empty_and_full_rosters() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let original = ctx.charge_work_limit(1, "prior cap refusal").expect_err("refusal");
    for outlines in [[None, None], [Some(cap(0, 1.0)), Some(cap(1, 1.0))]] {
        assert!(matches!(circular_sweep_cylinder_from_cap_outlines(&ctx, PLANES, outlines),
            Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    }
}
