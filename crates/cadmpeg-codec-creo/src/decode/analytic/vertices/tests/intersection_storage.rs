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
