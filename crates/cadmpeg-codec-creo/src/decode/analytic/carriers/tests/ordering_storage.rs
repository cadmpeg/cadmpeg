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

#[test]
fn accepted_face_ordering_promotes_only_surviving_reference_backing() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (10..13).map(|curve_id| crate::topology::HalfEdgeId {
            curve_id, side: crate::topology::Side::Zero,
        }).collect(),
    );
    // extend_from_slice reserves the amortized minimum of four reference slots.
    let bytes = (4 * std::mem::size_of::<&crate::topology::Loop>()) as u64;
    let plane = super::super::PlaneEquation {
        origin: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0],
    };
    crate::test_support::assert_refusal_order(ResourceDimension::RetainedBytes, &["creo face ordering candidate references"], |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_collection_items = 1;
        policy.limits.max_recursion_depth = 0;
        // One source slot plus the complete copied reference representation.
        policy.limits.max_work_units = 1 + std::mem::size_of::<&crate::topology::Loop>() as u64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = super::super::ordered_face_loops(&ctx, &[&lp], Some(plane), &BTreeMap::new(), &BTreeMap::new());
        let original = if result.is_ok() {
            assert_eq!(cap, bytes);
            let ordered = result.expect("exact retained backing").expect("one loop");
            assert_eq!(ordered.len(), 1);
            assert!(std::ptr::eq(ordered[0], &lp));
            let refunded = ctx.reserve_scoped(bytes, "refunded accepted face ordering scratch").expect("scratch released while result survives");
            drop(refunded);
            let original = ctx.charge_retained_limit(1, "after accepted face ordering").expect_err("reference backing stays retained");
            assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::RetainedBytes, bytes, 1));
            assert!(std::ptr::eq(ordered[0], &lp));
            original
        } else {
            let original = ctx.resource_refusal().expect("promotion refusal");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!((original.dimension, original.limit, original.used, original.additional, original.operation),
                (ResourceDimension::RetainedBytes, cap, 0, bytes, "creo face ordering candidate references"));
            original
        };
        assert!(matches!(super::super::ordered_face_loops(&ctx, &[&lp], Some(plane), &BTreeMap::new(), &BTreeMap::new()), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        if cap == bytes { Ok(()) } else { Err(CodecError::ResourceLimit(original)) }
    });
}

#[test]
fn accepted_face_ordering_preserves_prior_refusal_before_value_promotion() {
    use cadmpeg_core::CodecError;
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (10..13).map(|curve_id| crate::topology::HalfEdgeId {
            curve_id, side: crate::topology::Side::Zero,
        }).collect(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let original = ctx.charge_collection_items_limit(1, "before face ordering promotion").expect_err("seeded refusal");
    assert!(matches!(super::super::ordered_face_loops(&ctx, &[&lp], None, &BTreeMap::new(), &BTreeMap::new()), Err(CodecError::ResourceLimit(actual)) if actual == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}
