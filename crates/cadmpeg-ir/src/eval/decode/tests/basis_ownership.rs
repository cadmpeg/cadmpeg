// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn reusable_basis_constructor_preserves_initialization_refusal_and_backing_lifetime() {
    let curve = super::curve();
    let support = usize::try_from(curve.degree()).unwrap() + 1;
    let bytes = u64::try_from(support * std::mem::size_of::<f64>()).unwrap();
    for work in 0..=u64::try_from(support).unwrap() {
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_work_units = work;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::NurbsPointEvaluator::new(&ctx, &curve);
        if work < u64::try_from(support).unwrap() {
            let limit = result.err().expect("basis initialization needs each slot");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(limit.operation, "IR B-spline basis work");
            assert_eq!(limit.used, 0);
            assert_eq!(limit.additional, u64::try_from(support).unwrap());
            assert_eq!(ctx.charge_work_limit(0, "observe basis constructor refusal"), Err(limit));
            assert_eq!(super::super::NurbsPointEvaluator::new(&ctx, &curve).err(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
        } else {
            let evaluator = result.expect("exact initialization work");
            assert_eq!(&*evaluator.basis, &[0.0; 3]);
            assert!(evaluator._storage.is_some());
            drop(evaluator);
            let reuse = ctx.reserve_scoped_limit(bytes, "basis backing released").unwrap();
            drop(reuse);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn reusable_basis_constructor_observes_original_refusal_before_inline_or_heap_storage() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;

    for degree in 0..=3 {
        let count = usize::try_from(degree).unwrap() + 1;
        let knots: Vec<f64> = (0..count).map(|_| 0.0).chain((0..count).map(|_| 1.0)).collect();
        let points: Vec<Point3> = (0..count).map(|_| Point3::new(0.0, 0.0, 0.0)).collect();
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            degree,
            knots,
            points,
            None,
            false,
        ).expect("fixture admission").expect("valid constant coordinates");
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = ctx.charge_work_limit(1, "original evaluator constructor refusal").unwrap_err();
        let actual = super::super::NurbsPointEvaluator::new(&ctx, &curve).err().expect("original refusal");
        assert_eq!(actual, original);
        assert_eq!(ctx.charge_work_limit(0, "observe evaluator constructor refusal"), Err(original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
}

#[test]
fn reusable_basis_mutation_observes_its_original_session_before_scalar_or_depth() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;

    for degree in 0..=3 {
        let count = usize::try_from(degree).unwrap() + 1;
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            degree,
            (0..count).map(|_| 0.0).chain((0..count).map(|_| 1.0)).collect::<Vec<f64>>(),
            (0..count).map(|_| Point3::new(0.0, 0.0, 0.0)).collect::<Vec<Point3>>(),
            None,
            false,
        ).expect("fixture admission").expect("finite constant coordinates");
        let mut policy = DecodePolicy::service();
        let initialization_work = if count > 2 { u64::try_from(count).unwrap() } else { 0 };
        policy.limits.max_work_units = initialization_work;
        policy.limits.max_materialized_bytes = u64::try_from(count * std::mem::size_of::<f64>()).unwrap();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = u64::try_from(count).unwrap();
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut evaluator = super::super::NurbsPointEvaluator::new(&ctx, &curve).unwrap();
        let original = ctx.charge_work_limit(1, "original reusable basis refusal").unwrap_err();
        for parameter in [f64::NAN, 0.5] {
            assert_eq!(evaluator.point(parameter), Err(original));
            assert!(evaluator.basis.iter().all(|value| *value == 0.0));
        }
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!((original.used, original.additional), (initialization_work, 1));
        assert_eq!(original.operation, "original reusable basis refusal");
        drop(evaluator);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
}
