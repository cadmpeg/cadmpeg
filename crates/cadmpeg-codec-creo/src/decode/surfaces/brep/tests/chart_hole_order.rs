// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};

use super::super::{ordered_chart_polygon_hole_loops, NativeCurveEvidence, NativePcurveCandidates};

fn half_edge(curve_id: u32) -> crate::topology::HalfEdgeId {
    crate::topology::HalfEdgeId {
        curve_id,
        side: crate::topology::Side::Zero,
    }
}

fn line_curve(id: u32, start: [f64; 2], end: [f64; 2]) -> Curve {
    let length = (end[0] - start[0]).hypot(end[1] - start[1]);
    Curve {
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("id"),
        parameter_range: None,
        source_object: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(start[0], start[1], 0.0),
                Vector3::new(
                    (end[0] - start[0]) / length,
                    (end[1] - start[1]) / length,
                    0.0,
                ),
            )
            .expect("line"),
        )),
    }
}

fn half_circle_curve(id: u32, center: [f64; 2], radius: f64, reference: f64) -> Curve {
    Curve {
        id: CurveId::mint(format!("creo:visibgeom:curve#{id}")).expect("id"),
        parameter_range: Some(
            cadmpeg_ir::topology::IncreasingParameterInterval::new([0.0, std::f64::consts::PI])
                .expect("half turn"),
        ),
        source_object: None,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                Point3::new(center[0], center[1], 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(reference, 0.0, 0.0),
                radius,
            )
            .expect("circle"),
        )),
    }
}

/// Orders an L-shaped cap with one circular hole per case and returns the
/// first loop's first curve.
fn first_loop_curve(holes: &[([f64; 2], f64)]) -> Option<u32> {
    let face = std::num::NonZeroU32::new(5);
    let outer_points = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 4.0],
        [4.0, 4.0],
        [4.0, 10.0],
        [0.0, 10.0],
    ];
    let outer_ids: Vec<u32> = (1..=6).collect();
    let mut loops = vec![crate::test_support::closed_loop(
        face,
        outer_ids.iter().copied().map(half_edge).collect(),
    )];
    let mut curves = Vec::new();
    let mut bindings = Vec::new();
    let mut points = BTreeMap::new();
    for (index, start) in outer_points.iter().enumerate() {
        let id = outer_ids[index];
        let next = (index + 1) % outer_points.len();
        curves.push(line_curve(id, *start, outer_points[next]));
        points.insert(id, [start[0], start[1], 0.0]);
        bindings.push((id, id, outer_ids[next]));
    }
    let mut nonlinear = BTreeSet::new();
    for (hole, (center, radius)) in holes.iter().enumerate() {
        let first = 10 + 2 * u32::try_from(hole).expect("hole index");
        let [start, end] = [first, first + 1];
        loops.push(crate::test_support::closed_loop(
            face,
            vec![half_edge(first), half_edge(first + 1)],
        ));
        curves.push(half_circle_curve(first, *center, *radius, 1.0));
        curves.push(half_circle_curve(first + 1, *center, *radius, -1.0));
        points.insert(start, [center[0] + radius, center[1], 0.0]);
        points.insert(end, [center[0] - radius, center[1], 0.0]);
        bindings.push((first, start, end));
        bindings.push((first + 1, end, start));
        nonlinear.extend([first, first + 1]);
    }
    let bindings: Vec<_> = bindings
        .into_iter()
        .map(
            |(curve_id, start, end)| crate::topology::HalfEdgeVertexIncidence {
                half_edge: half_edge(curve_id),
                start_vertex_id: std::num::NonZeroU32::new(start).expect("vertex"),
                end_vertex_id: std::num::NonZeroU32::new(end),
            },
        )
        .collect();
    let incidence = bindings
        .iter()
        .map(|binding| (binding.half_edge, binding))
        .collect();
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    ));
    let loop_refs: Vec<_> = loops.iter().rev().collect();
    crate::decode::with_service_context(|ctx| {
        ordered_chart_polygon_hole_loops(
            ctx,
            &loop_refs,
            (5, &surface),
            &incidence,
            &points,
            &NativePcurveCandidates::new(),
            NativeCurveEvidence {
                model_curves: &curves,
                typed_nonlinear_curve_ids: &nonlinear,
                source_carriers: &crate::decode::source_carriers::SourceUnitCarriers::default(),
            },
        )
    })
    .expect("bounded chart evaluation")
    .map(|ordered| ordered[0].half_edges()[0].curve_id)
}

#[test]
fn chart_polygon_orders_a_nonconvex_cap_around_circular_holes() {
    assert_eq!(first_loop_curve(&[([2.0, 7.0], 1.0)]), Some(1));
    assert_eq!(
        first_loop_curve(&[([2.0, 7.0], 1.0), ([7.0, 2.0], 1.0)]),
        Some(1)
    );
    assert_eq!(first_loop_curve(&[([7.0, 7.0], 1.0)]), None);
    assert_eq!(first_loop_curve(&[([3.5, 7.0], 1.0)]), None);
    assert_eq!(
        first_loop_curve(&[([2.0, 7.0], 1.0), ([2.0, 8.5], 1.0)]),
        None
    );
    assert_eq!(first_loop_curve(&[]), None);
}
