// SPDX-License-Identifier: Apache-2.0
//! Rejected parameter orderings release candidate reference lists.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use super::super::{ordered_native_parameter_face_loops, NativeCurveEvidence};

#[test]
fn rejected_parameter_ordering_retains_no_loop_references() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).expect("plane fixture"),
    ));
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (10..13).map(|curve_id| crate::topology::HalfEdgeId {
            curve_id, side: crate::topology::Side::Zero,
        }).collect(),
    );
    let polygon = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
    let bindings: Vec<_> = lp.half_edges().iter().copied().enumerate().map(|(index, half_edge)| {
        crate::topology::HalfEdgeVertexIncidence {
            half_edge,
            start_vertex_id: std::num::NonZeroU32::new(u32::try_from(index + 1).expect("three vertices"))
                .expect("one-based vertex"),
            end_vertex_id: std::num::NonZeroU32::new(u32::try_from((index + 1) % 3 + 1)
                .expect("three vertices")),
        }
    }).collect();
    let incidence: BTreeMap<_, _> = bindings.iter().map(|binding| (binding.half_edge, binding)).collect();
    let solved_vertices: BTreeMap<_, _> = polygon.iter().enumerate().map(|(index, point)| {
        (u32::try_from(index + 1).expect("three vertices"), [point[0], point[1], 0.0])
    }).collect();
    let pcurves = lp.half_edges().iter().enumerate().map(|(index, half_edge)| {
        ((half_edge.curve_id, 5), vec![([polygon[index], polygon[(index + 1) % 3]], 0)])
    }).collect();
    let typed = BTreeSet::new();
    let source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // An empty root has the core's 16 MiB materialized base allowance.
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for _ in 0..2 {
        // Coincident polygons have no strict outer loop. Three-edge loops
        // cannot use the two-edge circle fallback.
        assert!(ordered_native_parameter_face_loops(
            &ctx, &[&lp, &lp], (5, &surface), &incidence, &solved_vertices, &pcurves,
            NativeCurveEvidence {
                typed_nonlinear_curve_ids: &typed, model_curves: &[],
                source_carriers: &source_carriers,
            },
        ).expect("rejected ordering uses scratch only").is_none());
    }
    ctx.reserve_scoped(ctx.policy().limits.max_materialized_bytes, "released parameter ordering scratch")
        .expect("candidate lists and polygons are released");
}
