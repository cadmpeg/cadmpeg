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
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid curve"),
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
        &ctx,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    ) else {
        panic!("four knots exceed three collection items")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("empty root fits service policy");
    assert!(super::project_edge(
        &ctx,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service budget")
    .is_some());
}

#[test]
fn projected_sketch_nurbs_poles_refuse_collection_limit() {
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
    let source_points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let points = HashMap::from([
        (&start_point, source_points[0]),
        (&end_point, source_points[1]),
    ]);
    let knot_values = vec![0.0, 0.0, 1.0, 1.0];
    let knot_count = knot_values.len();
    let pole_count = source_points.len();
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            knot_values,
            source_points.to_vec(),
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    let one_below_projected_slots = knot_count
        .checked_add(pole_count)
        .and_then(|slots| slots.checked_sub(1))
        .expect("fixture collection count fits");
    assert_eq!(knot_count, 4);
    assert_eq!(pole_count, 2);
    assert_eq!(one_below_projected_slots, 5);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items =
        u64::try_from(one_below_projected_slots).expect("fixture count fits u64");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits collection policy");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = super::project_edge(
        &ctx,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    ) else {
        panic!("projected poles exceed the remaining collection item slot")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::CollectionItems
    );
    assert_eq!(limit.used, 4);
    assert_eq!(limit.additional, 2);
    assert_eq!(
        limit.operation,
        "project SLDPRT sketch NURBS edge"
    );
}

#[test]
fn projected_sketch_nurbs_poles_refuse_work_limit() {
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
    let source_points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    let points = HashMap::from([
        (&start_point, source_points[0]),
        (&end_point, source_points[1]),
    ]);
    let knot_values = vec![0.0, 0.0, 1.0, 1.0];
    let knot_count = knot_values.len();
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            knot_values,
            source_points.to_vec(),
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(0.0, 0.0, 0.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
        v_axis: Vector3::new(0.0, 1.0, 0.0),
    };
    // Prefix work is the five source map-key byte lengths plus the knot lane.
    let prefix_work = [
        start_vertex.as_str().len(),
        start_point.as_str().len(),
        end_vertex.as_str().len(),
        end_point.as_str().len(),
        curve_id.as_str().len(),
        knot_count,
    ]
    .into_iter()
    .fold(0_usize, |total, amount| {
        total.checked_add(amount).expect("fixture work count fits")
    });
    assert_eq!(knot_count, 4);
    assert_eq!(prefix_work, 141);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units =
        u64::try_from(prefix_work).expect("fixture work count fits u64");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits work policy");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = super::project_edge(
        &ctx,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    ) else {
        panic!("first projected pole exceeds the remaining work units")
    };
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.used, 141);
    assert_eq!(limit.additional, 1);
    assert_eq!(
        limit.operation,
        "project SLDPRT sketch NURBS edge"
    );
}

#[test]
fn projected_polynomial_sketch_nurbs_preserves_pole_order_and_knots() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
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
    let source_points = [
        Point3::new(10.0, 4.0, 8.0),
        Point3::new(30.0, -3.0, 1.0),
        Point3::new(-10.0, 0.0, 5.0),
    ];
    let points = HashMap::from([
        (&start_point, source_points[0]),
        (&end_point, source_points[2]),
    ]);
    let knots = [0.0, 0.0, 0.0, 2.0, 2.0, 2.0];
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            knots.to_vec(),
            source_points.to_vec(),
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(10.0, -5.0, 2.0),
        u_axis: Vector3::new(0.0, 1.0, 0.0),
        v_axis: Vector3::new(0.0, 0.0, 1.0),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("empty root fits service policy");
    let projected = super::project_edge(
        &ctx,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service budget")
    .expect("NURBS curve projects");

    let SketchGeometryDefinition::Nurbs { curve } = projected.definition() else {
        panic!("projected curve remains NURBS")
    };
    assert_eq!(curve.degree(), 2);
    assert_eq!(curve.knots().as_slice(), knots.as_slice());
    assert!(!curve.periodic());
    assert!(curve.weights().is_none());
    let PcurveNurbsPoles::Polynomial { points } = curve.pole_rows() else {
        panic!("polynomial source poles remain polynomial")
    };
    assert_eq!(
        points.iter().map(|point| point.get()).collect::<Vec<_>>(),
        vec![
            Point2::new(9.0, 6.0),
            Point2::new(2.0, -1.0),
            Point2::new(5.0, 3.0),
        ]
    );
}

#[test]
fn projected_rational_sketch_nurbs_preserves_pole_order_weights_and_knots() {
    use cadmpeg_ir::geometry::nurbs::NurbsCurve;
    use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::ids::{CurveId, EdgeId, PointId, VertexId};
    use cadmpeg_ir::math::{Point3, Vector3};
    use cadmpeg_ir::sketches::SketchGeometryDefinition;
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
    let source_points = [
        Point3::new(10.0, 4.0, 8.0),
        Point3::new(30.0, -3.0, 1.0),
        Point3::new(-10.0, 0.0, 5.0),
    ];
    let points = HashMap::from([
        (&start_point, source_points[0]),
        (&end_point, source_points[2]),
    ]);
    let knots = [0.0, 0.0, 0.0, 2.0, 2.0, 2.0];
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            knots.to_vec(),
            source_points.to_vec(),
            Some(vec![2.0, 3.0, 5.0]),
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(10.0, -5.0, 2.0),
        u_axis: Vector3::new(0.0, 1.0, 0.0),
        v_axis: Vector3::new(0.0, 0.0, 1.0),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("empty root fits service policy");
    let projected = super::project_edge(
        &ctx,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service budget")
    .expect("NURBS curve projects");

    let SketchGeometryDefinition::Nurbs { curve } = projected.definition() else {
        panic!("projected curve remains NURBS")
    };
    assert_eq!(curve.degree(), 2);
    assert_eq!(curve.knots().as_slice(), knots.as_slice());
    assert!(!curve.periodic());
    assert_eq!(
        curve.weights().map(|weights| {
            weights.into_iter().map(|weight| weight.get()).collect::<Vec<_>>()
        }),
        Some(vec![2.0, 3.0, 5.0])
    );
    let PcurveNurbsPoles::Rational { points } = curve.pole_rows() else {
        panic!("rational source poles remain rational")
    };
    assert_eq!(
        points.iter().map(|point| point.point.get()).collect::<Vec<_>>(),
        vec![
            Point2::new(9.0, 6.0),
            Point2::new(2.0, -1.0),
            Point2::new(5.0, 3.0),
        ]
    );
}

#[test]
fn projected_rational_sketch_nurbs_weights_refuse_work_limit() {
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
    let source_points = [
        Point3::new(10.0, 4.0, 8.0),
        Point3::new(30.0, -3.0, 1.0),
        Point3::new(-10.0, 0.0, 5.0),
    ];
    let points = HashMap::from([
        (&start_point, source_points[0]),
        (&end_point, source_points[2]),
    ]);
    let knots = [0.0, 0.0, 0.0, 2.0, 2.0, 2.0];
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            2,
            knots.to_vec(),
            source_points.to_vec(),
            Some(vec![2.0, 3.0, 5.0]),
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid curve"),
    ));
    let curves = HashMap::from([(&curve_id, &geometry)]);
    let frame = super::SketchPlaneFrame {
        origin: Point3::new(10.0, -5.0, 2.0),
        u_axis: Vector3::new(0.0, 1.0, 0.0),
        v_axis: Vector3::new(0.0, 0.0, 1.0),
    };
    // Prefix work counts five map keys, six copied knots and three projected poles.
    let prefix_work = [
        start_vertex.as_str().len(),
        start_point.as_str().len(),
        end_vertex.as_str().len(),
        end_point.as_str().len(),
        curve_id.as_str().len(),
        knots.len(),
        source_points.len(),
    ]
    .into_iter()
    .fold(0_usize, |total, amount| {
        total.checked_add(amount).expect("fixture work count fits")
    });
    assert_eq!(prefix_work, 146);
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units =
        u64::try_from(prefix_work).expect("fixture work count fits u64");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits work policy");
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = super::project_edge(
        &ctx,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    else {
        panic!("first projected weight exceeds the remaining work units")
    };
    assert_eq!(limit.dimension, cadmpeg_core::decode::ResourceDimension::WorkUnits);
    assert_eq!(limit.used, 146);
    assert_eq!(limit.additional, 1);
    assert_eq!(limit.operation, "project SLDPRT sketch NURBS edge");
}

fn shared_endpoint_constraints(
    policy: &cadmpeg_core::decode::DecodePolicy,
) -> Result<Vec<cadmpeg_ir::sketches::SketchConstraint>, cadmpeg_core::CodecError> {
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, policy)
        .expect("empty root fits policy");
    let mut annotations = cadmpeg_ir::annotations::Annotations::default();
    let mut constraints = Vec::new();
    super::project_endpoint_constraints(
        &ctx,
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
    )?;
    Ok(constraints)
}

#[test]
fn sketch_projection_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = shared_endpoint_constraints(&policy)
    else {
        panic!("shared endpoints must charge collection items")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(
        shared_endpoint_constraints(&DecodePolicy::service())
            .expect("service budget")
            .len(),
        1
    );
}

#[test]
fn sketch_projection_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = shared_endpoint_constraints(&policy)
    else {
        panic!("shared endpoints must charge retained bytes")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        shared_endpoint_constraints(&DecodePolicy::service())
            .expect("service budget")
            .len(),
        1
    );
}

#[test]
fn sketch_projection_refuses_work_limit() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};

    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) = shared_endpoint_constraints(&policy)
    else {
        panic!("shared endpoints must charge work")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(
        shared_endpoint_constraints(&DecodePolicy::service())
            .expect("service budget")
            .len(),
        1
    );
}

#[test]
fn sketch_projection_refuses_opaque_curve_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let error = super::project_edge(
        &limited,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect_err("opaque curve text exceeds retained limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain SLDPRT opaque sketch curve"
    ));
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("empty root");
    assert!(super::project_edge(
        &service,
        &edge,
        &vertices,
        &points,
        &curves,
        frame,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("service budget")
    .is_some());
}
