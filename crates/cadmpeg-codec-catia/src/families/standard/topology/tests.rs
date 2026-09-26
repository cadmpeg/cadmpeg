use super::{incidence_cycles, solve_boundary_orientation_constraints, StandardTopology};
use std::collections::HashMap;

#[test]
fn standard_edge_vertices_propagate_collection_refusal() {
    use super::{Boundary, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopology};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary::new(vec![CoedgeUse {
                edge_row: 0,
                reversed: false,
                start_vertex: 0,
                end_vertex: 1,
            }])
            .expect("nonempty boundary")],
        }],
        edge_rows: vec![EdgeRow {
            kind: 1,
            handles: vec![10],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert_eq!(
        topology
            .edge_vertices(&ctx)
            .expect("service resource budget"),
        Some(vec![[0, 1]])
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = topology
        .edge_vertices(&ctx)
        .expect_err("edge array exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia standard edge vertices"));
}

#[test]
fn body_kinds_rejects_an_overflowing_face_group_sum() {
    let topology = StandardTopology {
        faces: Vec::new(),
        edge_rows: Vec::new(),
        vertex_points: Vec::new(),
        logical_vertex_count: 0,
    };

    assert_eq!(topology.body_kinds(&[usize::MAX, 1]), None);
}

#[test]
fn incidence_cycles_rejects_duplicate_and_out_of_range_edges() {
    assert!(incidence_cycles(&[0, 0], &[[0, 0]]).is_none());
    assert!(incidence_cycles(&[1], &[[0, 0]]).is_none());
}

#[test]
fn boundary_orientation_constraints_retain_the_flip_assignment() {
    let edge_uses = HashMap::from([
        (0, vec![(0, false), (1, false)]),
        (1, vec![(1, false), (2, true)]),
    ]);

    assert_eq!(
        solve_boundary_orientation_constraints(4, &edge_uses, false),
        Some(vec![false, true, true, false])
    );
}
