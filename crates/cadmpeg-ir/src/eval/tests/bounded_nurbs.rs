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

    assert_eq!(
        super::super::bounded_tail_intervals(&boundaries)
            .expect("resource allocation did not fail"),
        (vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]], false)
    );

    let many_boundaries = (0..=10_000).map(f64::from).collect::<Vec<_>>();
    let (intervals, truncated) = super::super::bounded_tail_intervals(&many_boundaries)
        .expect("resource allocation did not fail");
    assert_eq!(intervals.len(), 512);
    assert!(truncated);
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
    let curve = NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], None, false).unwrap();
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
