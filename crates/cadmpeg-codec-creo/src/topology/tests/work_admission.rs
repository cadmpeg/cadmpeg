// SPDX-License-Identifier: Apache-2.0
use super::row;

#[test]
fn faceless_rows_refuse_identity_and_graph_work() {
    let mut rows = [row(1, 2), row(2, 1)];
    for row in &mut rows {
        row.faces = [None, None];
    }
    let components = crate::test_support::assert_work_boundaries(
        &["creo unique-row identity work", "creo face graph assembly"],
        |ctx| super::super::face_components(ctx, &rows),
    );
    assert!(components.is_empty());
}

#[test]
fn face_component_graph_refuses_assembly_and_traversal_work() {
    let rows = [row(1, 2), row(2, 1)];
    let components = crate::test_support::assert_work_boundaries(
        &[
            "creo face graph assembly",
            "creo face graph seeds",
            "creo face graph traversal",
            "creo face graph curve memberships",
            "creo face graph neighbours",
            "creo face component projection",
        ],
        |ctx| super::super::face_components(ctx, &rows),
    );
    assert_eq!(components.len(), 1);
}

#[test]
fn vertex_orbits_refuse_before_graph_assembly_and_at_traversal() {
    let ids = [
        super::super::HalfEdgeId {
            curve_id: 1,
            side: super::super::Side::Zero,
        },
        super::super::HalfEdgeId {
            curve_id: 1,
            side: super::super::Side::One,
        },
    ];
    let edges = ids.map(|id| super::super::HalfEdge {
        id,
        face_id: None,
        next: Some(id),
    });
    let orbits = crate::test_support::assert_work_boundaries(
        &[
            "creo vertex graph assembly",
            "creo vertex graph adjacency",
            "creo vertex graph seeds",
            "creo vertex graph traversal",
            "creo vertex component union",
            "creo vertex orbit projection",
        ],
        |ctx| super::super::vertex_orbits(ctx, &edges),
    );
    assert_eq!(orbits.vertices.len(), 1);
    assert_eq!(orbits.vertices[0].half_edges(), ids);
}

#[test]
fn face_graph_mints_separate_components_for_disconnected_faces() {
    let mut rows = [row(1, 1), row(2, 2)];
    rows[0].faces = [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)];
    rows[1].faces = [std::num::NonZeroU32::new(3), std::num::NonZeroU32::new(4)];
    let components =
        crate::decode::with_test_decode_ctx(|ctx| super::super::face_components(ctx, &rows))
            .expect("graph admission");
    assert_eq!(components.len(), 2);
    assert_eq!(components[0].face_ids(), [1, 2]);
    assert_eq!(components[0].curve_ids(), [1]);
    assert_eq!(components[1].face_ids(), [3, 4]);
    assert_eq!(components[1].curve_ids(), [2]);
}

#[test]
fn vertex_graph_mints_separate_orbits_for_unrelated_starts() {
    let ids = [
        super::super::HalfEdgeId {
            curve_id: 1,
            side: super::super::Side::Zero,
        },
        super::super::HalfEdgeId {
            curve_id: 2,
            side: super::super::Side::Zero,
        },
    ];
    let edges = ids.map(|id| super::super::HalfEdge {
        id,
        face_id: None,
        next: None,
    });
    let orbits =
        crate::decode::with_test_decode_ctx(|ctx| super::super::vertex_orbits(ctx, &edges))
            .expect("orbit admission");
    assert_eq!(orbits.vertices.len(), 2);
    assert_eq!(orbits.vertices[0].half_edges(), [ids[0]]);
    assert_eq!(orbits.vertices[1].half_edges(), [ids[1]]);
}

#[test]
fn vertex_orbits_use_dense_components_without_adjacency_trees() {
    use super::super::{HalfEdge, HalfEdgeId, Side};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let mut edges = Vec::new();
    for curve_id in 1..=1000 {
        for side in [Side::Zero, Side::One] {
            let id = HalfEdgeId { curve_id, side };
            edges.push(HalfEdge {
                id,
                face_id: None,
                next: Some(id),
            });
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 40_000_000;
    policy.limits.max_collection_items = 25_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let orbits = super::super::vertex_orbits(&ctx, &edges).expect("component work is admitted");
    assert_eq!(orbits.vertices.len(), 1000);
    assert_eq!(orbits.incidence.len(), 2000);
    assert!(orbits.unstatable_orbits.is_empty());
    for (vertex, pair) in orbits.vertices.iter().zip(edges.chunks_exact(2)) {
        assert_eq!(vertex.half_edges(), [pair[0].id, pair[1].id]);
        assert_eq!(vertex.id.get(), pair[0].id.curve_id);
    }
    for binding in &orbits.incidence {
        assert_eq!(binding.start_vertex_id.get(), binding.half_edge.curve_id);
        assert_eq!(binding.end_vertex_id, Some(binding.start_vertex_id));
    }
}

#[test]
fn open_topology_membership_charges_tree_lookups_without_set_scans() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    let rows: Vec<_> = (1..=10_000)
        .map(|id| {
            let mut row = row(id, 0);
            row.faces = [
                std::num::NonZeroU32::new(id),
                std::num::NonZeroU32::new(id + 10_000),
            ];
            row
        })
        .collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 350_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let (edges, loops) = super::super::build(&ctx, &rows).expect("indexed membership is admitted");
    assert_eq!(edges.len(), 20_000);
    assert!(edges.iter().all(|edge| edge.next.is_none()));
    assert!(loops.is_empty());
}
