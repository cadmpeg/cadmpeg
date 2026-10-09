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
