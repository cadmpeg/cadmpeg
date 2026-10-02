// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{finite_lanes, Homogeneous};
use crate::eval::decode::Scratch;
use crate::features::FinitePoint3;
use crate::math::Point3;

#[test]
fn homogeneous_sum_admits_every_pole_before_reading_it() {
    let points = [Point3::new(0.0, 2.0, 4.0), Point3::new(3.0, 2.0, 1.0), Point3::new(6.0, 2.0, -2.0)]
        .map(|point| FinitePoint3::new(point).expect("finite pole"));
    for cap in 0..15 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let scratch = Scratch::new(&ctx);
        let reads = std::cell::Cell::new(0);
        let result = Homogeneous::sum(&scratch, points.into_iter().map(|point| {
            reads.set(reads.get() + 1);
            Some(([1.0, 1.0], 1.0, point))
        }));
        let limit = result.err().expect("each pole read requires admission");
        assert_eq!(reads.get(), cap);
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "IR homogeneous pole traversal");
        assert_eq!(scratch.refused(), Some(limit));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 15;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let scratch = Scratch::new(&ctx);
    let sum = Homogeneous::sum(&scratch, points.into_iter().map(|point| Some(([1.0, 1.0], 1.0, point))))
        .expect("fifteen reads").expect("sum");
    assert_eq!(finite_lanes(sum.project(sum, &[]).expect("nonzero denominator"))
        .expect("finite quotient").map(|value| value.get()), [3.0, 2.0, 1.0]);
    drop(scratch);
    ctx.finish_session().expect("no storage or depth needed");
}

#[test]
fn homogeneous_sum_admits_exact_replay_and_preserves_cancellation() {
    let points = [-1.0, 0.0, 1.0]
        .map(|x| FinitePoint3::new(Point3::new(x, 2.0, 0.0)).expect("finite pole"));
    for cap in [15, 18] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let scratch = Scratch::new(&ctx);
        let result = Homogeneous::sum(&scratch, points.into_iter().map(|point| Some(([1.0, 1.0], 1.0, point))));
        if cap == 15 {
            let limit = result.err().expect("exact replay needs three additional reads");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(scratch.refused(), Some(limit));
            drop(scratch);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        } else {
            let sum = result.expect("exact replay admitted").expect("sum");
            assert_eq!(finite_lanes(sum.project(sum, &[]).expect("nonzero denominator"))
                .expect("finite quotient").map(|value| value.get()), [0.0, 2.0, 0.0]);
            drop(scratch);
            ctx.finish_session().expect("replay succeeded");
        }
    }
}
