// SPDX-License-Identifier: Apache-2.0
use super::super::{
    extrusion_cap_pcurve, extrusion_profile_signed_area, nurbs_profile_point,
    nurbs_profile_polyline, ordered_extrusion_profiles, polylines_intersect,
    profile_segments_intersect, sketch_geometry_endpoints, ProfileGeometry,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn work_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn fixed_profile_routes_are_free_and_preserve_original_refusal() {
    let line = super::profile_line([0.0, 0.0], [1.0, 0.0]);
    let crossing = super::profile_line([0.5, -1.0], [0.5, 1.0]);
    let geometry = cadmpeg_ir::sketches::SketchGeometry::try_from(
        cadmpeg_ir::sketches::SketchGeometryDefinition::Line {
            start: cadmpeg_ir::math::Point2::new(0.0, 0.0),
            end: cadmpeg_ir::math::Point2::new(1.0, 0.0),
        },
    ).expect("finite line");
    let expected_pcurve = super::super::line_pcurve([0.0, 0.0], [1.0, 0.0]);
    let linear = super::linear_profile_curve();
    let collapsed = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1, vec![0.0; 4],
        vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)],
        None, false,
    ).expect("structural constructor admission").expect("finite ordered knot and pole lanes");
    let arena = DecodeArena::new();
    let policy = work_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let mut evaluator = cadmpeg_ir::eval::decode::NurbsPointEvaluator::new(&ctx, &linear)
        .expect("fixed inline basis");
    let mut queries = || {
        let mut refusal = crate::lane_refusal::LaneRefusals::new();
        [
            sketch_geometry_endpoints(&ctx, &geometry).map(|value| value == Some([[0.0, 0.0], [1.0, 0.0]])),
            line.geometry().to_sketch(&ctx).map(|value| value.as_ref() == Some(&geometry)),
            extrusion_cap_pcurve(&ctx, &geometry, false, [0.0, 0.0], [1.0, 0.0], &"line", &mut refusal)
                .map(|value| value == expected_pcurve),
            extrusion_profile_signed_area(&ctx, &[]).map(|value| value.is_none()),
            ordered_extrusion_profiles(&ctx, Vec::new()).map(|value| value.is_none()),
            polylines_intersect(&ctx, &[], &[[0.0; 2]; 2], 0.0, [None, None]).map(|value| !value),
            polylines_intersect(&ctx, &[[0.0; 2]], &[[0.0; 2]; 2], 0.0, [None, None]).map(|value| !value),
            profile_segments_intersect(&ctx, &line, &crossing, 0.0, [None, None]),
            nurbs_profile_point(&ctx, &mut evaluator, &linear, f64::NAN).map(|value| value.is_none()),
            nurbs_profile_polyline(&ctx, &collapsed, 0.01).map(|value| value.is_none()),
        ]
    };
    for result in queries() { assert!(result.expect("fixed route")); }
    let original = ctx.charge_work_limit(1, "seed fixed profile refusal").expect_err("zero work");
    for result in queries() {
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(line.geometry(), ProfileGeometry::Line { .. }));
}

#[test]
fn profile_polyline_queries_admit_present_windows_and_stop_at_first_contact() {
    let source = "creo profile polyline intersection source scan";
    let pair = "creo profile polyline intersection pairs";
    let mut first_tail = vec![[-2.0, 0.0], [2.0, 0.0]];
    first_tail.extend(std::iter::repeat_n([3.0, 0.0], 128));
    let mut second_tail = vec![[1.0, -1.0], [1.0, 1.0]];
    second_tail.extend(std::iter::repeat_n([3.0, 1.0], 128));
    for (first, second, expected, charges) in [
        (Vec::new(), vec![[0.0; 2]; 2], false, Vec::new()),
        (vec![[0.0; 2]], vec![[0.0; 2]; 2], false, Vec::new()),
        (vec![[0.0, 0.0], [1.0, 0.0]], Vec::new(), false, vec![source]),
        (vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], vec![[0.0, 1.0], [2.0, 1.0]], false, vec![source, pair, source, pair]),
        (vec![[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]], vec![[0.0, 1.0], [1.0, 1.0], [2.0, 1.0]], false, vec![source, pair, pair, source, pair, pair]),
        (first_tail, second_tail, true, vec![source, pair]),
        (vec![[-2.0, 0.0], [2.0, 0.0]], vec![[1.0, 1.0], [2.0, 1.0], [2.0, -1.0]], true, vec![source, pair, pair]),
    ] {
        let visits = u64::try_from(charges.len()).expect("fixture visits");
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = polylines_intersect(&ctx, &first, &second, 0.0, [None, None]);
            if cap == visits {
                assert_eq!(result.expect("source visits admitted"), expected);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("window visit refusal"); };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!((original.used, original.additional), (cap, 1));
                assert_eq!(original.operation, charges[usize::try_from(cap).expect("fixture visit")]);
                assert!(matches!(polylines_intersect(&ctx, &first, &second, 0.0, [None, None]), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn profile_area_admits_only_entities_and_coordinate_rows() {
    for (profile, expected) in [
        (Vec::new(), None),
        (vec![super::profile_line([0.0, 0.0], [1.0, 0.0])], None),
        (vec![super::profile_line([0.0, 0.0], [1.0, 0.0]), super::profile_line([1.0, 0.0], [0.0, 1.0]), super::profile_line([0.0, 1.0], [0.0, 0.0])], Some(0.5)),
        (vec![super::profile_circle([0.0, 0.0], 1.0, false)], Some(std::f64::consts::PI)),
        (vec![super::profile_circle([0.0, 0.0], 1.0, true)], Some(-std::f64::consts::PI)),
    ] {
        let count = u64::try_from(profile.len()).expect("fixture count");
        let visits = count * 2;
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = extrusion_profile_signed_area(&ctx, &profile);
            if cap == visits {
                assert_eq!(result.expect("source area visits").map(cadmpeg_ir::scalar::FiniteReal::get), expected);
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("area work refusal"); };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                if cap < count {
                    assert_eq!((original.used, original.additional, original.operation), (cap, 1, "creo profile area entity scan"));
                } else {
                    assert_eq!((original.used, original.additional, original.operation), (count, count, "creo profile coordinate scale"));
                }
                assert!(matches!(extrusion_profile_signed_area(&ctx, &profile), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn empty_profile_query_stops_before_unrelated_profiles() {
    let mut profiles = vec![Vec::new()];
    profiles.extend(std::iter::repeat_n(vec![super::profile_circle([0.0, 0.0], 1.0, false)], 128));
    for (profiles, visits) in [(Vec::new(), 0), (profiles, 1)] {
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = work_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = ordered_extrusion_profiles(&ctx, profiles.clone());
            if cap == visits {
                assert!(result.expect("empty profile query").is_none());
                assert_eq!(ctx.resource_refusal(), None);
            } else {
                let Err(CodecError::ResourceLimit(original)) = result else { panic!("first profile visit refusal"); };
                assert_eq!((original.dimension, original.used, original.additional, original.operation), (ResourceDimension::WorkUnits, 0, 1, "creo profile empty scan"));
                assert!(matches!(ordered_extrusion_profiles(&ctx, profiles.clone()), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn single_circle_profile_uses_no_absent_pair_work() {
    let charges = [
        "creo profile empty scan", "creo profile coordinate scale", "creo profile coordinate scale",
        "creo profile intersection scan", "creo profile self intersection row scan",
        "creo profile pair source scan", "creo outer profile candidate scan",
        "creo outer profile containment pairs", "creo hole profile containment source scan",
        "creo profile validation scan", "creo profile validation entity scan",
        "creo profile area entity scan", "creo profile coordinate scale", "creo profile area sign scan",
    ];
    let total = u64::try_from(charges.len()).expect("source visits");
    for cap in 0..=total {
        let profile = vec![vec![super::profile_circle([0.0, 0.0], 1.0, false)]];
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = ordered_extrusion_profiles(&ctx, profile.clone());
        if cap == total {
            let result = result.expect("source visits admitted").expect("valid circle");
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].area(), std::f64::consts::PI);
            assert_eq!(result[0].entities(), &profile[0]);
            assert_eq!(ctx.resource_refusal(), None);
        } else {
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("circle visit refusal"); };
            assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, cap, 1));
            assert_eq!(original.operation, charges[usize::try_from(cap).expect("fixture visit")]);
            assert!(matches!(ordered_extrusion_profiles(&ctx, profile), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn singular_sampling_stops_before_quarter_evaluation() {
    use super::super::{append_nurbs_profile_span, NurbsProfileSpan};
    use cadmpeg_ir::eval::decode::NurbsPointEvaluator;
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 3],
        Some(vec![1.0, -1.0, 1.0]), false,
    ).expect("structural admission").expect("finite rational lanes");
    let span = NurbsProfileSpan {
        start: 0.0, end: 1.0, start_point: [0.0; 2], end_point: [0.0; 2],
        tolerance: 0.01, depth: 0,
    };
    crate::decode::with_test_decode_ctx(|ctx| {
        let mut evaluator = NurbsPointEvaluator::new(ctx, &curve).expect("basis");
        assert_eq!(nurbs_profile_point(ctx, &mut evaluator, &curve, 0.5).expect("midpoint"), None);
        for quarter in [0.25, 0.75] {
            assert_eq!(nurbs_profile_point(ctx, &mut evaluator, &curve, quarter).expect("quarter"), Some([0.0; 2]));
        }
    });
    // Basis storage fill: 3. Span: 1. Midpoint basis: 1 + (1 + 1) +
    // (2 + 1) = 6. Finite basis: 3. Homogeneous axes: 4 * 3 visits,
    // weight cancellation replay: 3, constant-coordinate pass: 3.
    const FIRST_ABSENT_SAMPLE_WORK: u64 = 3 + 1 + 6 + 3 + 4 * 3 + 3 + 3;
    for cap in 0..=FIRST_ABSENT_SAMPLE_WORK {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_collection_items = 3;
        policy.limits.max_materialized_bytes = 3 * cadmpeg_core::decode::u64_from_index(std::mem::size_of::<f64>());
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut storage = ctx.reserve_scoped(0, "test sampling points").expect("empty storage");
        let mut points = Vec::new();
        let mut evaluator = match NurbsPointEvaluator::new(&ctx, &curve) {
            Ok(evaluator) => evaluator,
            Err(original) => {
                assert!(cap < 3);
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, 0, 3, "IR B-spline basis work"));
                assert_eq!(ctx.resource_refusal(), Some(original));
                continue;
            }
        };
        let result = append_nurbs_profile_span(&ctx, &mut evaluator, &curve, &span, &mut points, &mut storage);
        assert!(points.is_empty());
        if cap == FIRST_ABSENT_SAMPLE_WORK {
            assert!(result.expect("only reached midpoint work").is_none());
            assert_eq!(ctx.resource_refusal(), None);
            let original = ctx.charge_work_limit(1, "after absent midpoint").expect_err("exact midpoint work");
            assert_eq!((original.used, original.additional), (FIRST_ABSENT_SAMPLE_WORK, 1));
            assert!(matches!(append_nurbs_profile_span(&ctx, &mut evaluator, &curve, &span, &mut points, &mut storage), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("midpoint refusal"); };
            let operation = match cap {
                3 => "creo NURBS profile sampling spans",
                4..=9 => "IR B-spline basis work",
                10..=12 => "IR B-spline finite basis inspection",
                _ => "IR homogeneous pole traversal",
            };
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::WorkUnits, cap, 1, operation));
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(append_nurbs_profile_span(&ctx, &mut evaluator, &curve, &span, &mut points, &mut storage), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn absent_first_endpoint_stops_before_last_endpoint_evaluation() {
    let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![cadmpeg_ir::math::Point2::new(0.0, 0.0); 4], None, false,
    ).expect("structural admission").expect("finite ordered knot and pole lanes");
    let geometry = cadmpeg_ir::sketches::SketchGeometry::nurbs(curve);
    crate::decode::with_test_decode_ctx(|ctx| {
        let lifted = super::super::super::nurbs::sketch_nurbs_curve(ctx, &geometry)
            .expect("lift admission").expect("positive curve with increasing domain");
        let carrier = cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(lifted),
        );
        assert!(cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, &carrier, 0.0),
        ).expect("lower evaluation").is_none());
        assert_eq!(cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, &carrier, 1.0),
        ).expect("upper evaluation").map(|point| point.get()),
            Some(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)));
    });
    // Lift: seven copied knots and four pole projections. Repeated knots
    // produce zero basis terms: three fill visits, six recurrence visits,
    // three finite checks, then five three-pole homogeneous passes. All
    // products are zero, so the exact accumulator does not replay them.
    const FIRST_ENDPOINT_WORK: u64 = 7 + 4 + 3 + 6 + 3 + 5 * 3;
    for cap in 0..=FIRST_ENDPOINT_WORK {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_collection_items = 7 + 4 + 3;
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
            7 * std::mem::size_of::<f64>() +
            4 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>() +
            4 * std::mem::size_of::<f64>(),
        );
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = sketch_geometry_endpoints(&ctx, &geometry);
        if cap == FIRST_ENDPOINT_WORK {
            assert!(result.expect("first endpoint only").is_none());
            assert_eq!(ctx.resource_refusal(), None);
            let original = ctx.charge_work_limit(1, "after absent first endpoint").expect_err("exact first endpoint work");
            assert_eq!((original.used, original.additional), (FIRST_ENDPOINT_WORK, 1));
            assert!(matches!(sketch_geometry_endpoints(&ctx, &geometry), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("endpoint work refusal"); };
            let expected = match cap {
                0..=6 => (0, 7, "creo sketch NURBS lift knots"),
                7..=10 => (7, 4, "creo NURBS point projection"),
                11..=19 => (cap, 1, "IR B-spline basis work"),
                20..=22 => (cap, 1, "IR B-spline finite basis inspection"),
                _ => (cap, 1, "IR homogeneous pole traversal"),
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.used, original.additional, original.operation), expected);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(sketch_geometry_endpoints(&ctx, &geometry), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn profile_sampling_reuses_the_admitted_span_start() {
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0); 3], None, false,
    ).expect("structural admission").expect("finite polynomial lanes");
    let basis = "IR B-spline basis work";
    let finite = "IR B-spline finite basis inspection";
    let poles = "IR homogeneous pole traversal";
    let knots = "creo NURBS profile knot scan";
    // Each point evaluates six basis steps, three finite-basis entries, four
    // axes of three poles and a three-pole constant-coordinate pass: 24.
    let point_visits = || std::iter::repeat_n((1_u64, basis), 6)
        .chain(std::iter::repeat_n((1, finite), 3))
        .chain(std::iter::repeat_n((1, poles), 4 * 3 + 3));
    let mut charges = vec![(3, basis)];
    charges.extend(point_visits()); // Initial lower point.
    charges.extend(std::iter::repeat_n((1, knots), 3));
    charges.extend(point_visits()); // Span end; start is already stored.
    charges.push((1, "creo NURBS profile sampling spans"));
    for _ in 0..3 { charges.extend(point_visits()); } // Midpoint and quarters.
    charges.extend(std::iter::repeat_n((1, knots), 2));
    const SAMPLING_WORK: u64 = 3 + 5 * 24 + 5 + 1;
    assert_eq!(charges.iter().map(|(amount, _)| amount).sum::<u64>(), SAMPLING_WORK);
    for cap in 0..=SAMPLING_WORK {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_collection_items = 3 + 2;
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
            3 * std::mem::size_of::<f64>() + 4 * std::mem::size_of::<[f64; 2]>(),
        );
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = nurbs_profile_polyline(&ctx, &curve, 0.01);
        if cap == SAMPLING_WORK {
            let result = result.expect("five required points").expect("sampled curve");
            assert_eq!(result.points, vec![[0.0; 2]; 2]);
            assert_eq!(ctx.resource_refusal(), None);
            drop(result);
            let original = ctx.charge_work_limit(1, "after sampled profile").expect_err("exact sampling work");
            assert_eq!((original.used, original.additional), (SAMPLING_WORK, 1));
            assert!(matches!(nurbs_profile_polyline(&ctx, &curve, 0.01), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("sampling work refusal"); };
            let mut used = 0;
            let expected = charges.iter().find_map(|&(amount, operation)| {
                if used + amount > cap { Some((used, amount, operation)) }
                else { used += amount; None }
            }).expect("first unadmitted source operation");
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.used, original.additional, original.operation), expected);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(nurbs_profile_polyline(&ctx, &curve, 0.01), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn absent_gauss_point_stops_before_tangent_evaluation() {
    use super::super::nurbs_profile_signed_area_twice;
    let width = f64::from_bits(1);
    let curve = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        2, vec![0.0, 0.0, 0.0, 0.0, width, width, width],
        vec![cadmpeg_ir::math::Point2::new(0.0, 0.0); 4], None, false,
    ).expect("structural admission").expect("finite ordered polynomial lanes");
    let geometry = cadmpeg_ir::sketches::SketchGeometry::nurbs(curve);
    assert_eq!(width * 0.5, 0.0);
    crate::decode::with_test_decode_ctx(|ctx| {
        let lifted = super::super::super::nurbs::sketch_nurbs_curve(ctx, &geometry)
            .expect("lift admission").expect("positive polynomial with increasing domain");
        let carrier = cadmpeg_ir::geometry::CurveGeometry::Solved(
            cadmpeg_ir::geometry::SolvedCurveGeometry::Nurbs(lifted),
        );
        assert!(cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, &carrier, 0.0),
        ).expect("first Gauss point").is_none());
        assert_eq!(cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, &carrier, width),
        ).expect("upper endpoint").map(|point| point.get()),
            Some(cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)));
    });
    // Lift: seven knots and four projections. Four knot windows precede
    // the first Gauss point. Its all-zero basis uses three fill, six
    // recurrence, three finite and fifteen homogeneous visits, with no replay.
    const FIRST_GAUSS_POINT_WORK: u64 = 7 + 4 + 4 + 3 + 6 + 3 + 5 * 3;
    for cap in 0..=FIRST_GAUSS_POINT_WORK {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        policy.limits.max_collection_items = 7 + 4 + 3;
        policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(
            7 * std::mem::size_of::<f64>() +
            4 * std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>() +
            4 * std::mem::size_of::<f64>(),
        );
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = nurbs_profile_signed_area_twice(&ctx, &geometry, false);
        if cap == FIRST_GAUSS_POINT_WORK {
            assert!(result.expect("first point only").is_none());
            assert_eq!(ctx.resource_refusal(), None);
            let original = ctx.charge_work_limit(1, "after absent Gauss point").expect_err("exact first point work");
            assert_eq!((original.used, original.additional), (FIRST_GAUSS_POINT_WORK, 1));
            assert!(matches!(nurbs_profile_signed_area_twice(&ctx, &geometry, false), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else {
            let Err(CodecError::ResourceLimit(original)) = result else { panic!("Gauss point work refusal"); };
            let expected = match cap {
                0..=6 => (0, 7, "creo sketch NURBS lift knots"),
                7..=10 => (7, 4, "creo NURBS point projection"),
                11..=14 => (cap, 1, "creo NURBS profile knot scan"),
                15..=23 => (cap, 1, "IR B-spline basis work"),
                24..=26 => (cap, 1, "IR B-spline finite basis inspection"),
                _ => (cap, 1, "IR homogeneous pole traversal"),
            };
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!((original.used, original.additional, original.operation), expected);
            assert_eq!(ctx.resource_refusal(), Some(original));
            assert!(matches!(nurbs_profile_signed_area_twice(&ctx, &geometry, false), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}
