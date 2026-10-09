// SPDX-License-Identifier: Apache-2.0

use super::*;
use super::super::CompositeIndex;

fn first_index_step(ir: &CadIr, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match CompositeIndex::from_ir(ir, &ctx) {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected first actual index source step refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [ir, &CadIr::empty()] {
        assert!(matches!(CompositeIndex::from_ir(replay, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    let mut empty = CompositeIndex::default();
    assert!(matches!(empty.refresh_topology(&CadIr::empty(), &ctx),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn composite_curve_index_refuses_one_visit_before_its_tail() {
    let mut ir = CadIr::empty();
    for suffix in ["one", "two", "three"] {
        ir.model.curves.push(Curve {
            id: CurveId::mint(format!("test:model:curve#{suffix}")).unwrap(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0),
                ).unwrap(),
            )),
            source_object: None,
        });
    }
    first_index_step(&ir, "iges composite curve index traversal");
}

#[test]
fn composite_edge_index_refuses_one_visit_before_its_tail() {
    let mut ir = CadIr::empty();
    for suffix in ["one", "two", "three"] {
        ir.model.edges.push(Edge {
            id: EdgeId::mint(format!("test:model:edge#{suffix}")).unwrap(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(None, None).unwrap(),
            start: VertexId::mint("test:model:vertex#start").unwrap(),
            end: VertexId::mint("test:model:vertex#end").unwrap(),
            tolerance: None,
        });
    }
    first_index_step(&ir, "iges composite edge index traversal");
}

#[test]
fn composite_point_index_refuses_one_visit_before_its_tail() {
    let mut ir = CadIr::empty();
    for suffix in ["one", "two", "three"] {
        ir.model.points.push(Point::new(
            PointId::mint(format!("test:model:point#{suffix}")).unwrap(),
            cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
            None,
        ));
    }
    first_index_step(&ir, "iges composite point index traversal");
}

#[test]
fn composite_vertex_index_refuses_one_visit_before_its_tail() {
    let mut ir = CadIr::empty();
    for suffix in ["one", "two", "three"] {
        ir.model.vertices.push(Vertex {
            id: VertexId::mint(format!("test:model:vertex#{suffix}")).unwrap(),
            point: PointId::mint("test:model:point#position").unwrap(),
            tolerance: None,
        });
    }
    first_index_step(&ir, "iges composite vertex index traversal");
}

#[test]
fn empty_composite_indexes_execute_no_source_steps() {
    let ir = CadIr::empty();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut index = CompositeIndex::from_ir(&ir, &ctx).unwrap();
    index.refresh_topology(&ir, &ctx).unwrap();
    assert!(index.curve_positions.is_empty());
    assert!(index.edges.is_empty());
    assert!(index.points.is_empty());
    assert!(index.vertex_points.is_empty());
    ctx.finish_session().unwrap();
}
