// SPDX-License-Identifier: Apache-2.0
//! The writer's span for an edge that stores no parameter range.

use super::super::edge_span;
use crate::test_support::test_closed_cylinder::closed_edge;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use std::f64::consts::TAU;

const MISSING_RANGE: &str = "IGES semantic writer requires a parameter range for edge";

/// A document with one vertex per position, keyed `v0`, `v1`, ...
fn vertices_at(positions: &[Point3]) -> CadIr {
    let mut ir = CadIr::empty();
    for (index, position) in positions.iter().enumerate() {
        let point = PointId::mint(format!("test:model:point#v{index}")).expect("identity grammar");
        ir.model.points.push(Point::new(
            point.clone(),
            FinitePoint3::new(*position).expect("finite position"),
            None,
        ));
        ir.model.vertices.push(Vertex {
            id: VertexId::mint(format!("test:model:vertex#v{index}")).expect("identity grammar"),
            point,
            tolerance: None,
        });
    }
    ir
}

fn circle() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            10.0,
        )
        .expect("circle"),
    ))
}

/// The unit square `(0,0) (1,0) (1,1) (0,1)` as a degree-1 curve that
/// returns to its first pole at the end of its knot domain `[0, 4]`.
fn square_loop(periodic: bool) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
            ],
            None,
            periodic,
        )
        .expect("degree-1 loop"),
    ))
}

fn refusal(ir: &CadIr, edge: &Edge, geometry: &CurveGeometry) -> String {
    match edge_span(ir, edge, geometry) {
        Err(CodecError::NotImplemented(message)) => message,
        other => panic!("expected a not-implemented refusal, got {other:?}"),
    }
}

#[test]
fn closed_circle_edge_without_range_spans_one_revolution() {
    let ir = vertices_at(&[Point3::new(10.0, 0.0, 0.0)]);
    let edge = closed_edge("circle", "circle", "v0");
    let span = edge_span(&ir, &edge, &circle()).expect("a closed circle edge has a span");
    assert_eq!(span.range.get(), [0.0, TAU]);
    assert_eq!(span.start, Point3::new(10.0, 0.0, 0.0));
    assert_eq!(span.end, span.start);
}

#[test]
fn closed_ellipse_edge_without_range_spans_one_revolution() {
    let ir = vertices_at(&[Point3::new(0.0, 0.0, 6.0)]);
    let edge = closed_edge("ellipse", "ellipse", "v0");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            6.0,
            2.0,
        )
        .expect("ellipse"),
    ));
    let span = edge_span(&ir, &edge, &geometry).expect("a closed ellipse edge has a span");
    assert_eq!(span.range.get(), [0.0, TAU]);
}

#[test]
fn closed_periodic_nurbs_edge_without_range_spans_its_knot_domain() {
    let ir = vertices_at(&[Point3::new(0.0, 0.0, 0.0)]);
    let edge = closed_edge("loop", "loop", "v0");
    let span = edge_span(&ir, &edge, &square_loop(true)).expect("a periodic loop has a span");
    assert_eq!(span.range.get(), [0.0, 4.0]);
}

#[test]
fn closed_edge_on_a_non_periodic_nurbs_without_range_is_refused() {
    let ir = vertices_at(&[Point3::new(0.0, 0.0, 0.0)]);
    let edge = closed_edge("loop", "loop", "v0");
    assert!(refusal(&ir, &edge, &square_loop(false)).starts_with(MISSING_RANGE));
}

#[test]
fn closed_edge_on_a_line_without_range_is_refused() {
    let ir = vertices_at(&[Point3::new(0.0, 0.0, 0.0)]);
    let edge = closed_edge("line", "line", "v0");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).expect("line"),
    ));
    assert!(refusal(&ir, &edge, &geometry).starts_with(MISSING_RANGE));
}

#[test]
fn open_circle_edge_without_range_is_refused() {
    let ir = vertices_at(&[Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 10.0, 0.0)]);
    let mut edge = closed_edge("arc", "circle", "v0");
    edge.end = VertexId::mint("test:model:vertex#v1").expect("identity grammar");
    assert!(refusal(&ir, &edge, &circle()).starts_with(MISSING_RANGE));
}

#[test]
fn closed_circle_edge_off_the_period_origin_is_refused() {
    let ir = vertices_at(&[Point3::new(0.0, 10.0, 0.0)]);
    let edge = closed_edge("circle", "circle", "v0");
    assert!(refusal(&ir, &edge, &circle()).starts_with(MISSING_RANGE));
}
