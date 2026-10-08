// SPDX-License-Identifier: Apache-2.0
//! Rejected face orderings release their candidate reference lists.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn rejected_face_ordering_retains_no_loop_references() {
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (10..13).map(|curve_id| crate::topology::HalfEdgeId {
            curve_id,
            side: crate::topology::Side::Zero,
        }).collect(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // An empty root has the core's 16 MiB materialized base allowance.
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for _ in 0..2 {
        assert!(super::super::ordered_face_loops(
            &ctx, &[&lp, &lp], None, &BTreeMap::new(), &BTreeMap::new(),
        ).expect("rejected ordering uses scratch only").is_none());
    }
    ctx.reserve_scoped(ctx.policy().limits.max_materialized_bytes, "released face ordering scratch")
        .expect("candidate lists are released");
}
