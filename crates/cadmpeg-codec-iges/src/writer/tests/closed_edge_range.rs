// SPDX-License-Identifier: Apache-2.0
//! The writer's span for an edge that stores no parameter range.

use super::super::edge_span;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{
    CompositeCurveSegment, CompositeCurveTransition, Curve, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_ir::ids::{CurveId, PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Edge, Point, Vertex};
use cadmpeg_ir::CadIr;
use cadmpeg_test_support::solids::closed_edge;
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
            native_ref: None,
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
    square_path(periodic, Point3::new(0.0, 0.0, 0.0))
}

/// The square path from `(0,0)` through `(1,0) (1,1) (0,1)` to `last` over
/// the knot domain `[0, 4]`.
fn square_path(periodic: bool, last: Point3) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                last,
            ],
            None,
            periodic,
        )
        .expect("degree-1 path"),
    ))
}

fn refusal(ir: &CadIr, edge: &Edge, geometry: &CurveGeometry) -> String {
    match edge_span(ir, edge, geometry) {
        Err(CodecError::UnsupportedContent(message)) => message,
        _ => panic!("expected an unsupported-content refusal"),
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
fn closed_clamped_nurbs_edge_without_range_spans_its_knot_domain() {
    let ir = vertices_at(&[Point3::new(0.0, 0.0, 0.0)]);
    let edge = closed_edge("loop", "loop", "v0");
    let span = edge_span(&ir, &edge, &square_loop(false)).expect("a clamped loop has a span");
    assert_eq!(span.range.get(), [0.0, 4.0]);
}

#[test]
fn closed_edge_on_an_open_clamped_nurbs_without_range_is_refused() {
    let ir = vertices_at(&[Point3::new(0.0, 0.0, 0.0)]);
    let edge = closed_edge("path", "path", "v0");
    let geometry = square_path(false, Point3::new(0.0, 0.5, 0.0));
    assert!(refusal(&ir, &edge, &geometry).starts_with(MISSING_RANGE));
}

#[test]
fn closed_nurbs_edge_off_the_domain_start_is_refused() {
    let ir = vertices_at(&[Point3::new(1.0, 1.0, 0.0)]);
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
fn nested_single_line_composites_use_the_owning_edge_endpoints() {
    let mut ir = vertices_at(&[Point3::new(2.0, 0.0, 0.0), Point3::new(5.0, 0.0, 0.0)]);
    let id = |name| CurveId::mint(format!("test:model:curve#{name}")).expect("curve id");
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).expect("line"),
    ));
    let composite = |child| {
        CurveGeometry::Solved(SolvedCurveGeometry::Composite {
            segments: vec![CompositeCurveSegment {
                curve: id(child),
                same_sense: false,
                transition: CompositeCurveTransition::Continuous,
            }]
            .try_into()
            .unwrap(),
            self_intersect: Some(false),
        })
    };
    for (name, geometry) in [
        ("line", line),
        ("inner", composite("line")),
        ("outer", composite("inner")),
    ] {
        ir.model.curves.push(Curve {
            id: id(name),
            geometry,
            native_ref: None,
            source_object: None,
            parameter_range: None,
        });
    }
    let mut edge = closed_edge("edge", "outer", "v0");
    edge.end = VertexId::mint("test:model:vertex#v1").expect("vertex id");
    ir.model.edges.push(edge);
    let geometry = &ir
        .model
        .curves
        .iter()
        .find(|curve| curve.id == id("outer"))
        .unwrap()
        .geometry;
    let span = edge_span(&ir, &ir.model.edges[0], geometry).expect("edge bounds the child line");
    assert_eq!(span.range.get(), [0.0, 3.0]);
    assert_eq!(span.start, Point3::new(2.0, 0.0, 0.0));
    assert_eq!(span.end, Point3::new(5.0, 0.0, 0.0));
    let output =
        super::super::synthesize(&ir, crate::IgesVersion::V5_3).expect("curve synthesizes");
    assert!(output
        .losses
        .iter()
        .any(|loss| loss.message.contains(id("inner").as_str())));
    assert!(output
        .losses
        .iter()
        .any(|loss| loss.message.contains(id("outer").as_str())
            && loss.message.contains("nested composite")));
    assert!(output
        .losses
        .iter()
        .any(|loss| loss.message.contains("test:model:edge#edge")
            && loss.message.contains("flattened composite")));
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
