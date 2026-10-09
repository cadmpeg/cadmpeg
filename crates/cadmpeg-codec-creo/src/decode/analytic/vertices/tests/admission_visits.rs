// SPDX-License-Identifier: Apache-2.0
use super::super::{
    conic_conic_intersections, line_conic_intersections,
    pcurve_candidate_agrees_with_fixed_points, pcurve_endpoint_is_ambiguous,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

fn visit_policy(work: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    policy
}

#[test]
fn vertex_fixed_absence_routes_preserve_original_refusal_without_work() {
    let circle = |z| CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(Point3::new(0.0, 0.0, z), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0), 1.0).expect("unit circle"),
    ));
    let line = |z| CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, z), Vector3::new(1.0, 0.0, 0.0))
            .expect("unit line"),
    ));
    let first = circle(0.0);
    let separated = circle(1.0);
    let same_plane_line = line(0.0);
    let skew_line = line(1.0);
    let fixed = BTreeMap::new();
    let arena = DecodeArena::new();
    let policy = visit_policy(0);
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let check = |refused| {
        let results = [
            line_conic_intersections(&ctx, &first, &same_plane_line).map(|points| points.is_empty()),
            line_conic_intersections(&ctx, &same_plane_line, &same_plane_line).map(|points| points.is_empty()),
            line_conic_intersections(&ctx, &skew_line, &first).map(|points| points.is_empty()),
            conic_conic_intersections(&ctx, &same_plane_line, &first).map(|points| points.is_empty()),
            conic_conic_intersections(&ctx, &first, &same_plane_line).map(|points| points.is_empty()),
            conic_conic_intersections(&ctx, &first, &separated).map(|points| points.is_empty()),
            // Unsupported direction bytes exercise this helper's fixed precondition.
            pcurve_candidate_agrees_with_fixed_points(&ctx, [1, 2], [[0.0; 3]; 2], [0, 0], &fixed),
            pcurve_endpoint_is_ambiguous(&ctx, &[]).map(|ambiguous| !ambiguous),
        ];
        for result in results {
            if refused {
                let original = ctx.resource_refusal().expect("seeded refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert!(result.expect("fixed absence route"));
            }
        }
    };
    check(false);
    assert_eq!(ctx.resource_refusal(), None);
    let original = ctx.charge_work_limit(1, "after fixed vertex absence").expect_err("zero cap");
    assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, 0, 1));
    check(true);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn vertex_ambiguity_visits_only_present_comparisons_until_first_disagreement() {
    let same = [1.0, 2.0, 3.0];
    let different = [2.0, 2.0, 3.0];
    let mut first = vec![same, different];
    first.extend(std::iter::repeat_n(same, 128));
    let mut second = vec![same, same, different];
    second.extend(std::iter::repeat_n(same, 128));
    for (points, visits, expected) in [
        (Vec::new(), 0, false), (vec![same], 0, false),
        (vec![same, same], 1, false), (vec![same, same, same], 2, false),
        (first, 1, true), (second, 2, true),
    ] {
        for cap in 0..=visits {
            let arena = DecodeArena::new();
            let policy = visit_policy(cap);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = pcurve_endpoint_is_ambiguous(&ctx, &points);
            let original = if cap == visits {
                assert_eq!(result.expect("present comparisons"), expected);
                let original = ctx.charge_work_limit(1, "after vertex ambiguity").expect_err("exact visits");
                assert_eq!((original.dimension, original.used, original.additional), (ResourceDimension::WorkUnits, visits, 1));
                original
            } else {
                let original = ctx.resource_refusal().expect("comparison refusal");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((original.dimension, original.limit, original.used, original.additional, original.operation),
                    (ResourceDimension::WorkUnits, cap, cap, 1, "creo pcurve endpoint ambiguity search"));
                original
            };
            assert!(matches!(pcurve_endpoint_is_ambiguous(&ctx, &points), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(ctx.resource_refusal(), Some(original));
        }
    }
}
