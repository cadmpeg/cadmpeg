// SPDX-License-Identifier: Apache-2.0
//! Intersection probes release storage before the next curve pair.

use super::super::incident_analytic_vertex_domain;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn repeated_line_conic_pair_reuses_intersection_storage() {
    let circle = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        ).expect("unit circle"),
    ));
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).expect("diameter line"),
    ));
    let single_pair = [&line, &circle];
    let repeated_pair = [&line, &circle, &line];
    let minimum_limit = |curves: &[&CurveGeometry]| {
        crate::test_support::allocation_limit_at(
            ResourceDimension::MaterializedBytes,
            None,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root");
                let points = incident_analytic_vertex_domain(&ctx, curves)?;
                assert_eq!(points, [[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
                Ok(points)
            },
        )
    };
    // Both routes keep two unique results and at most four candidate slots.
    // The second pair must reuse the first pair's released probe storage.
    assert_eq!(minimum_limit(&single_pair), minimum_limit(&repeated_pair));
}

#[test]
fn fixed_conic_parameter_conversion_needs_no_work() {
    let circle = |center, normal| CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(Point3::from(center), Vector3::from(normal), Vector3::new(1.0, 0.0, 0.0), 1.0).expect("unit circle"),
    ));
    for (second, x, y) in [
        (circle([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), 0.5, 0.75_f64.sqrt()),
        (circle([0.0; 3], [0.0, 1.0, 0.0]), 1.0, 0.0),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let points = super::super::conic_conic_intersections(&ctx, &circle([0.0; 3], [0.0, 0.0, 1.0]), &second).expect("bounded conversion and filtering");
        assert_eq!(points.len(), 2);
        const EPS_INTERSECTION_POINT: f64 = 1.0e-10;
        for point in points {
            assert!((point[0].abs() - x).abs() <= EPS_INTERSECTION_POINT);
            assert!((point[1].abs() - y).abs() <= EPS_INTERSECTION_POINT);
            assert_eq!(point[2], 0.0);
        }
    }
}
