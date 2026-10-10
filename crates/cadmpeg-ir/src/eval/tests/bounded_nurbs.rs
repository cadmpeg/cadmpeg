// SPDX-License-Identifier: Apache-2.0

#[test]
fn matching_polynomial_seed_avoids_full_control_and_interval_scratch() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let preparation = cadmpeg_test_support::service_decode_context();
    let mut knots = vec![0.0; 4];
    knots.extend((1..1_021).map(|ordinal| f64::from(ordinal) / 1_021.0));
    knots.extend([1.0; 4]);
    let curve = NurbsCurve::from_lanes(
        &preparation,
        3,
        knots,
        (0..1_024)
            .map(|ordinal| Point3::new(f64::from(ordinal), 0.0, 0.0))
            .collect(),
        None,
        false,
    )
    .expect("admitted authored spline")
    .expect("valid authored spline");
    let point = crate::eval::decode::nurbs_curve_point_at(&preparation, &curve, 0.5)
        .expect("seed evaluation")
        .get();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 1_000;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty input");
    assert_eq!(
        super::super::nurbs_curve_parameter_near_point(&ctx, &curve, point, 0.0, 0.5)
            .expect("matching witness needs no population-sized scratch"),
        Some(crate::scalar::FiniteReal::HALF)
    );
    ctx.finish_session().expect("all work remains admitted");
}

#[test]
fn matching_rational_seed_keeps_positive_weight_search_admission() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;
    let ctx = cadmpeg_test_support::service_decode_context();
    let point = Point3::new(0.0, 0.0, 0.0);
    let curve = NurbsCurve::from_lanes(
        &ctx,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![point, Point3::new(1.0, 0.0, 0.0)],
        Some(vec![-1.0, 1.0]),
        false,
    )
    .expect("admitted signed-weight fixture")
    .expect("valid signed-weight carrier");
    assert_eq!(
        super::super::nurbs_curve_parameter_near_point(&ctx, &curve, point, 0.0, 0.0)
            .expect("signed weights have no certified search"),
        None
    );
}

#[test]
fn bounded_nurbs_interval_search_keeps_a_fixed_working_set() {
    let boundaries = (0..=10_000)
        .map(|index| crate::scalar::FiniteReal::from_index(index).expect("test index is exact"))
        .collect::<Vec<_>>();
    let seed = crate::scalar::FiniteReal::new(5_000.5).expect("finite seed");
    let ctx = cadmpeg_test_support::service_decode_context();
    let intervals = super::super::bounded_nearest_intervals(&ctx, &boundaries, seed)
        .expect("resource allocation did not fail");

    assert_eq!(intervals.0.len(), 512);
    assert!(intervals
        .0
        .iter()
        .any(|interval| interval.map(crate::scalar::FiniteReal::get) == [5_000.0, 5_001.0]));
}

#[test]
fn bounded_nurbs_containment_search_keeps_the_final_valid_spans() {
    let boundaries = [0.0, 1.0, 1.0, 2.0, 3.0];

    let ctx = cadmpeg_test_support::service_decode_context();
    let search = super::super::bounded_tail_intervals(&ctx, &boundaries)
        .expect("resource allocation did not fail");
    assert_eq!(
        (search.intervals, search.truncated),
        (vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]], false)
    );

    let many_boundaries = (0..=10_000).map(f64::from).collect::<Vec<_>>();
    let search = super::super::bounded_tail_intervals(&ctx, &many_boundaries)
        .expect("resource allocation did not fail");
    assert_eq!(search.intervals.len(), 512);
    assert!(search.truncated);
}

#[test]
fn bounded_nurbs_boundary_witness_preserves_seed_priority() {
    let boundaries = [0, 1, 2]
        .map(|index| crate::scalar::FiniteReal::from_index(index).expect("test index is exact"));
    let seed = crate::scalar::FiniteReal::new(1.4).expect("finite seed");

    assert_eq!(
        super::super::nearest_boundary_witness(
            &cadmpeg_test_support::service_decode_context(),
            &boundaries,
            seed,
            0.0,
            |_| Ok(Some(0.0))
        )
        .expect("resource allocation did not fail"),
        super::super::BoundaryWitness::Found(crate::scalar::FiniteReal::ONE)
    );
}

#[test]
fn boundary_witness_admits_each_visit_before_evaluation() {
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::cell::Cell;
    let boundaries = [
        FiniteReal::ZERO,
        FiniteReal::ONE,
        FiniteReal::from_index(2).unwrap(),
    ];
    let seed = FiniteReal::new(1.4).unwrap();
    for allowance in 0..=3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let calls = Cell::new(0);
        let result = super::super::nearest_boundary_witness(&ctx, &boundaries, seed, 0.0, |_| {
            calls.set(calls.get() + 1);
            Ok(Some(0.0))
        });
        assert_eq!(calls.get(), allowance.min(2));
        if allowance < 3 {
            let original = result.unwrap_err();
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(
                original.operation,
                "IR curve inversion boundary witness scan"
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
            );
        } else {
            assert_eq!(
                result.unwrap(),
                super::super::BoundaryWitness::Found(FiniteReal::ONE)
            );
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn parameter_interval_scan_admits_only_visited_pairs() {
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let boundaries = [0, 1, 1, 2, 3].map(|value| FiniteReal::from_index(value).unwrap());
    for allowance in 0..=3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::parameter_interval_containing(
            &ctx,
            &boundaries,
            FiniteReal::new(1.5).unwrap(),
        );
        if allowance < 3 {
            let original = result.unwrap_err();
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(
                original.operation,
                "IR curve inversion Newton interval scan"
            );
            assert!(
                matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
            );
        } else {
            assert_eq!(result.unwrap().unwrap().endpoints(), [1.0, 2.0]);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn boundary_scans_stop_at_invalid_witness_and_preserve_fused_empty_refusals() {
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use std::cell::Cell;
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let calls = Cell::new(0);
    assert_eq!(
        super::super::nearest_boundary_witness(
            &ctx,
            &[FiniteReal::ZERO, FiniteReal::ONE],
            FiniteReal::HALF,
            0.0,
            |_| {
                calls.set(calls.get() + 1);
                Ok(None)
            }
        )
        .unwrap(),
        super::super::BoundaryWitness::Invalid
    );
    assert_eq!(calls.get(), 1);
    let original = ctx
        .charge_work_limit(1, "original empty boundary refusal")
        .unwrap_err();
    assert_eq!(
        super::super::nearest_boundary_witness(
            &ctx,
            &[],
            FiniteReal::ZERO,
            0.0,
            |_| -> Result<Option<f64>, _> { panic!("empty boundary has no evaluation") }
        )
        .unwrap_err(),
        original
    );
    assert_eq!(
        super::super::parameter_interval_containing(&ctx, &[], FiniteReal::ZERO).unwrap_err(),
        original
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn bounded_curve_search_preserves_caller_refusals_and_uses_scoped_storage() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .expect("fixture constructor admission")
    .unwrap();
    let boundaries = [FiniteReal::ZERO, FiniteReal::ONE];
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::super::nurbs_curve_parameter_near_point(
            &ctx,
            &curve,
            Point3::new(0.25, 0.0, 0.0),
            0.0,
            0.5,
        )
        .unwrap_err();
        assert!(matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
        assert_eq!(
            ctx.finish_session().unwrap_err().to_string(),
            error.to_string()
        );
        if dimension != ResourceDimension::RecursionDepth {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let error =
                super::super::bounded_nearest_intervals(&ctx, &boundaries, FiniteReal::HALF)
                    .unwrap_err();
            assert!(
                matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == dimension)
            );
            assert_eq!(
                ctx.finish_session().unwrap_err().to_string(),
                error.to_string()
            );
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        super::super::nurbs_curve_parameter_near_point(
            &ctx,
            &curve,
            Point3::new(0.25, 0.0, 0.0),
            0.0,
            0.5
        )
        .unwrap(),
        Some(FiniteReal::new(0.25).unwrap())
    );
    let intervals =
        super::super::bounded_nearest_intervals(&ctx, &boundaries, FiniteReal::HALF).unwrap();
    assert_eq!(intervals.0, vec![boundaries]);
    drop(intervals);
    ctx.finish_session().unwrap();
}

#[test]
fn tail_interval_search_preserves_refusals_and_releases_its_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let boundaries = [0., 1., 1., 2., 3.];
    for cap in 0..8 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let limit = super::super::bounded_tail_intervals(&ctx, &boundaries)
            .err()
            .expect("every scan, copy and swap requires work");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if dimension == ResourceDimension::MaterializedBytes {
            policy.limits.max_materialized_bytes = 0;
        } else {
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let limit = super::super::bounded_tail_intervals(&ctx, &boundaries)
            .err()
            .expect("scratch requires admission");
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let search = super::super::bounded_tail_intervals(&ctx, &boundaries).expect("exact visits");
    assert_eq!(search.intervals, vec![[0., 1.], [1., 2.], [2., 3.]]);
    assert!(!search.truncated);
    drop(search);
    let reuse = ctx
        .reserve_scoped(4096, "tail interval scratch reuse")
        .expect("scratch released");
    drop(reuse);
    ctx.finish_session().expect("exact visits and scoped bytes");
}

#[test]
fn pcurve_containment_preserves_caller_refusals_and_releases_scratch() {
    use crate::math::Point2;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let knots = [0., 0., 1., 1.];
    let controls = [Point2::new(0., 0.), Point2::new(1., 0.)];
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let limit = super::super::nurbs_pcurve_contains_point(
            &ctx,
            1,
            &knots,
            &controls,
            None,
            Point2::new(0.5, 0.),
            0.,
        )
        .expect_err("original caller refusal");
        assert_eq!(limit.dimension, dimension);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(
        super::super::nurbs_pcurve_contains_point(
            &ctx,
            1,
            &knots,
            &controls,
            None,
            Point2::new(0.5, 0.),
            0.
        )
        .expect("within limits"),
        Some(true)
    );
    let reuse = ctx
        .reserve_scoped(4096, "pcurve containment scratch reuse")
        .expect("all scratch released");
    drop(reuse);
    ctx.finish_session().expect("no retained temporary storage");
}

#[test]
fn nearest_interval_heap_preserves_descending_order_and_releases_working_storage() {
    use crate::scalar::FiniteReal;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let boundaries =
        [0, 1, 1, 2, 3, 4].map(|value| FiniteReal::from_index(value).expect("exact test index"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    // Four valid heap slots and four output slots. Repeated intervals add none.
    policy.limits.max_collection_items = 8;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let output = super::super::bounded_nearest_intervals(
        &ctx,
        &boundaries,
        FiniteReal::from_index(1).unwrap(),
    )
    .expect("eight slots");
    assert_eq!(
        output
            .0
            .iter()
            .map(|interval| interval.map(FiniteReal::get))
            .collect::<Vec<_>>(),
        [[3.0, 4.0], [2.0, 3.0], [1.0, 2.0], [0.0, 1.0]]
    );
    let bytes = u64::try_from(output.0.capacity() * std::mem::size_of::<[FiniteReal; 2]>())
        .expect("test bytes fit");
    let spare = ctx
        .reserve_scoped_limit(4096 - bytes, "test nearest interval heap released")
        .expect("only interval output remains");
    drop(spare);
    drop(output);
    let reuse = ctx
        .reserve_scoped_limit(4096, "test nearest intervals released")
        .expect("all bytes reusable");
    drop(reuse);
    ctx.finish_session()
        .expect("scoped intervals retain no bytes");
}
