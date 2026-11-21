// SPDX-License-Identifier: Apache-2.0
//! Edges the source states with no parameter range.
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::surface_point_solved;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{DegenerateCurve, LineCurve};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs};
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CoedgeId, CurveId, EdgeId, LoopId, PcurveId, PointId, VertexId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::check::Check;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::{
    Coedge, Edge, EdgeCarrier, EdgeEnds, FaceLoops, Loop, LoopBoundary, LoopRing, PcurveUse, Point,
    Sense, Vertex,
};

use super::{charges, write};
use crate::loss::F3dLossCode;
use crate::test_support::assert_written_with_loss;

const EPS_PARAMETER: f64 = 1.0e-9;

/// The unit cube with edge 0, `(0,0,0)` to `(10,0,0)`, stated without a
/// range, and its end vertex moved to `end`.
fn cube_with_unranged_edge(end: Point3) -> CadIr {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture");
    let edge = &mut ir.geometry.edges[0];
    edge.carrier = EdgeCarrier::new(edge.curve().cloned(), None).expect("a curve without a range");
    let point = &mut ir.geometry.points[1];
    *point = Point::new(
        point.id.clone(),
        FinitePoint3::new(end).expect("finite"),
        None,
    );
    ir
}

/// The geometric-consistency Errors of `ir` about its edge `index` or a
/// coedge of it.
fn edge_errors(ir: &CadIr, index: usize) -> Vec<String> {
    let edge = &ir.geometry.edges[index].id;
    let mut named = ir
        .geometry
        .coedges
        .iter()
        .filter(|coedge| coedge.edge == *edge)
        .map(|coedge| coedge.id.to_string())
        .collect::<Vec<_>>();
    named.push(edge.to_string());
    cadmpeg_ir::validate::validate_neutral(ir, Vec::new())
        .expect("validate the neutral model")
        .findings
        .into_iter()
        .filter(|finding| {
            finding.check == Check::GeometricConsistency
                && finding.severity == Severity::Error
                && finding
                    .entity
                    .as_ref()
                    .is_some_and(|entity| named.contains(entity))
        })
        .map(|finding| finding.message)
        .collect()
}

fn geometry_omitted() -> String {
    F3dLossCode::WriterGeometryOmitted.kind().to_string()
}

fn widened() -> String {
    F3dLossCode::VertexToleranceWidened.kind().to_string()
}

/// Whether `report` charges `writer.geometry-omitted` for `edge` running over
/// one of two arcs of its curve.
fn arc_assumed_for(report: &cadmpeg_ir::report::export::ExportReport, edge: &str) -> bool {
    report.losses.iter().any(|loss| {
        loss.code.to_string() == geometry_omitted()
            && loss.message.contains(&format!("edge {edge}:"))
            && loss.message.contains("on two arcs")
    })
}

fn record_invalid_names(report: &cadmpeg_ir::report::export::ExportReport, edge: &str) -> bool {
    report
        .losses
        .iter()
        .any(|loss| loss.code.to_string() == "export/record.invalid" && loss.message.contains(edge))
}

#[test]
fn an_open_edge_within_the_coincidence_allowance_takes_its_exact_range() {
    let ir = cube_with_unranged_edge(Point3::new(10.0, 0.005, 0.0));
    let edge = ir.geometry.edges[0].id.to_string();
    let (report, decoded) = write(&ir);
    for code in [geometry_omitted(), widened()] {
        assert!(!charges(&report, &code), "{code}: {:#?}", report.losses);
    }
    assert!(
        !record_invalid_names(&report, &edge),
        "{:#?}",
        report.losses
    );
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(start.abs() < EPS_PARAMETER && (end - 10.0).abs() < EPS_PARAMETER);
    assert_eq!(decoded.geometry.vertices[1].tolerance, None);
    assert_eq!(edge_errors(&decoded, 0), Vec::<String>::new());
}

#[test]
fn an_open_edge_just_beyond_the_allowance_keeps_the_feet_and_both_charges() {
    let ir = cube_with_unranged_edge(Point3::new(10.0, 0.02, 0.0));
    let edge = ir.geometry.edges[0].id.to_string();
    let (report, decoded) = write(&ir);
    assert!(
        charges(&report, &geometry_omitted()),
        "{:#?}",
        report.losses
    );
    assert!(record_invalid_names(&report, &edge), "{:#?}", report.losses);
    assert!(!charges(&report, &widened()), "{:#?}", report.losses);
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(start.abs() < EPS_PARAMETER && (end - 10.0).abs() < EPS_PARAMETER);
    // The charge is true: the output's own validation reports the edge.
    assert_ne!(edge_errors(&decoded, 0), Vec::<String>::new());
}

#[test]
fn a_vertex_beyond_its_own_allowance_is_raised_with_a_loss_naming_it() {
    let mut ir = cube_with_unranged_edge(Point3::new(10.0, 0.03, 0.0));
    let ends = ir.geometry.edges[0].ends.clone().expect("edge ends");
    let edge = ir.geometry.edges[0].id.to_string();
    ir.geometry
        .vertices
        .iter_mut()
        .find(|vertex| vertex.id == ends.start)
        .expect("start vertex")
        .tolerance = Some(PositiveReal::new(0.05).expect("positive"));
    let (report, decoded) = write(&ir);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == widened()
                && loss.message.contains(&format!("vertex {}:", ends.end))
                && loss.message.contains("0.030000 mm")),
        "{:#?}",
        report.losses
    );
    assert!(
        !charges(&report, &geometry_omitted()),
        "{:#?}",
        report.losses
    );
    assert!(
        !record_invalid_names(&report, &edge),
        "{:#?}",
        report.losses
    );
    let tolerance = decoded.geometry.vertices[1]
        .tolerance
        .expect("the raised vertex is written tolerant")
        .get();
    assert!((tolerance - 0.03).abs() < 1.0e-9, "{tolerance}");
    assert_eq!(edge_errors(&decoded, 0), Vec::<String>::new());
}

#[test]
fn an_open_edge_on_a_curve_shorter_than_the_allowance_spans_its_domain() {
    // The whole curve lies within the allowance of either vertex, so either
    // vertex inverts to any parameter: the range comes from the domain's ends.
    let mut ir = cube_with_unranged_edge(Point3::new(0.005, 0.0, 0.0));
    let edge = ir.geometry.edges[0].id.to_string();
    let curve = ir.geometry.edges[0].curve().cloned().expect("edge curve");
    ir.geometry
        .curves
        .iter_mut()
        .find(|row| row.id == curve)
        .expect("edge curve row")
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 2.0, 2.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.005, 0.0, 0.0)],
            None,
            false,
        )
        .expect("a short straight NURBS"),
    ));
    let (report, decoded) = write(&ir);
    assert!(
        !record_invalid_names(&report, &edge),
        "{:#?}",
        report.losses
    );
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(start.abs() < EPS_PARAMETER && (end - 2.0).abs() < EPS_PARAMETER);
}

#[test]
fn an_open_edge_within_its_own_tolerance_takes_its_range() {
    let mut ir = cube_with_unranged_edge(Point3::new(10.0, 0.01, 0.0));
    let curve = ir.geometry.edges[0].curve().cloned().expect("edge curve");
    ir.geometry
        .curves
        .iter_mut()
        .find(|row| row.id == curve)
        .expect("edge curve row")
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 2.0, 2.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("a straight NURBS"),
    ));
    ir.geometry.edges[0].tolerance = Some(PositiveReal::new(0.025).expect("positive"));
    let (report, decoded) = write(&ir);
    assert!(
        !charges(&report, &geometry_omitted()),
        "{:#?}",
        report.losses
    );
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(start.abs() < EPS_PARAMETER && (end - 2.0).abs() < EPS_PARAMETER);
}

/// The straight pcurve from `ends[0]` to `ends[1]` over `domain` on the plane
/// of the face that owns `coedge`.
fn plane_pcurve(
    ir: &CadIr,
    coedge: &Coedge,
    ends: [Point3; 2],
    domain: [f64; 2],
) -> PcurveGeometry {
    let face = &ir
        .geometry
        .loops
        .iter()
        .find(|row| row.id == coedge.owner_loop)
        .expect("coedge loop")
        .face;
    let surface = &ir
        .geometry
        .faces
        .iter()
        .find(|row| row.id == *face)
        .expect("loop face")
        .surface;
    let plane = ir
        .geometry
        .surfaces
        .iter()
        .find(|row| row.id == *surface)
        .and_then(|row| row.geometry.solved())
        .expect("face plane");
    let at = |u, v| {
        surface_point_solved(EvaluationAdmission::Standard, plane, u, v)
            .expect("plane point")
            .get()
    };
    let origin = at(0.0, 0.0);
    let axes = [at(1.0, 0.0), at(0.0, 1.0)]
        .map(|end| Vector3::new(end.x - origin.x, end.y - origin.y, end.z - origin.z));
    let chart = |point: Point3| {
        let offset = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        Point2::new(
            offset.dot(axes[0]) / axes[0].dot(axes[0]),
            offset.dot(axes[1]) / axes[1].dot(axes[1]),
        )
    };
    PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![domain[0], domain[0], domain[1], domain[1]],
            ends.map(chart).to_vec(),
            None,
            false,
        )
        .expect("straight pcurve"),
    }
}

/// Each coedge of edge 0 takes the straight pcurve from `(10,0,0)` at
/// `domain[0]` to `(0,0,0)` at `domain[1]` on its face's plane.
fn put_plane_pcurves(ir: &mut CadIr, domain: [f64; 2]) {
    let edge = ir.geometry.edges[0].id.clone();
    for ordinal in 0..ir.geometry.coedges.len() {
        if ir.geometry.coedges[ordinal].edge != edge {
            continue;
        }
        let geometry = plane_pcurve(
            ir,
            &ir.geometry.coedges[ordinal],
            [Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)],
            domain,
        );
        let id = PcurveId::mint(format!("generated:test:against-pcurve#{ordinal}")).unwrap();
        ir.geometry.pcurves.push(Pcurve {
            id: id.clone(),
            geometry,
            metadata: PcurveMetadata::default(),
            native_ref: None,
        });
        ir.geometry.coedges[ordinal].pcurves = vec![PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: None,
        }];
    }
}

#[test]
fn an_open_line_edge_against_its_line_is_written_with_its_ends_exchanged() {
    let mut ir = cube_with_unranged_edge(Point3::new(10.0, 0.0, 0.0));
    let against =
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(-1.0, 0.0, 0.0)).unwrap();
    let curve = ir.geometry.edges[0].curve().cloned().expect("edge curve");
    ir.geometry
        .curves
        .iter_mut()
        .find(|row| row.id == curve)
        .expect("edge curve row")
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(against));
    put_plane_pcurves(&mut ir, [-10.0, 0.0]);
    let (report, decoded) = write(&ir);
    assert!(
        !charges(&report, &geometry_omitted()),
        "{:#?}",
        report.losses
    );
    assert!(
        !charges(
            &report,
            &F3dLossCode::WriterPcurveOmitted.kind().to_string()
        ),
        "{:#?}",
        report.losses
    );
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written")
        .get();
    assert!((start + 10.0).abs() < EPS_PARAMETER && end.abs() < EPS_PARAMETER);
    assert!(start_position(&decoded, 0).distance(Point3::new(10.0, 0.0, 0.0)) < EPS_PARAMETER);
}

/// The position of the start vertex of `ir`'s edge `index`.
fn start_position(ir: &CadIr, index: usize) -> Point3 {
    let first = &ir.geometry.edges[index]
        .ends
        .as_ref()
        .expect("edge ends")
        .start;
    let point = &ir
        .geometry
        .vertices
        .iter()
        .find(|vertex| vertex.id == *first)
        .expect("start vertex")
        .point;
    ir.geometry
        .points
        .iter()
        .find(|row| row.id == *point)
        .expect("start point")
        .position()
        .get()
}

/// The unit cube with edge 0, `(0,0,0)` to `(10,0,0)`, stated without a range
/// on a degree-1 NURBS through `poles` at the parameters `knots` interpolates.
fn cube_with_unranged_nurbs_edge(knots: Vec<f64>, poles: Vec<Point3>, periodic: bool) -> CadIr {
    let mut ir = cube_with_unranged_edge(Point3::new(10.0, 0.0, 0.0));
    let curve = ir.geometry.edges[0].curve().cloned().expect("edge curve");
    ir.geometry
        .curves
        .iter_mut()
        .find(|row| row.id == curve)
        .expect("edge curve row")
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(1, knots, poles, None, periodic).expect("a polyline NURBS"),
    ));
    ir
}

#[test]
fn an_open_edge_against_its_nurbs_curve_is_written_with_its_ends_exchanged() {
    // The curve runs from (10,0,0) at 0 to (0,0,0) at 2: the edge runs from 2
    // to 0. Its coedges' pcurves are stated on the curve's parameter.
    let mut ir = cube_with_unranged_nurbs_edge(
        vec![0.0, 0.0, 2.0, 2.0],
        vec![Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)],
        false,
    );
    put_plane_pcurves(&mut ir, [0.0, 2.0]);
    let edge = ir.geometry.edges[0].id.clone();
    let (report, decoded) = write(&ir);
    for code in [
        geometry_omitted(),
        F3dLossCode::WriterPcurveOmitted.kind().to_string(),
    ] {
        assert!(!charges(&report, &code), "{code}: {:#?}", report.losses);
    }
    assert!(
        !record_invalid_names(&report, edge.as_str()),
        "{:#?}",
        report.losses
    );
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(start.abs() < EPS_PARAMETER && (end - 2.0).abs() < EPS_PARAMETER);
    assert!(start_position(&decoded, 0).distance(Point3::new(10.0, 0.0, 0.0)) < EPS_PARAMETER);
    assert_eq!(edge_errors(&decoded, 0), Vec::<String>::new());
}

#[test]
fn edges_with_and_against_one_nurbs_curve_both_take_ranges_on_it() {
    // Edge 0 runs from 10 to 0 on the curve and the edge from (0,0,0) to
    // (0,0,10) from 10 to 20: the curve is shared, so only the edge against it
    // changes. The arena is in id order, so that edge is found by its id.
    let mut ir = cube_with_unranged_nurbs_edge(
        vec![0.0, 0.0, 10.0, 20.0, 20.0],
        vec![
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 10.0),
        ],
        false,
    );
    let with = ir
        .geometry
        .edges
        .iter()
        .position(|edge| edge.id.as_str() == "synthetic:cube:edge#8")
        .expect("the cube's edge from corner 0 to corner 4");
    let curve = ir.geometry.edges[0].curve().cloned();
    ir.geometry.edges[with].carrier =
        EdgeCarrier::new(curve, None).expect("a curve without a range");
    let edges = [0, with].map(|index| ir.geometry.edges[index].id.to_string());
    let (report, decoded) = write(&ir);
    for edge in &edges {
        assert!(!record_invalid_names(&report, edge), "{:#?}", report.losses);
    }
    assert_eq!(
        decoded.geometry.edges[0].curve(),
        decoded.geometry.edges[with].curve()
    );
    for (index, expected, start) in [
        (0, [0.0, 10.0], Point3::new(10.0, 0.0, 0.0)),
        (with, [10.0, 20.0], Point3::new(0.0, 0.0, 0.0)),
    ] {
        let range = decoded.geometry.edges[index]
            .param_range()
            .expect("the edge is written with a range")
            .get();
        assert!(
            (range[0] - expected[0]).abs() < EPS_PARAMETER
                && (range[1] - expected[1]).abs() < EPS_PARAMETER,
            "edge {index}: {range:?}"
        );
        assert!(start_position(&decoded, index).distance(start) < EPS_PARAMETER);
        assert_eq!(edge_errors(&decoded, index), Vec::<String>::new());
    }
}

#[test]
fn an_open_edge_against_a_periodic_nurbs_curve_keeps_its_stand_in_and_is_named() {
    // On a periodic curve an end before the start may cross the seam, so the
    // writer does not exchange the edge's ends.
    let ir = cube_with_unranged_nurbs_edge(
        vec![0.0, 0.0, 2.0, 2.0],
        vec![Point3::new(10.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)],
        true,
    );
    let edge = ir.geometry.edges[0].id.to_string();
    let (report, decoded) = write(&ir);
    assert!(record_invalid_names(&report, &edge), "{:#?}", report.losses);
    assert!(start_position(&decoded, 0).distance(Point3::new(0.0, 0.0, 0.0)) < EPS_PARAMETER);
}

#[test]
fn an_open_edge_ending_where_its_nurbs_curve_crosses_itself_takes_the_inner_arc_and_is_named() {
    // The curve passes (10,0,0) at 0.5 and again at 2.5, and ends at (0,0,0)
    // at 3. The edge runs against the curve from 3 to 2.5 or to 0.5; the arc
    // [0.5, 3] passes the edge's end vertex again at 2.5, so the edge takes
    // [2.5, 3] with its ends exchanged.
    let ir = cube_with_unranged_nurbs_edge(
        vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0],
        vec![
            Point3::new(10.0, -10.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(20.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        false,
    );
    let edge = ir.geometry.edges[0].id.to_string();
    let (report, decoded) = write(&ir);
    assert_inner_arc(&report, &decoded, &edge);
}

/// `edge`, edge 0, is written over `[2.5, 3]` from `(10,0,0)`, with the loss
/// naming the arc assumption.
fn assert_inner_arc(
    report: &cadmpeg_ir::report::export::ExportReport,
    decoded: &CadIr,
    edge: &str,
) {
    assert!(!record_invalid_names(report, edge), "{:#?}", report.losses);
    assert!(arc_assumed_for(report, edge), "{:#?}", report.losses);
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(
        (start - 2.5).abs() < EPS_PARAMETER && (end - 3.0).abs() < EPS_PARAMETER,
        "[{start}, {end}]"
    );
    assert!(start_position(decoded, 0).distance(Point3::new(10.0, 0.0, 0.0)) < EPS_PARAMETER);
    assert_eq!(edge_errors(decoded, 0), Vec::<String>::new());
}

#[test]
fn an_open_edge_starting_where_its_nurbs_curve_crosses_itself_takes_the_inner_arc_and_is_named() {
    // Edge 0 is reversed to run from (10,0,0) to (0,0,0) on the curve of the
    // test above. Its start inverts to 0.5 and to 2.5; the arc [0.5, 3] passes
    // the start vertex again at 2.5, so the edge takes [2.5, 3] as it runs.
    let mut ir = cube_with_unranged_nurbs_edge(
        vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0],
        vec![
            Point3::new(10.0, -10.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(20.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        false,
    );
    let edge = ir.geometry.edges[0].id.clone();
    let ends = ir.geometry.edges[0].ends.take().expect("edge ends");
    ir.geometry.edges[0].ends = Some(EdgeEnds::new(ends.end, ends.start));
    for coedge in ir
        .geometry
        .coedges
        .iter_mut()
        .filter(|row| row.edge == edge)
    {
        coedge.sense = match coedge.sense {
            Sense::Forward => Sense::Reversed,
            Sense::Reversed => Sense::Forward,
        };
    }
    let (report, decoded) = write(&ir);
    assert_inner_arc(&report, &decoded, edge.as_str());
}

#[test]
fn an_open_edge_far_along_a_long_nurbs_curve_takes_its_range() {
    // A degree-1 curve of 600 spans zigzags 1 mm either side of the x axis
    // and crosses it at each half parameter, through (0,0,0) at 0.5 and
    // (10,0,0) at 599.5. Each vertex lies more spans from the far end of the
    // curve than the inversion searches from that seed: the second inversion
    // finds nothing, and the edge still takes its range.
    const SPANS: u32 = 600;
    let step = 10.0 / f64::from(SPANS - 1);
    let mut knots = vec![0.0];
    knots.extend((0..=SPANS).map(f64::from));
    knots.push(f64::from(SPANS));
    let poles = (0..=SPANS)
        .map(|index| {
            let side = if index % 2 == 0 { 1.0 } else { -1.0 };
            Point3::new((f64::from(index) - 0.5) * step, side, 0.0)
        })
        .collect();
    let ir = cube_with_unranged_nurbs_edge(knots, poles, false);
    let edge = ir.geometry.edges[0].id.to_string();
    let (report, decoded) = write(&ir);
    assert!(
        !record_invalid_names(&report, &edge),
        "{:#?}",
        report.losses
    );
    assert!(!arc_assumed_for(&report, &edge), "{:#?}", report.losses);
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    let last = f64::from(SPANS) - 0.5;
    assert!(
        (start - 0.5).abs() < EPS_PARAMETER && (end - last).abs() < EPS_PARAMETER,
        "[{start}, {end}]"
    );
    assert!(start_position(&decoded, 0).distance(Point3::new(0.0, 0.0, 0.0)) < EPS_PARAMETER);
}

#[test]
fn an_open_edge_from_where_its_closed_nurbs_curve_closes_takes_its_range() {
    // The curve starts and ends at (0,0,0) and passes (10,0,0) at 1. The
    // edge's start inverts to 0 from the curve's start and to 3 from its end:
    // the two ends of a closed curve's domain are one place, so the edge runs
    // over [0, 1] as a sibling edge from (10,0,0) back to the closure would
    // run over [1, 3].
    let ir = cube_with_unranged_nurbs_edge(
        vec![0.0, 0.0, 1.0, 2.0, 3.0, 3.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        false,
    );
    let edge = ir.geometry.edges[0].id.to_string();
    let (report, decoded) = write(&ir);
    assert!(
        !record_invalid_names(&report, &edge),
        "{:#?}",
        report.losses
    );
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(
        start.abs() < EPS_PARAMETER && (end - 1.0).abs() < EPS_PARAMETER,
        "[{start}, {end}]"
    );
    assert!(start_position(&decoded, 0).distance(Point3::new(0.0, 0.0, 0.0)) < EPS_PARAMETER);
}

#[test]
fn an_open_edge_off_its_curve_is_ranged_between_the_feet_with_a_loss() {
    let ir = cube_with_unranged_edge(Point3::new(10.0, 5.0, 0.0));
    let edge = ir.geometry.edges[0].id.to_string();
    let report = assert_written_with_loss(&ir, None, &geometry_omitted(), &edge);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.message.contains(&edge) && loss.message.contains("5.000000 mm")),
        "{:#?}",
        report.losses
    );
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == "export/record.invalid"
                && loss.message.contains(&edge)),
        "{:#?}",
        report.losses
    );
    let (_, decoded) = write(&ir);
    let [start, end] = decoded.geometry.edges[0]
        .param_range()
        .expect("the edge is written with a range")
        .get();
    assert!(start.abs() < EPS_PARAMETER && (end - 10.0).abs() < EPS_PARAMETER);
}

/// A box with a one-edge loop on its top face at `(5, 5, 10)`, whose closed
/// edge has a point carrier at `point` and no range.
fn box_with_point_edge(point: Point3) -> CadIr {
    let corners = [
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 10.0, 10.0],
        [0.0, 10.0, 10.0],
    ];
    let faces = cadmpeg_test_support::polyhedron::box_faces([0, 1, 2, 3, 4, 5, 6, 7]);
    let faces = faces.iter().map(<[usize; 4]>::as_slice).collect::<Vec<_>>();
    let mut ir = cadmpeg_test_support::polyhedron::polyhedron(&corners, &faces).unwrap();
    let vertex_point = PointId::mint("test:model:point#apex").unwrap();
    let vertex = VertexId::mint("test:model:vertex#apex").unwrap();
    let curve = CurveId::mint("test:model:curve#apex").unwrap();
    let edge = EdgeId::mint("test:model:edge#apex").unwrap();
    let coedge = CoedgeId::mint("test:model:coedge#apex").unwrap();
    let apex_loop = LoopId::mint("test:model:loop#apex").unwrap();
    let top = &mut ir.geometry.faces[1];
    let outer = top.loops.iter().next().unwrap().clone();
    top.loops = FaceLoops::unspecified(vec![outer, apex_loop.clone()]);
    let top = top.id.clone();
    ir.geometry.points.push(Point::new(
        vertex_point.clone(),
        FinitePoint3::new(Point3::new(5.0, 5.0, 10.0)).unwrap(),
        None,
    ));
    ir.geometry.vertices.push(Vertex {
        id: vertex.clone(),
        point: vertex_point,
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.curves.push(Curve {
        parameter_range: None,
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(DegenerateCurve::new(
            FinitePoint3::new(point).unwrap(),
        ))),
        source_object: None,
        native_ref: None,
    });
    ir.geometry.edges.push(Edge {
        id: edge.clone(),
        carrier: EdgeCarrier::new(Some(curve), None).unwrap(),
        ends: Some(EdgeEnds::new(vertex.clone(), vertex)),
        tolerance: None,
        native_ref: None,
    });
    ir.geometry.coedges.push(Coedge {
        id: coedge.clone(),
        owner_loop: apex_loop.clone(),
        edge,
        radial_next: coedge.clone(),
        sense: Sense::Forward,
        pcurves: Vec::new(),
        use_curve: None,
        native_ref: None,
    });
    ir.geometry.loops.push(Loop {
        id: apex_loop,
        face: top,
        boundary: LoopBoundary::Ring(LoopRing::new(vec![coedge], Vec::new()).unwrap()),
        native_ref: None,
    });
    ir.finalize();
    ir
}

#[test]
fn a_closed_edge_on_a_point_is_written_with_a_null_curve() {
    let ir = box_with_point_edge(Point3::new(5.0, 5.0, 10.0));
    let (report, decoded) = write(&ir);
    assert!(
        !charges(&report, &F3dLossCode::WriterBodyOmitted.kind().to_string()),
        "{:#?}",
        report.losses
    );
    assert_eq!(decoded.geometry.edges.len(), ir.geometry.edges.len());
    let collapsed = decoded
        .geometry
        .edges
        .iter()
        .find(|edge| {
            edge.ends
                .as_ref()
                .is_some_and(|ends| ends.start == ends.end)
        })
        .expect("the collapsed edge is written");
    assert!(
        collapsed
            .curve()
            .is_some_and(|curve| curve.as_str().contains("point-edge-curve")),
        "{collapsed:?}"
    );
}

#[test]
fn a_closed_edge_beside_its_point_keeps_its_carrier() {
    let ir = box_with_point_edge(Point3::new(5.0, 5.0, 11.0));
    assert_written_with_loss(
        &ir,
        None,
        &F3dLossCode::WriterBodyOmitted.kind().to_string(),
        "requires a parameter range",
    );
}

#[test]
fn a_vertex_raised_by_one_edge_prevents_false_record_invalid_on_a_sibling_edge() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture");
    // Edge 0 (E1): runs from V (vertex 1) to X (vertex 2), line on y = 0.2.
    // Edge 1 (E2): runs from W (vertex 0, tol 0.6) to V (vertex 1), line on y = 0.
    // V lies at (5, 0.5, 0) and X at (15, 0.5, 0): E2 raises V to 0.5 mm, which
    // puts both ends of E1 within E1's 0.5 mm allowance (0.3 mm off y = 0.2).
    let v_id = ir.geometry.vertices[1].id.clone();
    let w_id = ir.geometry.vertices[0].id.clone();
    let x_id = ir.geometry.vertices[2].id.clone();
    ir.geometry.vertices[0].tolerance = Some(PositiveReal::new(0.6).expect("positive"));
    ir.geometry.vertices[1].tolerance = None;
    ir.geometry.vertices[2].tolerance = None;
    for (index, coords) in [
        (0, Point3::new(0.0, 0.0, 0.0)),
        (1, Point3::new(5.0, 0.5, 0.0)),
        (2, Point3::new(15.0, 0.5, 0.0)),
    ] {
        let point = &mut ir.geometry.points[index];
        *point = Point::new(point.id.clone(), FinitePoint3::new(coords).unwrap(), None);
    }
    let e1_curve = ir.geometry.edges[0].curve().cloned().expect("e1 curve");
    let e2_curve = ir.geometry.edges[1].curve().cloned().expect("e2 curve");
    ir.geometry
        .curves
        .iter_mut()
        .find(|row| row.id == e1_curve)
        .expect("e1 curve row")
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
        FinitePoint3::new(Point3::new(0.0, 0.2, 0.0)).unwrap(),
        cadmpeg_ir::units::UnitVector3::try_from(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    )));
    ir.geometry
        .curves
        .iter_mut()
        .find(|row| row.id == e2_curve)
        .expect("e2 curve row")
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(LineCurve::new(
        FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        cadmpeg_ir::units::UnitVector3::try_from(Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    )));
    ir.geometry.edges[0].ends = Some(EdgeEnds::new(v_id.clone(), x_id));
    ir.geometry.edges[0].carrier = EdgeCarrier::new(Some(e1_curve), None).unwrap();
    ir.geometry.edges[0].tolerance = None;
    ir.geometry.edges[1].ends = Some(EdgeEnds::new(w_id, v_id));
    ir.geometry.edges[1].carrier = EdgeCarrier::new(Some(e2_curve), None).unwrap();
    ir.geometry.edges[1].tolerance = None;
    let e1_id = ir.geometry.edges[0].id.to_string();
    let e2_id = ir.geometry.edges[1].id.to_string();

    let (report, decoded) = write(&ir);
    assert!(
        !record_invalid_names(&report, &e1_id),
        "E1 should not charge record.invalid once E2 raises V: {:#?}",
        report.losses
    );
    assert!(
        !record_invalid_names(&report, &e2_id),
        "E2 should not charge record.invalid: {:#?}",
        report.losses
    );
    assert_eq!(edge_errors(&decoded, 0), Vec::<String>::new());
}
