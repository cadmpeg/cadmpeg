// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Sense;

use crate::test_support::parasolid::{
    circle_carrier, line_carrier, linear_nurbs_curve_carrier_through, triangle_body,
};

/// Decode the triangle with its first edge, from (0, 0, 0) to (1, 0, 0), on
/// the compact curve `carrier` with attribute 70, carrying `marker`.
fn decode_marked_edge(mut carrier: Vec<u8>, marker: u8) -> (String, String, Sense) {
    let mut body = triangle_body();
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
    carrier[18] = marker;
    body.extend(carrier);
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let brep =
        super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("curve-orientation")).unwrap();
    let edge = brep
        .edges
        .iter()
        .find(|edge| edge.curve().is_some())
        .unwrap();
    let coedge = brep
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge.id)
        .unwrap();
    (
        edge.start.as_str().to_owned(),
        edge.end.as_str().to_owned(),
        coedge.sense,
    )
}

fn decode_marked_line_edge(marker: u8) -> (String, String, Sense) {
    decode_marked_edge(line_carrier(70, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), marker)
}

/// A circle about +z whose short arc from (1, 0, 0) to (0, 0, 0) in the
/// direction of increasing parameter spans `SHORT_ARC_DEGREES`.
const SHORT_ARC_DEGREES: f64 = 1.5;

fn short_arc_radius() -> f64 {
    0.5 / (SHORT_ARC_DEGREES.to_radians() / 2.0).sin()
}

fn short_arc_center() -> [f64; 3] {
    let radius = short_arc_radius();
    [0.5, -(radius * radius - 0.25).sqrt(), 0.0]
}

/// The sweep in degrees from the neutral start vertex to the neutral end
/// vertex in the direction of increasing circle parameter.
fn circle_sweep_degrees(start: &str, end: &str) -> f64 {
    let center = short_arc_center();
    let vertex = |name: &str| match name {
        name if name.ends_with("#50") => [0.0, 0.0],
        name if name.ends_with("#51") => [1.0, 0.0],
        name => panic!("unexpected vertex {name}"),
    };
    let angle = |point: [f64; 2]| (point[1] - center[1]).atan2(point[0] - center[0]);
    (angle(vertex(end)) - angle(vertex(start)))
        .rem_euclid(std::f64::consts::TAU)
        .to_degrees()
}

fn decode_marked_circle_edge(marker: u8) -> (String, String, Sense) {
    decode_marked_edge(
        circle_carrier(70, short_arc_center(), [0.0, 0.0, 1.0], short_arc_radius()),
        marker,
    )
}

#[test]
fn against_marker_trims_a_circle_edge_through_its_short_arc() {
    let (start, end, sense) = decode_marked_circle_edge(0x2d);
    assert_eq!(sense, Sense::Reversed);
    let sweep = circle_sweep_degrees(&start, &end);
    assert!((sweep - SHORT_ARC_DEGREES).abs() < 1.0e-9, "{sweep}");
}

#[test]
fn along_marker_trims_a_circle_edge_through_its_long_arc() {
    let (start, end, sense) = decode_marked_circle_edge(0x2b);
    assert_eq!(sense, Sense::Forward);
    let sweep = circle_sweep_degrees(&start, &end);
    assert!(
        (sweep - (360.0 - SHORT_ARC_DEGREES)).abs() < 1.0e-9,
        "{sweep}"
    );
}

#[test]
fn along_marker_keeps_the_stored_edge_direction() {
    let (start, end, sense) = decode_marked_line_edge(0x2b);
    assert!(start.ends_with("#50"), "{start}");
    assert!(end.ends_with("#51"), "{end}");
    assert_eq!(sense, Sense::Forward);
}

#[test]
fn against_marker_reverses_the_stored_edge_direction() {
    let (start, end, sense) = decode_marked_line_edge(0x2d);
    assert!(start.ends_with("#51"), "{start}");
    assert!(end.ends_with("#50"), "{end}");
    assert_eq!(sense, Sense::Reversed);
}

/// Decode the triangle with its first edge, stored from (0, 0, 0) to
/// (1, 0, 0), on a degree-one B-spline from `first` to `last`.
fn decode_spline_edge(first: [f64; 3], last: [f64; 3]) -> (String, String, Sense) {
    let mut body = triangle_body();
    let edge = body.windows(2).position(|w| w == [0x00, 0x10]).unwrap();
    body[edge + 24..edge + 26].copy_from_slice(&70u16.to_be_bytes());
    body.extend(linear_nurbs_curve_carrier_through(70, 80, first, last));
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&body, &arena, &policy).unwrap();
    let brep =
        super::decode_body(&ctx, &body, &cadmpeg_ir::stream_name!("curve-orientation")).unwrap();
    let edge = brep
        .edges
        .iter()
        .find(|edge| edge.curve().is_some())
        .unwrap();
    assert!(edge.param_range().is_none());
    let coedge = brep
        .coedges
        .iter()
        .find(|coedge| coedge.edge == edge.id)
        .unwrap();
    (
        edge.start.as_str().to_owned(),
        edge.end.as_str().to_owned(),
        coedge.sense,
    )
}

#[test]
fn spline_edge_along_its_curve_keeps_the_stored_direction() {
    let (start, end, sense) = decode_spline_edge([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    assert!(start.ends_with("#50"), "{start}");
    assert!(end.ends_with("#51"), "{end}");
    assert_eq!(sense, Sense::Forward);
}

#[test]
fn spline_edge_against_its_curve_reverses_the_stored_direction() {
    let (start, end, sense) = decode_spline_edge([1.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
    assert!(start.ends_with("#51"), "{start}");
    assert!(end.ends_with("#50"), "{end}");
    assert_eq!(sense, Sense::Reversed);
}

#[test]
fn spline_edge_with_ends_off_its_vertices_keeps_the_stored_direction() {
    let (start, end, sense) = decode_spline_edge([2.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
    assert!(start.ends_with("#50"), "{start}");
    assert!(end.ends_with("#51"), "{end}");
    assert_eq!(sense, Sense::Forward);
}

fn polyline_carrier(points: &[[f64; 3]]) -> crate::brep::CurveCarrier {
    let mut knots = vec![0.0];
    knots.extend((0..points.len()).map(|index| f64::from(u32::try_from(index).unwrap())));
    knots.push(knots[knots.len() - 1]);
    let controls = points
        .iter()
        .map(|&[x, y, z]| Point3::new(x, y, z))
        .collect::<Vec<_>>();
    crate::brep::CurveCarrier {
        runs_against_edges: false,
        attr: 1,
        offset: 0,
        end: 0,
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(1, knots, controls, None, false).unwrap(),
        )),
        parameter_range: None,
    }
}

fn domain_reversal(carrier: &crate::brep::CurveCarrier, ends: [[f64; 3]; 2]) -> Option<bool> {
    super::domain_endpoint_reversal(
        &cadmpeg_test_support::service_decode_context(),
        carrier,
        Some(ends.map(|[x, y, z]| Point3::new(x, y, z))),
        [super::LINEAR_RESOLUTION_MM; 2],
    )
    .unwrap()
}

#[test]
fn domain_ends_within_the_linear_resolution_decide_the_direction() {
    let carrier = polyline_carrier(&[[240.0, -130.0, -50.0], [250.0, -150.0, -60.0]]);
    let off = 0.5 * super::LINEAR_RESOLUTION_MM;
    assert_eq!(
        domain_reversal(
            &carrier,
            [[250.0 + off, -150.0, -60.0], [240.0, -130.0, -50.0]]
        ),
        Some(true)
    );
    assert_eq!(
        domain_reversal(
            &carrier,
            [[240.0, -130.0, -50.0 + off], [250.0, -150.0, -60.0]]
        ),
        Some(false)
    );
}

#[test]
fn closed_curve_domain_ends_leave_the_direction_undecided() {
    let carrier = polyline_carrier(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]]);
    assert_eq!(
        domain_reversal(&carrier, [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]),
        None
    );
}

#[test]
fn an_edge_shorter_than_its_vertex_bounds_takes_the_nearer_order() {
    let length = 0.5 * super::LINEAR_RESOLUTION_MM;
    let carrier = polyline_carrier(&[[0.0, 0.0, 0.0], [length, 0.0, 0.0]]);
    assert_eq!(
        domain_reversal(&carrier, [[length, 0.0, 0.0], [0.0, 0.0, 0.0]]),
        Some(true)
    );
}

fn unit_circle() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("circle frame"),
    ))
}

#[test]
fn a_compact_circle_edge_takes_its_range_from_its_vertices_and_marker() {
    use std::f64::consts::{FRAC_PI_2, TAU};

    let circle = unit_circle();
    let bound = [super::LINEAR_RESOLUTION_MM; 2];
    let quarter = [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)];
    // Along: from θ(start) = 0 forward to θ(end) = π/2.
    assert_eq!(
        super::conic_edge_range(&circle, false, quarter, bound, false),
        Some(([0.0, FRAC_PI_2], false))
    );
    // Against: from θ(end) = π/2 forward to θ(start) + 2π, and the edge reverses.
    assert_eq!(
        super::conic_edge_range(&circle, true, quarter, bound, false),
        Some(([FRAC_PI_2, TAU], true))
    );
    // A closed edge spans one turn from its vertex at θ = 3π/2.
    let seam = Point3::new(0.0, -1.0, 0.0);
    let (range, reversed) =
        super::conic_edge_range(&circle, false, [seam, seam], bound, true).expect("closed range");
    assert!(!reversed);
    assert!((range[0] - 3.0 * FRAC_PI_2).abs() < 1e-12);
    assert!((range[1] - range[0] - TAU).abs() < 1e-12);
    // The marker selects the direction of a closed edge.
    let (_, reversed) =
        super::conic_edge_range(&circle, true, [seam, seam], bound, true).expect("closed range");
    assert!(reversed);
    // An open edge whose two vertices coincide has no range.
    assert_eq!(
        super::conic_edge_range(&circle, false, [seam, seam], bound, false),
        None
    );
}

#[test]
fn a_tolerant_vertex_bounds_a_conic_within_its_tolerance() {
    let circle = unit_circle();
    // The end vertex is 0.002 mm off the circle's plane, inside a stored
    // 0.0035 mm tolerance; the arc is the quarter turn between.
    let endpoints = [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.002)];
    let exact = super::LINEAR_RESOLUTION_MM;
    assert_eq!(
        super::conic_edge_range(&circle, false, endpoints, [exact; 2], false),
        None
    );
    let (range, reversed) =
        super::conic_edge_range(&circle, false, endpoints, [exact, 0.0035], false).unwrap();
    assert!(!reversed);
    assert!(range[0].abs() < 1e-12);
    assert!((range[1] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}

#[test]
fn a_stored_range_matches_a_tolerant_vertex_within_its_tolerance() {
    let circle = unit_circle();
    let endpoints = [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.002)];
    let carrier = crate::brep::CurveCarrier {
        runs_against_edges: false,
        attr: 1,
        offset: 0,
        end: 0,
        geometry: circle,
        parameter_range: Some(
            cadmpeg_ir::units::FiniteVector::new([0.0, std::f64::consts::FRAC_PI_2])
                .expect("finite fixture range"),
        ),
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::edge_parameter_range(&ctx, &carrier, Some(endpoints), [None; 2], false).unwrap(),
        None
    );
    assert_eq!(
        super::edge_parameter_range(&ctx, &carrier, Some(endpoints), [None, Some(0.0035)], false)
            .unwrap(),
        Some(([0.0, std::f64::consts::FRAC_PI_2], false))
    );
}

#[test]
fn a_carrier_angle_is_the_parametric_angle_in_a_whole_turn() {
    use std::f64::consts::FRAC_PI_4;

    let ellipse = CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(
        cadmpeg_ir::geometry::analytic::EllipseCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
        )
        .expect("ellipse frame"),
    ));
    // (2 cos t, sin t) at t = π/4; its polar angle is atan(1/2).
    let point = Point3::new(2.0_f64.sqrt(), 0.5_f64.sqrt(), 0.0);
    let angle = super::carrier_angle(&ellipse, point, 1e-12).expect("on the ellipse");
    assert!((angle - FRAC_PI_4).abs() < 1e-12);
    // Below the reference direction the angle is in [π, 2π).
    let below = super::carrier_angle(&unit_circle(), Point3::new(0.0, -1.0, 0.0), 1e-12)
        .expect("on the circle");
    assert!((below - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    // The centre has no angle.
    assert_eq!(
        super::carrier_angle(&unit_circle(), Point3::new(0.0, 0.0, 0.0), f64::INFINITY),
        None
    );
}

#[test]
fn a_stored_range_shorter_than_its_vertex_tolerances_takes_the_nearer_order() {
    // A 1e-3 rad arc inside 0.01 mm tolerances matches its vertices both ways;
    // its ends meet them exactly in the reversed order.
    let carrier = crate::brep::CurveCarrier {
        runs_against_edges: false,
        attr: 1,
        offset: 0,
        end: 0,
        geometry: unit_circle(),
        parameter_range: Some(
            cadmpeg_ir::units::FiniteVector::new([0.0, 1.0e-3]).expect("finite fixture range"),
        ),
    };
    let endpoints = [
        Point3::new(1.0e-3_f64.cos(), 1.0e-3_f64.sin(), 0.0),
        Point3::new(1.0, 0.0, 0.0),
    ];
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        super::edge_parameter_range(&ctx, &carrier, Some(endpoints), [Some(0.01); 2], false)
            .unwrap(),
        Some(([0.0, 1.0e-3], true))
    );
}

#[test]
fn a_full_turn_edge_range_starts_at_its_vertex() {
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::topology::{Edge, Point, Vertex};

    let curve_id = CurveId::mint("test:model:curve#circle").expect("curve id");
    let mut brep = super::Brep::default();
    brep.curves.push(cadmpeg_ir::geometry::Curve {
        parameter_range: None,
        id: curve_id.clone(),
        geometry: unit_circle(),
        source_object: None,
    });
    let point_id = PointId::mint("test:model:point#seam").expect("point id");
    brep.points.push(Point::new(
        point_id.clone(),
        cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, -1.0, 0.0)).expect("finite point"),
        None,
    ));
    let vertex_id = VertexId::mint("test:model:vertex#seam").expect("vertex id");
    brep.vertices.push(Vertex {
        id: vertex_id.clone(),
        point: point_id,
        tolerance: None,
    });
    // A sentinel closed circle starts at its reference direction; its vertex
    // has since moved to θ = 3π/2.
    brep.edges.push(Edge {
        id: EdgeId::mint("test:model:edge#circle").expect("edge id"),
        carrier: cadmpeg_ir::topology::EdgeCarrier::new(
            Some(curve_id),
            Some([0.0, std::f64::consts::TAU]),
        )
        .expect("edge carrier"),
        start: vertex_id.clone(),
        end: vertex_id,
        tolerance: None,
    });
    super::phase_full_turn_edge_ranges(&mut brep);
    let [start, end] = brep.edges[0]
        .param_range()
        .expect("the edge keeps its range")
        .get();
    assert!((start - 3.0 * std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!((end - start - std::f64::consts::TAU).abs() < 1.0e-12);
}
