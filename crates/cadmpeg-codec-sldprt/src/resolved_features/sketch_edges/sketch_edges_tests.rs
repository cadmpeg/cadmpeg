//! Tests for the `sketch_edges` module.

use super::{circle_contains_point, ellipse_contains_point};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::PositiveLength;
use std::collections::HashMap;

fn length(value: f64) -> PositiveLength {
    PositiveLength::new(value).expect("positive radius")
}

#[test]
fn rejects_analytic_carriers_that_do_not_contain_the_edge_vertex() {
    assert!(!circle_contains_point(
        Point2::new(-35.0, -5.85),
        length(1.25),
        Point2::new(-75.0, -8.85),
        1.0e-9,
    ));
    assert!(!ellipse_contains_point(
        Point2::new(-60.0, -150.0),
        0.0,
        length(7.5),
        length(f64::MIN_POSITIVE),
        Point2::new(140.0, -70.5),
        1.0e-9,
    ));
}

#[test]
fn accepts_vertices_on_nondegenerate_analytic_carriers() {
    assert!(circle_contains_point(
        Point2::new(2.0, 3.0),
        length(4.0),
        Point2::new(6.0, 3.0),
        1.0e-9,
    ));
    assert!(ellipse_contains_point(
        Point2::new(2.0, 3.0),
        0.0,
        length(4.0),
        length(2.0),
        Point2::new(2.0, 5.0),
        1.0e-9,
    ));
}

#[test]
fn projected_sketch_nurbs_edge_projects() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};

    let curve_id = CurveId::mint("test:model:entity#curve").expect("curve id");
    let start_vertex = VertexId::mint("test:model:entity#start-vertex").expect("vertex id");
    let end_vertex = VertexId::mint("test:model:entity#end-vertex").expect("vertex id");
    let start_point = PointId::mint("test:model:entity#start-point").expect("point id");
    let end_point = PointId::mint("test:model:entity#end-point").expect("point id");
    let edge = Edge {
        native_ref: None,
        id: EdgeId::mint("test:model:entity#edge").expect("edge id"),
        carrier: EdgeCarrier::unbounded(Some(curve_id.clone())),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
        tolerance: None,
    };
    let vertices = HashMap::from([(&start_vertex, &start_point), (&end_vertex, &end_point)]);
    let points = HashMap::from([
        (&start_point, Point3::new(0.0, 0.0, 0.0)),
        (&end_point, Point3::new(1.0, 0.0, 0.0)),
    ]);
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let (geometry, start_aligned) = super::project_edge(
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        Some([0.0, 1.0]),
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("the NURBS edge projects");
    assert!(start_aligned);
    assert!(matches!(
        geometry.definition(),
        cadmpeg_ir::sketches::SketchGeometryDefinition::Nurbs { curve }
            if curve.pole_rows().point_at(0).map(cadmpeg_ir::units::FinitePoint2::get)
                == Some(Point2::new(0.0, 0.0))
                && curve.pole_rows().point_at(1).map(cadmpeg_ir::units::FinitePoint2::get)
                    == Some(Point2::new(1.0, 0.0))
    ));
    let swapped = HashMap::from([
        (&start_point, Point3::new(1.0, 0.0, 0.0)),
        (&end_point, Point3::new(0.0, 0.0, 0.0)),
    ]);
    let (_, start_aligned) = super::project_edge(
        &edge,
        &vertices,
        &swapped,
        &curves,
        frame,
        Some([1.0, 0.0]),
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("the NURBS edge projects against swapped vertices");
    assert!(!start_aligned);
}

#[test]
fn a_nurbs_edge_whose_carrier_misses_its_vertices_projects_as_its_chord() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};

    let curve_id = CurveId::mint("test:model:entity#curve").expect("curve id");
    let start_vertex = VertexId::mint("test:model:entity#start-vertex").expect("vertex id");
    let end_vertex = VertexId::mint("test:model:entity#end-vertex").expect("vertex id");
    let start_point = PointId::mint("test:model:entity#start-point").expect("point id");
    let end_point = PointId::mint("test:model:entity#end-point").expect("point id");
    let edge = Edge {
        id: EdgeId::mint("test:model:entity#edge").expect("edge id"),
        carrier: EdgeCarrier::unbounded(Some(curve_id.clone())),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
        tolerance: None,
    };
    let vertices = HashMap::from([(&start_vertex, &start_point), (&end_vertex, &end_point)]);
    let points = HashMap::from([
        (&start_point, Point3::new(0.0, 0.0, 0.0)),
        (&end_point, Point3::new(1.0, 0.0, 0.0)),
    ]);
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 5.0, 0.0), Point3::new(1.0, 5.0, 0.0)],
            None,
            false,
        )
        .expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let (geometry, start_aligned) = super::project_edge(
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("an unusable carrier still projects");
    assert!(start_aligned);
    assert!(matches!(
        geometry.definition(),
        cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end }
            if *start == Point2::new(0.0, 0.0) && *end == Point2::new(1.0, 0.0)
    ));
}

#[test]
fn sketch_projection_shares_endpoint_constraint() {
    use cadmpeg_ir::sketches::{
        SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
    };

    let sketch = SketchId::mint("test:model:sketch#shared").expect("valid sketch ID");
    let geometry = SketchGeometry::try_from(SketchGeometryDefinition::Line {
        start: Point2::new(0.0, 0.0),
        end: Point2::new(1.0, 0.0),
    })
    .expect("valid line");
    let entities = [
        SketchEntity::new(
            SketchEntityId::mint("test:model:sketch-entity#first").expect("valid entity ID"),
            sketch.clone(),
            geometry.clone(),
        )
        .with_endpoint_refs(vec!["first".into(), "shared".into()]),
        SketchEntity::new(
            SketchEntityId::mint("test:model:sketch-entity#second").expect("valid entity ID"),
            sketch.clone(),
            geometry,
        )
        .with_endpoint_refs(vec!["shared".into(), "second".into()]),
    ];
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut constraints = Vec::new();
    super::project_endpoint_constraints(
        super::EndpointConstraintSource {
            sketch: &sketch,
            entities: &entities,
            block_offset: 0,
            stream_ordinal: 0,
            face_ordinal: 0,
            stream: &cadmpeg_ir::stream_name!("test:shared-endpoint"),
        },
        &mut annotations,
        &mut constraints,
    )
    .expect("project endpoint constraints");
    assert_eq!(constraints.len(), 1);
}

#[test]
fn sketch_projection_projects_opaque_curve() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, UnknownId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};

    let curve_id = CurveId::mint("test:model:curve#opaque").expect("curve ID");
    let start_vertex = VertexId::mint("test:model:vertex#start").expect("start vertex ID");
    let end_vertex = VertexId::mint("test:model:vertex#end").expect("end vertex ID");
    let start_point = PointId::mint("test:model:point#start").expect("start point ID");
    let end_point = PointId::mint("test:model:point#end").expect("end point ID");
    let edge = Edge {
        native_ref: None,
        id: EdgeId::mint("test:model:edge#opaque").expect("edge ID"),
        carrier: EdgeCarrier::unbounded(Some(curve_id.clone())),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
        tolerance: None,
    };
    let vertices = HashMap::from([(&start_vertex, &start_point), (&end_vertex, &end_point)]);
    let points = HashMap::from([
        (&start_point, Point3::new(0.0, 0.0, 0.0)),
        (&end_point, Point3::new(1.0, 0.0, 0.0)),
    ]);
    let record = format!("test:model:unknown#{}", "x".repeat(128));
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
        record: Some(UnknownId::mint(record).expect("unknown record ID")),
    });
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    assert!(super::project_edge(
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .is_some());
}

#[test]
fn degree_one_nurbs_parameter_inverts_each_segment() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::Point3;

    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        None,
        false,
    )
    .expect("valid degree-one curve");
    let parameter = super::degree_one_nurbs_parameter(&curve, Point3::new(1.0, 0.25, 0.0), 1.0e-9)
        .expect("the point is on the second segment");
    assert!((parameter - 0.625).abs() < 1.0e-9);
    assert!(
        super::degree_one_nurbs_parameter(&curve, Point3::new(2.0, 0.0, 0.0), 1.0e-9).is_none()
    );
}

#[test]
fn a_rational_degree_one_span_inverts_through_its_weights() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::math::Point3;

    // With P0 at the origin, C(t) = 2t·P1 / (1 + t), which is the midpoint
    // when 2t / (1 + t) = 1/2, so t = 1/3.
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]),
        false,
    )
    .expect("valid rational degree-one curve");
    let parameter = super::degree_one_nurbs_parameter(&curve, Point3::new(1.0, 0.0, 0.0), 1.0e-9)
        .expect("the midpoint is on the span");
    assert!((parameter - 1.0 / 3.0).abs() < 1.0e-12);
}

/// A one-vertex edge on the closed square `(0,0) → (1,0) → (1,1) → (0,1) → (0,0)`.
fn closed_square_edge(
    param_range: Option<[f64; 2]>,
) -> (
    cadmpeg_ir::topology::Edge,
    cadmpeg_ir::ids::VertexId,
    cadmpeg_ir::ids::PointId,
    cadmpeg_ir::ids::CurveId,
    cadmpeg_ir::geometry::CurveGeometry,
) {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::Point3;
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};

    let curve_id = CurveId::mint("test:model:curve#square").expect("curve id");
    let vertex = VertexId::mint("test:model:vertex#corner").expect("vertex id");
    let point = PointId::mint("test:model:point#corner").expect("point id");
    let edge = Edge {
        id: EdgeId::mint("test:model:edge#square").expect("edge id"),
        carrier: EdgeCarrier::new(Some(curve_id.clone()), param_range).expect("edge carrier"),
        start: vertex.clone(),
        end: vertex.clone(),
        tolerance: None,
    };
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
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
            false,
        )
        .expect("valid closed square"),
    ));
    (edge, vertex, point, curve_id, geometry)
}

#[test]
fn a_one_vertex_edge_keeps_its_whole_carrier_when_its_range_does_not_clamp() {
    use cadmpeg_ir::math::{Point3, Vector3};

    let (edge, vertex, point, curve_id, geometry) = closed_square_edge(Some([5.0, 6.0]));
    let vertices = HashMap::from([(&vertex, &point)]);
    let points = HashMap::from([(&point, Point3::new(0.0, 0.0, 0.0))]);
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let (geometry, _) = super::project_edge(
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        Some([5.0, 6.0]),
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("the closed edge projects");
    let cadmpeg_ir::sketches::SketchGeometryDefinition::Nurbs { curve } = geometry.definition()
    else {
        panic!("the closed edge keeps its carrier");
    };
    assert_eq!(curve.control_points().len(), 5);
}

#[test]
fn a_circle_turning_clockwise_in_the_sketch_projects_a_negative_sweep() {
    use cadmpeg_ir::geometry::analytic::CircleCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};

    // About axis -z from reference +x, parameter π/2 is at -y: in the sketch
    // plane the arc turns clockwise from (1, 0) to (0, -1).
    let curve_id = CurveId::mint("test:model:curve#circle").expect("curve id");
    let start_vertex = VertexId::mint("test:model:vertex#start").expect("vertex id");
    let end_vertex = VertexId::mint("test:model:vertex#end").expect("vertex id");
    let start_point = PointId::mint("test:model:point#start").expect("point id");
    let end_point = PointId::mint("test:model:point#end").expect("point id");
    let edge = Edge {
        id: EdgeId::mint("test:model:edge#arc").expect("edge id"),
        carrier: EdgeCarrier::new(
            Some(curve_id.clone()),
            Some([0.0, std::f64::consts::FRAC_PI_2]),
        )
        .expect("edge carrier"),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
        tolerance: None,
    };
    let vertices = HashMap::from([(&start_vertex, &start_point), (&end_vertex, &end_point)]);
    let points = HashMap::from([
        (&start_point, Point3::new(1.0, 0.0, 0.0)),
        (&end_point, Point3::new(0.0, -1.0, 0.0)),
    ]);
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid circle"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let (geometry, _) = super::project_edge(
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        None,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("the arc projects");
    let cadmpeg_ir::sketches::SketchGeometryDefinition::Arc {
        start_angle,
        end_angle,
        ..
    } = geometry.definition()
    else {
        panic!("the edge is an arc");
    };
    assert!(start_angle.get().abs() < 1.0e-12);
    assert!((end_angle.get() + std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
}

#[test]
fn projected_handedness_follows_the_carrier_axis_sign() {
    use cadmpeg_ir::math::Vector3;

    let u_axis = Vector3::new(1.0, 0.0, 0.0);
    let v_axis = Vector3::new(0.0, 1.0, 0.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    assert!(
        super::projected_handedness(Vector3::new(0.0, 0.0, 1.0), reference, u_axis, v_axis) > 0.0
    );
    assert!(
        super::projected_handedness(Vector3::new(0.0, 0.0, -1.0), reference, u_axis, v_axis) < 0.0
    );
}

#[test]
fn a_cubic_edge_trims_to_its_interior_vertices() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};

    let curve_id = CurveId::mint("test:model:curve#cubic").expect("curve id");
    let start_vertex = VertexId::mint("test:model:vertex#start").expect("vertex id");
    let end_vertex = VertexId::mint("test:model:vertex#end").expect("vertex id");
    let start_point = PointId::mint("test:model:point#start").expect("point id");
    let end_point = PointId::mint("test:model:point#end").expect("point id");
    let edge = Edge {
        id: EdgeId::mint("test:model:edge#cubic").expect("edge id"),
        carrier: EdgeCarrier::unbounded(Some(curve_id.clone())),
        start: start_vertex.clone(),
        end: end_vertex.clone(),
        tolerance: None,
    };
    let vertices = HashMap::from([(&start_vertex, &start_point), (&end_vertex, &end_point)]);
    let start_position = Point3::new(0.75, 0.0, 0.0);
    let end_position = Point3::new(2.25, 0.0, 0.0);
    let points = HashMap::from([(&start_point, start_position), (&end_point, end_position)]);
    let curve = NurbsCurve::from_lanes(
        3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(3.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .expect("valid cubic curve");
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let vertex_tolerances = HashMap::from([(&start_vertex, None), (&end_vertex, None)]);
    let context = cadmpeg_test_support::service_decode_context();
    let range = super::nurbs_edge_trim_range(
        &context,
        &edge,
        &vertices,
        &vertex_tolerances,
        &points,
        &curves,
    )
    .expect("trim range")
    .expect("both vertices invert");
    assert!((range[0] - 0.25).abs() < 1.0e-9);
    assert!((range[1] - 0.75).abs() < 1.0e-9);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let (geometry, start_aligned) = super::project_edge(
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        Some(range),
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("the cubic edge projects");
    assert!(start_aligned);
    let cadmpeg_ir::sketches::SketchGeometryDefinition::Nurbs { curve } = geometry.definition()
    else {
        panic!("the trimmed edge is a NURBS");
    };
    let poles = curve.control_points();
    assert!((poles[0].get().u - 0.75).abs() < 1.0e-9);
    assert!((poles[poles.len() - 1].get().u - 2.25).abs() < 1.0e-9);
}
