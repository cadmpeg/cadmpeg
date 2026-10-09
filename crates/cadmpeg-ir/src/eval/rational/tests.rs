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
fn projection_preserves_subnormal_rounding_and_exact_constants() {
    use crate::math::sum::scaled_finite;
    use crate::scalar::FiniteReal;
    let least = f64::from_bits(1);
    // Each numerator is nonzero. Dividing by two respectively preserves
    // one subnormal unit, ties to zero, or stays in the normal range.
    let base = weight_sum(scaled_finite(2.0));
    for (numerator, expected) in [
        (2.0 * least, least),
        (least, 0.0),
        (2.0 * f64::MIN_POSITIVE, f64::MIN_POSITIVE),
    ] {
        let sum = Homogeneous {
            values: [scaled_finite(numerator), None, None, None],
            constant: [None; 3],
        };
        assert_eq!(sum.project(base, &[]).unwrap()[0].unwrap().get(), expected);
    }
    assert_eq!(Homogeneous::zero().project(base, &[]),
        Some([Ok(FiniteReal::ZERO); 3]));
    // An unchanged coordinate needs no quotient rounding. Preserve its
    // actual bits, including a subnormal and negative zero.
    let constants = [least, -0.0, f64::MAX].map(|value| FiniteReal::new(value).unwrap());
    let sum = Homogeneous { values: [None; 4], constant: constants.map(Some) };
    let lanes = sum.project(base, &[]).unwrap().map(Result::unwrap);
    assert_eq!(lanes.map(|value| value.get().to_bits()), constants.map(|value| value.get().to_bits()));
    assert!(sum.project(weight_sum(None), &[]).is_none());
}

#[test]
fn quadratic_orders_keep_tiny_coefficients_and_weight_derivative_cancellation() {
    use crate::geometry::nurbs::WeightedPole3;
    use crate::scalar::{FiniteReal, NonZeroReal};
    let s = 2.0_f64.powi(-600);
    let q = 2.0_f64.powi(300);
    assert_eq!(s * s, 0.0);
    for exponent in [-900, 0, 900] {
        for sign in [-1.0, 1.0] {
            let common = sign * 2.0_f64.powi(exponent);
            let poles = [(0.0, common), (0.0, common), (q, 2.0 * common)]
                .map(|(x, weight)| WeightedPole3 {
                    point: FinitePoint3::new(Point3::new(x, -0.0, 0.0)).unwrap(),
                    weight: NonZeroReal::new(weight).unwrap(),
                });
            let [base, first, second] = Homogeneous::quadratic_orders(&poles,
                FiniteReal::new(s).unwrap()).unwrap();
            // W=common*(1+s²), W'=2*common*s, W''=2*common.
            // The complete W rounds to common. Differencing rounded
            // Bernstein coefficients would instead erase or double W'.
            let weight = base.values[3].unwrap();
            assert_eq!(first.values[3].unwrap().quotient(weight).unwrap().get(), 2.0 * s);
            assert_eq!(second.values[3].unwrap().quotient(weight).unwrap().get(), 2.0);
            // C=2q*s²/(1+s²). Its value and first two derivatives
            // differ from these powers of two below binary64 resolution.
            let point = finite_lanes(base.project(base, &[]).unwrap()).unwrap();
            assert_eq!(point[0].get(), 2.0_f64.powi(-899));
            assert_eq!(point[1].get().to_bits(), (-0.0_f64).to_bits());
            let tangent = finite_lanes(first.project(base, &[(first, point)]).unwrap()).unwrap();
            assert_eq!(tangent[0].get(), 2.0_f64.powi(-298));
            let acceleration = finite_lanes(second.project(base,
                &[(second, point), (first, tangent), (first, tangent)]).unwrap()).unwrap();
            assert_eq!(acceleration[0].get(), 2.0_f64.powi(302));
        }
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

#[test]
fn quadratic_third_uses_complete_source_constant_equality_and_nonzero_weight() {
    use crate::geometry::nurbs::WeightedPole3;
    use crate::math::sum::scaled_finite;
    use crate::scalar::{FiniteReal, NonZeroReal};
    let width = scaled_finite(f64::from_bits(1)).unwrap();
    let poles = [1.0, 1.0, 2.0].map(|weight| WeightedPole3 {
        point: FinitePoint3::new(Point3::new(f64::MAX, -0.0, f64::from_bits(1))).unwrap(),
        weight: NonZeroReal::new(weight).unwrap(),
    });
    assert_eq!(Homogeneous::quadratic_third(&poles, FiniteReal::new(0.5).unwrap(), width),
        Some([Ok(FiniteReal::ZERO); 3]));
    let poles = [1.0, -1.0, 1.0].map(|weight| WeightedPole3 {
        point: FinitePoint3::new(Point3::new(f64::MAX, -0.0, f64::from_bits(1))).unwrap(),
        weight: NonZeroReal::new(weight).unwrap(),
    });
    assert!(Homogeneous::quadratic_third(&poles, FiniteReal::new(0.5).unwrap(), width).is_none());
}
