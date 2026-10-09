// SPDX-License-Identifier: Apache-2.0
use crate::curve::CurveTopologyRow;
use crate::decode::records::{
    face_component_records, half_edge_records, half_edge_vertex_incidence_records,
    loop_array_frame_records, loop_array_record_records, loop_records, topological_vertex_records,
};
use crate::loop_array::{LoopArrayFrame, LoopArrayRecord};
use crate::topology::{HalfEdge, HalfEdgeId, HalfEdgeVertexIncidence, Side};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use std::num::NonZeroU32;

fn scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let half_edge = HalfEdgeId {
        curve_id: 8,
        side: Side::Zero,
    };
    scan.curves.topology_rows.push(CurveTopologyRow {
        id: 8,
        type_byte: 1,
        feature_id: 2,
        directions: [0, 0],
        faces: [NonZeroU32::new(1), None],
        next_edges: [8, 0],
        offset: 13,
    });
    scan.topology.half_edges.push(HalfEdge {
        id: half_edge,
        face_id: NonZeroU32::new(1),
        next: Some(half_edge),
    });
    scan.topology.loops.push(crate::test_support::closed_loop(
        NonZeroU32::new(1),
        vec![half_edge],
    ));
    scan.topology.vertices.push(
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::TopologicalVertex::new_for_test(ctx, 1, vec![half_edge])
        })
        .expect("vertex admission")
        .expect("valid vertex fixture"),
    );
    scan.topology
        .half_edge_vertex_incidence
        .push(HalfEdgeVertexIncidence {
            half_edge,
            start_vertex_id: std::num::NonZeroU32::new(1).expect("one-based vertex fixture"),
            end_vertex_id: std::num::NonZeroU32::new(1),
        });
    scan.topology.face_components.push(
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::topology::FaceComponent::new_for_test(ctx, vec![1], vec![8])
        })
        .expect("component admission")
        .expect("valid component fixture"),
    );
    scan.loop_arrays.frames.push(LoopArrayFrame {
        offset: 17,
        variant: None,
        declared_count: 1,
        class_id: 4,
        prototype_end: 19,
        end: 23,
        overfull: false,
    });
    scan.loop_arrays.records.push(LoopArrayRecord {
        frame_offset: 17,
        lo_id: 3,
        lo_type: 0,
        lo_subtype: 0,
        feature_id: 2,
        attributes: 0,
        direction: 0,
        next_lo_ptr: 0,
        body: vec![0xe3],
        offset: 19,
        body_offset: 22,
    });
    scan
}

macro_rules! collection_limit_test {
        ($name:ident, $projection:ident, $operation:literal) => {
            #[test]
            fn $name() {
                let scan = scan();
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
      ResourceDimension::CollectionItems, Some($operation), |cap| {
          let trial_arena = DecodeArena::new();
          let mut trial_policy = DecodePolicy::service();
          trial_policy.limits.max_collection_items = cap;
          let (trial_ctx, _) = DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
          $projection(&trial_ctx, &scan).map(|_| ())
      });

                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("empty root is admitted");
                let Err(error) = $projection(&ctx, &scan) else { panic!("one native record exceeds the collection limit") };
                assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
                    if resource.dimension == ResourceDimension::CollectionItems
                        && resource.operation == $operation), "{error:?}");
            }
        };
    }

collection_limit_test!(
    half_edge_topology_nodes_refuse_limit,
    half_edge_records,
    "creo native half edge topology row nodes"
);
collection_limit_test!(
    half_edge_records_refuse_limit,
    half_edge_records,
    "creo native half edge records"
);
collection_limit_test!(
    loop_records_refuse_limit,
    loop_records,
    "creo native loop records"
);
collection_limit_test!(
    loop_array_frame_count_nodes_refuse_limit,
    loop_array_frame_records,
    "creo native loop array frame count nodes"
);
collection_limit_test!(
    loop_array_frame_records_refuse_limit,
    loop_array_frame_records,
    "creo native loop array frame records"
);
collection_limit_test!(
    loop_array_records_refuse_limit,
    loop_array_record_records,
    "creo native loop array records"
);
collection_limit_test!(
    topological_vertex_records_refuse_limit,
    topological_vertex_records,
    "creo native topological vertex records"
);
collection_limit_test!(
    half_edge_vertex_incidence_records_refuse_limit,
    half_edge_vertex_incidence_records,
    "creo native half edge vertex incidence records"
);
collection_limit_test!(
    face_component_records_refuse_limit,
    face_component_records,
    "creo native face component records"
);

#[test]
fn borrowed_topology_projection_preserves_half_edges_and_body() {
    let scan = scan();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let loops_parts = loop_records(&ctx, &scan).expect("loop is admitted");
    let _loops_storage = loops_parts.1;
    let loops = loops_parts.0;
    let rows_parts = loop_array_record_records(&ctx, &scan).expect("row is admitted");
    let _rows_storage = rows_parts.1;
    let rows = rows_parts.0;
    let loop_value = serde_json::to_value(&loops[0]).expect("loop serializes");
    let row_value = serde_json::to_value(&rows[0]).expect("row serializes");
    assert_eq!(
        loop_value["half_edges"],
        serde_json::json!([{"curve_id":8,"side":0}])
    );
    assert_eq!(row_value["body"], serde_json::json!([0xe3]));
    assert_eq!(row_value["end"], serde_json::json!(23));
}
