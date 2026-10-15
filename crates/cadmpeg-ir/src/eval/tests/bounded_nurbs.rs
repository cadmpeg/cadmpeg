// SPDX-License-Identifier: Apache-2.0

#[test]
fn matching_polynomial_seed_returns_the_seed_parameter() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let preparation = cadmpeg_test_support::service_decode_context();
    let mut knots = vec![0.0; 4];
    knots.extend((1..1_021).map(|ordinal| f64::from(ordinal) / 1_021.0));
    knots.extend([1.0; 4]);
    let curve = NurbsCurve::from_lanes(
        3,
        knots,
        (0..1_024)
            .map(|ordinal| Point3::new(f64::from(ordinal), 0.0, 0.0))
            .collect(),
        None,
        false,
    )
    .expect("valid authored spline");
    let point = crate::eval::decode::nurbs_curve_point_at(&preparation, &curve, 0.5)
        .expect("seed evaluation")
        .get();
    let policy = DecodePolicy::service();
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
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![point, Point3::new(1.0, 0.0, 0.0)],
        Some(vec![-1.0, 1.0]),
        false,
    )
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
    let intervals = super::super::bounded_nearest_intervals(&boundaries, seed);

    assert_eq!(intervals.len(), 512);
    assert!(intervals
        .iter()
        .any(|interval| interval.map(crate::scalar::FiniteReal::get) == [5_000.0, 5_001.0]));
}

#[test]
fn bounded_nurbs_containment_search_keeps_the_final_valid_spans() {
    let boundaries = [0.0, 1.0, 1.0, 2.0, 3.0];

    let search = super::super::bounded_tail_intervals(&boundaries);
    assert_eq!(
        (search.intervals, search.truncated),
        (vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]], false)
    );

    let many_boundaries = (0..=10_000).map(f64::from).collect::<Vec<_>>();
    let search = super::super::bounded_tail_intervals(&many_boundaries);
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
            .expect("no evaluation refusal"),
        super::super::BoundaryWitness::Found(crate::scalar::FiniteReal::ONE)
    );
}

#[test]
fn parameter_interval_scan_finds_the_containing_pair() {
    use crate::scalar::FiniteReal;
    let boundaries = [0, 1, 1, 2, 3].map(|value| FiniteReal::from_index(value).unwrap());
    let interval =
        super::super::parameter_interval_containing(&boundaries, FiniteReal::new(1.5).unwrap());
    assert_eq!(interval.unwrap().endpoints(), [1.0, 2.0]);
    assert!(super::super::parameter_interval_containing(&[], FiniteReal::ZERO).is_none());
}

#[test]
fn boundary_scan_stops_at_an_invalid_witness() {
    use crate::scalar::FiniteReal;
    use std::cell::Cell;
    let calls = Cell::new(0);
    assert_eq!(
        super::super::nearest_boundary_witness(
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
}

#[test]
fn bounded_curve_search_finds_the_linear_parameter() {
    use crate::geometry::nurbs::NurbsCurve;
    use crate::math::Point3;
    use crate::scalar::FiniteReal;
    let ctx = cadmpeg_test_support::service_decode_context();
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
        None,
        false,
    )
    .unwrap();
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
    let boundaries = [FiniteReal::ZERO, FiniteReal::ONE];
    assert_eq!(
        super::super::bounded_nearest_intervals(&boundaries, FiniteReal::HALF),
        vec![boundaries]
    );
    ctx.finish_session().unwrap();
}

#[test]
fn pcurve_containment_finds_a_point_on_a_linear_pcurve() {
    use crate::math::Point2;
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::super::nurbs_pcurve_contains_point(
            1,
            &[0., 0., 1., 1.],
            &[Point2::new(0., 0.), Point2::new(1., 0.)],
            None,
            Point2::new(0.5, 0.),
            0.,
        )
        .expect("no session refusal"),
        Some(true)
    );
    ctx.finish_session().unwrap();
}

#[test]
fn nearest_interval_heap_preserves_descending_order() {
    use crate::scalar::FiniteReal;
    let boundaries =
        [0, 1, 1, 2, 3, 4].map(|value| FiniteReal::from_index(value).expect("exact test index"));
    let output =
        super::super::bounded_nearest_intervals(&boundaries, FiniteReal::from_index(1).unwrap());
    assert_eq!(
        output
            .iter()
            .map(|interval| interval.map(FiniteReal::get))
            .collect::<Vec<_>>(),
        [[3.0, 4.0], [2.0, 3.0], [1.0, 2.0], [0.0, 1.0]]
    );
}

#[test]
fn bounded_curve_search_returns_the_session_depth_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let curve = crate::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![
            crate::math::Point3::new(0.0, 0.0, 0.0),
            crate::math::Point3::new(1.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::nurbs_curve_parameter_near_point(
        &ctx,
        &curve,
        crate::math::Point3::new(0.25, 0.0, 0.0),
        0.0,
        0.5,
    )
    .unwrap_err();
    assert!(
        matches!(&error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RecursionDepth)
    );
    assert_eq!(
        ctx.finish_session().unwrap_err().to_string(),
        error.to_string()
    );
}
