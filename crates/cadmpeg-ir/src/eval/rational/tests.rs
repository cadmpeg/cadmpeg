// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{finite_lanes, Homogeneous};
use crate::eval::decode::Scratch;
use crate::features::FinitePoint3;
use crate::math::Point3;

#[test]
fn homogeneous_sum_admits_every_pole_before_reading_it() {
    let points = [
        Point3::new(0.0, 2.0, 4.0),
        Point3::new(3.0, 2.0, 1.0),
        Point3::new(6.0, 2.0, -2.0),
    ]
    .map(|point| FinitePoint3::new(point).expect("finite pole"));
    for cap in 0..15 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let scratch = Scratch::new(&ctx);
        let reads = std::cell::Cell::new(0);
        let result = Homogeneous::sum(
            &scratch,
            points.into_iter().map(|point| {
                reads.set(reads.get() + 1);
                Some(([1.0, 1.0], 1.0, point))
            }),
        );
        let limit = result.err().expect("each pole read requires admission");
        assert_eq!(reads.get(), cap);
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "IR homogeneous pole traversal");
        assert_eq!(scratch.refused(), Some(limit));
        drop(scratch);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
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
    let sum = Homogeneous::sum(
        &scratch,
        points
            .into_iter()
            .map(|point| Some(([1.0, 1.0], 1.0, point))),
    )
    .expect("fifteen reads")
    .expect("sum");
    assert_eq!(
        finite_lanes(sum.project(sum, &[]).expect("nonzero denominator"))
            .expect("finite quotient")
            .map(crate::scalar::FiniteReal::get),
        [3.0, 2.0, 1.0]
    );
    drop(scratch);
    ctx.finish_session().expect("no storage or depth needed");
}

#[test]
fn homogeneous_sum_admits_exact_replay_and_preserves_cancellation() {
    let points =
        [-1.0, 0.0, 1.0].map(|x| FinitePoint3::new(Point3::new(x, 2.0, 0.0)).expect("finite pole"));
    for cap in [15, 18] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let scratch = Scratch::new(&ctx);
        let result = Homogeneous::sum(
            &scratch,
            points
                .into_iter()
                .map(|point| Some(([1.0, 1.0], 1.0, point))),
        );
        if cap == 15 {
            let limit = result
                .err()
                .expect("exact replay needs three additional reads");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(scratch.refused(), Some(limit));
            drop(scratch);
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
            );
        } else {
            let sum = result.expect("exact replay admitted").expect("sum");
            assert_eq!(
                finite_lanes(sum.project(sum, &[]).expect("nonzero denominator"))
                    .expect("finite quotient")
                    .map(crate::scalar::FiniteReal::get),
                [0.0, 2.0, 0.0]
            );
            drop(scratch);
            ctx.finish_session().expect("replay succeeded");
        }
    }
}

fn weight_sum(weight: Option<crate::math::sum::ScaledValue>) -> Homogeneous {
    Homogeneous {
        values: [None, None, None, weight],
        constant: [None; 3],
    }
}

#[test]
fn homogeneous_weights_admit_actual_scans_and_copies() {
    use crate::math::sum::scaled_finite;
    let normal = [
        weight_sum(scaled_finite(1.0)),
        weight_sum(scaled_finite(2.0)),
    ];
    let tiny = [
        weight_sum(scaled_finite(f64::from_bits(1))),
        weight_sum(scaled_finite(f64::from_bits(2))),
    ];
    let large = scaled_finite(f64::MAX).unwrap().doubled();
    let overflow = [weight_sum(Some(large)); 2];
    let impossible = [
        weight_sum(Some(large)),
        weight_sum(scaled_finite(f64::from_bits(1))),
    ];
    let cases = [
        (&normal[..], 22, Some(vec![1.0, 2.0])),
        (
            &tiny[..],
            41,
            Some(vec![f64::MIN_POSITIVE, f64::MIN_POSITIVE * 2.0]),
        ),
        (&overflow[..], 23, Some(vec![f64::MAX * 0.5; 2])),
        (&impossible[..], 5, None),
    ];
    for (values, work, expected) in cases {
        for allowance in 0..=work {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_materialized_bytes = 128;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 2;
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let scratch = Scratch::new(&ctx);
            let result = Homogeneous::weights(&scratch, values);
            if allowance < work {
                let original = result.unwrap_err();
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(scratch.refused(), Some(original));
                drop(scratch);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                );
            } else {
                assert_eq!(result.unwrap(), expected);
                drop(scratch);
                drop(
                    ctx.reserve_scoped_limit(128, "weight workspace released")
                        .unwrap(),
                );
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn homogeneous_weights_preserve_early_missing_and_empty_fuse() {
    let missing = [
        weight_sum(None),
        weight_sum(crate::math::sum::scaled_finite(1.0)),
    ];
    for (values, work, expected) in [
        (&missing[..], 1, None),
        (&[][..], 0, Some(Vec::<f64>::new())),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        assert_eq!(Homogeneous::weights(&scratch, values).unwrap(), expected);
        drop(scratch);
        ctx.finish_session().unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx
            .charge_work_limit(work + 1, "original empty weights refusal")
            .unwrap_err();
        let scratch = Scratch::new(&ctx);
        assert_eq!(Homogeneous::weights(&scratch, &[]).unwrap_err(), original);
        assert_eq!(scratch.refused(), Some(original));
        drop(scratch);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }
}

#[test]
fn homogeneous_weight_storage_is_scoped_and_slots_are_admitted_once() {
    let values = [weight_sum(crate::math::sum::scaled_finite(1.0)); 2];
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 1,
            _ => unreachable!(),
        }
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let original = Homogeneous::weights(&scratch, &values).unwrap_err();
        assert_eq!(original.dimension, dimension);
        assert_eq!(original.operation, "IR homogeneous output weights");
        assert_eq!(scratch.refused(), Some(original));
        drop(scratch);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 128;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = Scratch::new(&ctx);
    let weights = Homogeneous::weights(&scratch, &values).unwrap().unwrap();
    assert_eq!(weights, [1.0; 2]);
    let original = ctx
        .reserve_scoped_limit(128, "weights remain live")
        .unwrap_err();
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert!(original.used > 0);
    drop(weights);
    drop(scratch);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}
