// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn composite_topology_index_extends_only_appended_records() {
    let decoded = IgesCodec
        .decode(
            &mut Cursor::new(composite_curve_file()),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut ir = decoded.ir().clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut storage = ctx
        .reserve_scoped(0, "test composite index storage")
        .unwrap();
    let index = storage
        .with_storage(|| CompositeIndex::from_ir(&ir, &ctx))
        .unwrap();
    let point = PointId::mint("test:model:point#appended").unwrap();
    let vertex = VertexId::mint("test:model:vertex#appended").unwrap();
    let position = ir.model.points[0].position();
    ir.model
        .points
        .push(Point::new(point.clone(), position, None));
    ir.model.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance: None,
    });
    let mut edge = ir.model.edges[0].clone();
    let curve = edge.curve().unwrap().clone();
    edge.id = EdgeId::mint("test:model:edge#appended").unwrap();
    edge.start = vertex.clone();
    ir.model.edges.push(edge);
    for operation in [
        "iges composite edge index traversal",
        "iges composite point index traversal",
        "iges composite vertex index traversal",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let mut index = index.clone();
                let mut storage = ctx
                    .reserve_scoped(0, "test appended topology storage")
                    .unwrap();
                storage.with_storage(|| index.refresh_topology(&ir, &ctx))
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
    }
    let mut index = index;
    let previous_edge_count = index.edges[&curve].len();
    storage
        .with_storage(|| index.refresh_topology(&ir, &ctx))
        .unwrap();
    assert_eq!(index.edges[&curve].len(), previous_edge_count + 1);
    assert_eq!(index.vertex_points[&vertex], position);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (empty_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    index.refresh_topology(&ir, &empty_ctx).unwrap();
}
