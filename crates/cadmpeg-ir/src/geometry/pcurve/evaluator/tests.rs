// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

use super::PcurveEvaluatorLanes;
use crate::geometry::pcurve::{PcurveNurbsPoles, WeightedPole2};
use crate::math::Point2;
use crate::scalar::NonZeroReal;
use crate::units::FinitePoint2;

fn poles(rational: bool) -> PcurveNurbsPoles<FinitePoint2> {
    let points = [Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)]
        .map(|point| FinitePoint2::new(point).unwrap());
    if rational {
        PcurveNurbsPoles::Rational {
            points: points
                .into_iter()
                .zip([1.0, 2.0])
                .map(|(point, weight)| WeightedPole2 {
                    point,
                    weight: NonZeroReal::new(weight).unwrap(),
                })
                .collect(),
        }
    } else {
        PcurveNurbsPoles::Polynomial {
            points: points.to_vec(),
        }
    }
}

#[test]
fn evaluator_lanes_admit_each_copy_and_release_both_lanes() {
    for rational in [false, true] {
        let source = poles(rational);
        let point_work = 2 * u64_from_index(std::mem::size_of::<Point2>());
        let work = point_work
            + if rational {
                2 * u64_from_index(std::mem::size_of::<f64>())
            } else {
                0
            };
        for allowance in 0..=work {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = 4096;
            policy.limits.max_collection_items = if rational { 4 } else { 2 };
            policy.limits.max_recursion_depth = 0;
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = PcurveEvaluatorLanes::new(
                &ctx,
                &source,
                "evaluator point copy",
                "evaluator weight copy",
            );
            if allowance < work {
                let original = result.unwrap_err();
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(
                    original.operation,
                    if allowance < point_work {
                        "evaluator point copy"
                    } else {
                        "evaluator weight copy"
                    }
                );
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                );
            } else {
                let lanes = result.unwrap();
                assert_eq!(
                    lanes.points(),
                    [Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)]
                );
                assert_eq!(lanes.weights(), rational.then_some([1.0, 2.0].as_slice()));
                drop(lanes);
                drop(
                    ctx.reserve_scoped_limit(4096, "evaluator lanes released")
                        .unwrap(),
                );
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn evaluator_lanes_preserve_storage_and_slot_refusals() {
    let source = poles(true);
    for (dimension, slots, operation) in [
        (
            ResourceDimension::MaterializedBytes,
            4,
            "evaluator point copy",
        ),
        (
            ResourceDimension::CollectionItems,
            0,
            "evaluator point copy",
        ),
        (
            ResourceDimension::CollectionItems,
            2,
            "evaluator weight copy",
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = slots;
        if dimension == ResourceDimension::MaterializedBytes {
            policy.limits.max_materialized_bytes = 0;
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = PcurveEvaluatorLanes::new(
            &ctx,
            &source,
            "evaluator point copy",
            "evaluator weight copy",
        )
        .unwrap_err();
        assert_eq!(original.dimension, dimension);
        assert_eq!(original.operation, operation);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }
}

#[test]
fn evaluator_lanes_observe_fused_empty_input_and_keep_empty_weight_presence() {
    for rational in [false, true] {
        let source = if rational {
            PcurveNurbsPoles::Rational { points: Vec::new() }
        } else {
            PcurveNurbsPoles::Polynomial { points: Vec::new() }
        };
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let lanes = PcurveEvaluatorLanes::new(
            &ctx,
            &source,
            "empty evaluator points",
            "empty evaluator weights",
        )
        .unwrap();
        assert!(lanes.points().is_empty());
        assert_eq!(lanes.weights(), rational.then_some([].as_slice()));
        drop(lanes);
        let original = ctx
            .charge_work_limit(1, "original empty evaluator refusal")
            .unwrap_err();
        assert_eq!(
            PcurveEvaluatorLanes::new(
                &ctx,
                &source,
                "empty evaluator points",
                "empty evaluator weights"
            )
            .unwrap_err(),
            original
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
        );
    }
}
