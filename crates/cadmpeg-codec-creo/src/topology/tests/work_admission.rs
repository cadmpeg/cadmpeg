// SPDX-License-Identifier: Apache-2.0
use super::row;

#[test]
fn faceless_rows_form_no_face_components() {
    let mut rows = [row(1, 2), row(2, 1)];
    for row in &mut rows {
        row.faces = [None, None];
    }
    let components = super::super::face_components(&rows).expect("face components");
    assert!(components.is_empty());
}

#[test]
fn face_component_graph_joins_two_rows_into_one_component() {
    let rows = [row(1, 2), row(2, 1)];
    let components = super::super::face_components(&rows).expect("face components");
    assert_eq!(components.len(), 1);
}

#[test]
fn vertex_orbits_join_both_sides_of_a_self_successor() {
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
    let orbits = super::super::vertex_orbits(&edges).expect("vertex orbits");
    assert_eq!(orbits.vertices.len(), 1);
    assert_eq!(orbits.vertices[0].half_edges(), ids);
}

#[test]
fn face_graph_mints_separate_components_for_disconnected_faces() {
    let mut rows = [row(1, 1), row(2, 2)];
    rows[0].faces = [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)];
    rows[1].faces = [std::num::NonZeroU32::new(3), std::num::NonZeroU32::new(4)];
    let components = super::super::face_components(&rows).expect("graph admission");
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
    let orbits = super::super::vertex_orbits(&edges).expect("orbit admission");
    assert_eq!(orbits.vertices.len(), 2);
    assert_eq!(orbits.vertices[0].half_edges(), [ids[0]]);
    assert_eq!(orbits.vertices[1].half_edges(), [ids[1]]);
}

#[test]
fn vertex_orbits_use_dense_components_without_adjacency_trees() {
    use super::super::{HalfEdge, HalfEdgeId, Side};

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
    let orbits = super::super::vertex_orbits(&edges).expect("component work is admitted");
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
fn open_topology_membership_keeps_ten_thousand_rows_open() {
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
    let (edges, loops) = super::super::build(&rows).expect("indexed membership is admitted");
    assert_eq!(edges.len(), 20_000);
    assert!(edges.iter().all(|edge| edge.next.is_none()));
    assert!(loops.is_empty());
}
