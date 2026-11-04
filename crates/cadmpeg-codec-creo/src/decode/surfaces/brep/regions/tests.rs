// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use cadmpeg_ir::geometry::analytic::{CircleCurve, LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::IncreasingParameterInterval;

use crate::decode::analytic::pcurves::NativePcurveCandidates;
use crate::topology::HalfEdgeId;

use super::super::NativeCurveEvidence;
use super::{arcs_cross, outer_region, partition, segment_arc_crosses, CircularArc};

#[test]
fn disjoint_planar_regions_keep_each_hole_with_its_outer_ring() {
    for overlap in [false, true] {
        let mut curves = Vec::new();
        let mut vertices = BTreeMap::new();
        let mut bindings = Vec::new();
        let mut loops = Vec::new();
        let mut next_curve = 1u32;
        let mut next_vertex = 1u32;
        for origin in [0.0, if overlap { 3.0 } else { 10.0 }] {
            let points = [
                [origin, 0.0, 0.0],
                [origin + 4.0, 0.0, 0.0],
                [origin + 4.0, 4.0, 0.0],
                [origin, 4.0, 0.0],
            ];
            let mut half_edges = Vec::new();
            let first_vertex = next_vertex;
            for (index, point) in points.iter().enumerate() {
                let end = points[(index + 1) % points.len()];
                let edge = HalfEdgeId {
                    curve_id: next_curve,
                    side: crate::topology::Side::Zero,
                };
                half_edges.push(edge);
                vertices.insert(next_vertex, *point);
                bindings.push(crate::topology::HalfEdgeVertexIncidence {
                    half_edge: edge,
                    start_vertex_id: NonZeroU32::new(next_vertex).expect("vertex"),
                    end_vertex_id: NonZeroU32::new(
                        first_vertex + u32::try_from((index + 1) % 4).expect("index"),
                    ),
                });
                let direction = Point3::from(end)
                    .vector_from(Point3::from(*point))
                    .unit()
                    .expect("direction");
                curves.push(Curve {
                    id: CurveId::compose(&crate::identity::VISIBGEOM_CURVE, next_curve),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                        LineCurve::try_new(Point3::from(*point), direction).expect("line"),
                    )),
                    parameter_range: None,
                    source_object: None,
                });
                next_curve += 1;
                next_vertex += 1;
            }
            loops.push(crate::test_support::closed_loop(
                NonZeroU32::new(1),
                half_edges,
            ));
            let center = origin + 2.0;
            let start_vertex = next_vertex;
            vertices.insert(start_vertex, [center + 0.5, 2.0, 0.0]);
            vertices.insert(start_vertex + 1, [center - 0.5, 2.0, 0.0]);
            let mut hole_edges = Vec::new();
            for half in 0..2u32 {
                let edge = HalfEdgeId {
                    curve_id: next_curve,
                    side: crate::topology::Side::Zero,
                };
                hole_edges.push(edge);
                bindings.push(crate::topology::HalfEdgeVertexIncidence {
                    half_edge: edge,
                    start_vertex_id: NonZeroU32::new(start_vertex + half).expect("vertex"),
                    end_vertex_id: NonZeroU32::new(start_vertex + (1 - half)),
                });
                let start = f64::from(half) * std::f64::consts::PI;
                curves.push(Curve {
                    id: CurveId::compose(&crate::identity::VISIBGEOM_CURVE, next_curve),
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
                        CircleCurve::try_new(
                            Point3::new(center, 2.0, 0.0),
                            Vector3::new(0.0, 0.0, 1.0),
                            Vector3::new(1.0, 0.0, 0.0),
                            0.5,
                        )
                        .expect("circle"),
                    )),
                    parameter_range: IncreasingParameterInterval::new([
                        start,
                        start + std::f64::consts::PI,
                    ]),
                    source_object: None,
                });
                next_curve += 1;
            }
            next_vertex += 2;
            loops.push(crate::test_support::closed_loop(
                NonZeroU32::new(1),
                hole_edges,
            ));
        }
        let map = bindings
            .iter()
            .map(|binding| (binding.half_edge, binding))
            .collect();
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        ));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::default();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("context");
        let nonlinear = BTreeSet::from([5, 6, 11, 12]);
        let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
        let groups = partition(
            &ctx,
            &loops.iter().collect::<Vec<_>>(),
            (1, &surface),
            &map,
            &vertices,
            &NativePcurveCandidates::new(),
            NativeCurveEvidence {
                typed_nonlinear_curve_ids: &nonlinear,
                model_curves: &curves,
                source_carriers: &carriers,
            },
        )
        .expect("partition operation");
        if overlap {
            assert!(groups.is_none());
        } else {
            let groups = groups.expect("two disjoint regions");
            assert_eq!(groups.len(), 2);
            assert_eq!(
                groups[0]
                    .iter()
                    .map(|lp| lp.half_edges()[0].curve_id)
                    .collect::<Vec<_>>(),
                [1, 5]
            );
            assert_eq!(
                groups[1]
                    .iter()
                    .map(|lp| lp.half_edges()[0].curve_id)
                    .collect::<Vec<_>>(),
                [7, 11]
            );
        }
    }
}

#[test]
fn only_the_bounded_arc_must_avoid_other_boundary_segments() {
    let arc = CircularArc {
        circle: ([0.0, 0.0], 1.0),
        endpoints: [[1.0, 0.0], [-1.0, 0.0]],
        midpoint: [0.0, 1.0],
        span: std::f64::consts::PI,
    };
    assert!(segment_arc_crosses([0.0, 0.0], [0.0, 2.0], &arc));
    assert!(!segment_arc_crosses([0.0, -2.0], [0.0, -0.5], &arc));
    assert!(!segment_arc_crosses([1.0, 0.0], [2.0, 0.0], &arc));
    let crossing = CircularArc {
        circle: ([0.0, 1.0], 1.0),
        endpoints: [[1.0, 1.0], [-1.0, 1.0]],
        midpoint: [0.0, 0.0],
        span: std::f64::consts::PI,
    };
    assert!(arcs_cross(&arc, &crossing));
    let separate = CircularArc {
        circle: ([5.0, 0.0], 1.0),
        endpoints: [[6.0, 0.0], [4.0, 0.0]],
        midpoint: [5.0, 1.0],
        span: std::f64::consts::PI,
    };
    assert!(!arcs_cross(&arc, &separate));
}

#[test]
fn bounded_arc_must_bulge_outside_and_follow_its_source_interval() {
    let points = [[-1.0, 0.0], [1.0, 0.0], [1.0, -2.0], [-1.0, -2.0]];
    let lp = crate::test_support::closed_loop(
        NonZeroU32::new(1),
        (1..=4)
            .map(|curve_id| HalfEdgeId {
                curve_id,
                side: crate::topology::Side::Zero,
            })
            .collect(),
    );
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane"),
    ));
    let mut curves = Vec::new();
    curves.push(Curve {
        id: CurveId::compose(&crate::identity::VISIBGEOM_CURVE, 1),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Circle(
            CircleCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                1.0,
            )
            .expect("circle"),
        )),
        parameter_range: IncreasingParameterInterval::new([0.0, std::f64::consts::PI]),
        source_object: None,
    });
    for index in 1..4 {
        let a = Point3::new(points[index][0], points[index][1], 0.0);
        let next = points[(index + 1) % points.len()];
        let b = Point3::new(next[0], next[1], 0.0);
        let direction = b.vector_from(a).unit().expect("direction");
        curves.push(Curve {
            id: CurveId::compose(&crate::identity::VISIBGEOM_CURVE, index + 1),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                LineCurve::try_new(a, direction).expect("line"),
            )),
            parameter_range: None,
            source_object: None,
        });
    }
    let world: Vec<_> = (0..points.len())
        .map(|index| {
            [points[index], points[(index + 1) % points.len()]]
                .map(|point| Point3::new(point[0], point[1], 0.0))
        })
        .collect();
    let nonlinear = BTreeSet::from([1]);
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    for valid in [true, false] {
        if !valid {
            curves[0].parameter_range =
                IncreasingParameterInterval::new([0.0, std::f64::consts::TAU]);
        }
        let evidence = NativeCurveEvidence {
            typed_nonlinear_curve_ids: &nonlinear,
            model_curves: &curves,
            source_carriers: &carriers,
        };
        assert_eq!(
            outer_region(&lp, &plane, points.to_vec(), &world, evidence).is_some(),
            valid
        );
    }
}
