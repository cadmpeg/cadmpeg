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
    assert!(super::project_edge(
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .is_some());
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
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .is_some());
}
