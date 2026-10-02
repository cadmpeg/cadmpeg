// SPDX-License-Identifier: Apache-2.0

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
    assert!(intervals.0
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
        super::super::nearest_boundary_witness(&boundaries, seed, 0.0, |_| Ok(Some(0.0)))
            .expect("resource allocation did not fail"),
        super::super::BoundaryWitness::Found(crate::scalar::FiniteReal::ONE)
    );
}

#[test]
fn bounded_curve_search_preserves_caller_refusals_and_uses_scoped_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;
    use crate::scalar::FiniteReal;
    let curve = NurbsCurve::from_lanes(&cadmpeg_test_support::service_decode_context(), 1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], None, false).expect("fixture constructor admission").unwrap();
    let boundaries = [FiniteReal::ZERO, FiniteReal::ONE];
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits, ResourceDimension::RecursionDepth] {
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
        let error = super::super::nurbs_curve_parameter_near_point(&ctx, &curve, Point3::new(0.25, 0.0, 0.0), 0.0, 0.5).unwrap_err();
        assert!(matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
        assert_eq!(ctx.finish_session().unwrap_err().to_string(), error.to_string());
        if dimension != ResourceDimension::RecursionDepth {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let error = super::super::bounded_nearest_intervals(&ctx, &boundaries, FiniteReal::HALF).unwrap_err();
            assert!(matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
            assert_eq!(ctx.finish_session().unwrap_err().to_string(), error.to_string());
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(super::super::nurbs_curve_parameter_near_point(&ctx, &curve, Point3::new(0.25, 0.0, 0.0), 0.0, 0.5).unwrap(), Some(FiniteReal::new(0.25).unwrap()));
    let intervals = super::super::bounded_nearest_intervals(&ctx, &boundaries, FiniteReal::HALF).unwrap();
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
        let limit = super::super::bounded_tail_intervals(&ctx, &boundaries).err().expect("every scan, copy and swap requires work");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if dimension == ResourceDimension::MaterializedBytes { policy.limits.max_materialized_bytes = 0; }
        else { policy.limits.max_collection_items = 0; }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let limit = super::super::bounded_tail_intervals(&ctx, &boundaries).err().expect("scratch requires admission");
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
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
    let reuse = ctx.reserve_scoped(4096, "tail interval scratch reuse").expect("scratch released");
    drop(reuse);
    ctx.finish_session().expect("exact visits and scoped bytes");
}

#[test]
fn pcurve_containment_preserves_caller_refusals_and_releases_scratch() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use crate::math::Point2;
    let knots = [0., 0., 1., 1.];
    let controls = [Point2::new(0., 0.), Point2::new(1., 0.)];
    for dimension in [ResourceDimension::WorkUnits, ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let limit = super::super::nurbs_pcurve_contains_point(&ctx, 1, &knots, &controls, None, Point2::new(0.5, 0.), 0.)
            .expect_err("original caller refusal");
        assert_eq!(limit.dimension, dimension);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(super::super::nurbs_pcurve_contains_point(&ctx, 1, &knots, &controls, None, Point2::new(0.5, 0.), 0.)
        .expect("within limits"), Some(true));
    let reuse = ctx.reserve_scoped(4096, "pcurve containment scratch reuse").expect("all scratch released");
    drop(reuse);
    ctx.finish_session().expect("no retained temporary storage");
}
