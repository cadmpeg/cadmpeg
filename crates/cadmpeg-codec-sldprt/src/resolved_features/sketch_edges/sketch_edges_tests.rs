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
fn projected_sketch_nurbs_refuses_collection_limit() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
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
        ).expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits policy");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = super::project_edge(
        &ctx, &edge, &vertices, &points, &curves, frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    ) else { panic!("four knots exceed three collection items") };
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::CollectionItems);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[], &arena, &cadmpeg_core::decode::DecodePolicy::service(),
    ).expect("empty root fits service policy");
    assert!(super::project_edge(
        &ctx, &edge, &vertices, &points, &curves, frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    ).expect("service budget").is_some());
}
