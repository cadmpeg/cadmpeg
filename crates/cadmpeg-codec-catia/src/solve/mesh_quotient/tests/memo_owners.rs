//! Actual mesh parser memo admission.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn mesh_endpoint_memo_insertion_refuses_at_actual_candidate_boundary() {
    let run = |ctx: &DecodeContext<'_>, bytes: &[u8], candidates: &[Vec<[usize; 2]>], faces: &[[usize; 2]]| {
        let edge_count = candidates.len();
        let classes = (0..edge_count).collect::<Vec<_>>();
        let geometry = vec![super::super::MeshEdgeGeometry::Line; edge_count];
        let identity = vec![true; edge_count];
        let unconstrained = vec![false; edge_count];
        let budget = ctx.work_budget(cadmpeg_core::decode::u64_from_index(
            super::super::MAX_MESH_TOPOLOGY_OPERATIONS));
        super::super::parse_standard_mesh_candidate_outcome(ctx,
            super::super::ParseStandardMeshCandidateOutcomeInputs {
                bytes,
                edge_faces: faces,
                edge_candidates: candidates,
                edge_classes: &classes,
                edge_geometry: &geometry,
                edge_identity_evidence: &identity,
                edge_direction_evidence: &unconstrained,
                global_handle_ports: false,
                partial_constraint_edges: &unconstrained,
                preferred_assignment_edges: &unconstrained,
                priority_edges: None,
                assignment_dependencies: None,
                budget: &budget,
                partial_solution_valid: |_| Ok(true),
                complete_solution_valid: |_| Ok(true),
            })
    };
    let quad = crate::test_support::test_topology::standard_quad_topology_stream();
    let quad_candidates = [vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]];
    crate::test_support::with_service_context(|ctx| {
        let CodecError::ResourceLimit(limit) = run(ctx, &quad, &quad_candidates, &[[0, 0]; 4])
            .expect_err("the ordered quad reaches the existing face ceiling")
        else { panic!("resource refusal"); };
        assert_eq!(limit.operation, "catia_ordered_face_constraint_work");
        assert_eq!(limit.limit, 64);
        assert_eq!(limit.used, 64);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });

    let bytes = crate::test_support::test_topology::compact_standard_triangle_topology_stream();
    let candidates = [vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]];
    let solved = crate::test_support::with_service_context(|ctx| run(ctx, &bytes, &candidates, &[[0, 0]; 3]))
        .expect("service budget");
    let super::super::MeshSolve::Solved((topology, points)) = solved else {
        panic!("the closed triangle must solve");
    };
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(topology.edge_rows.len(), 3);
    assert_eq!(topology.logical_vertex_count, 3);
    assert_eq!(points, vec![0, 1, 2]);

    // Two distinct incident faces pass the incidence validator without asking
    // it to fill an unresolved second face slot from a single saturated face.
    let mut memo_bytes = vec![0x01, 0x41, 0x01, 0xff, 0x03, 0, 0, 0, 0, 1, 2];
    memo_bytes.extend_from_slice(&bytes);
    memo_bytes.splice(22..22, [0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2]);
    let memo_faces = [[0, 1]; 3];
    let solved = crate::test_support::with_service_context(|ctx| run(ctx, &memo_bytes, &candidates, &memo_faces))
        .expect("two-face service budget");
    let super::super::MeshSolve::Solved((topology, points)) = solved else {
        panic!("the two closed triangle faces must solve");
    };
    assert_eq!(topology.faces.len(), 2);
    assert_eq!(topology.edge_rows.len(), 3);
    assert_eq!(topology.logical_vertex_count, 3);
    assert_eq!(points, vec![0, 1, 2]);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let service_items = policy.limits.max_collection_items;
    // The probe uses this sentinel to select its budget. Replay the discovered
    // boundary below with a finite cap below the unchanged service ceiling.
    policy.limits.max_collection_items = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&memo_bytes, &arena, &policy)
        .expect("synthetic two-face triangle input");
    let probe = RefusalProbe::arm(ResourceDimension::CollectionItems,
        "catia_endpoint_memo_entries", Some(1));
    let CodecError::ResourceLimit(limit) = run(&ctx, &memo_bytes, &candidates, &memo_faces).expect_err("actual memo entry admission")
    else {
        panic!("resource refusal");
    };
    drop(probe);
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "catia_endpoint_memo_entries");
    assert_eq!(limit.additional, 1);
    assert!(limit.limit < service_items);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(run(&ctx, &memo_bytes, &candidates, &memo_faces), Err(CodecError::ResourceLimit(later)) if later == limit));
    let finite = crate::test_support::with_collection_limit(limit.limit,
        |finite_ctx| run(finite_ctx, &memo_bytes, &candidates, &memo_faces));
    assert!(matches!(finite, Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
