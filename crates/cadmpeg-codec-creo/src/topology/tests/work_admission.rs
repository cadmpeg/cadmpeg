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
            "creo vertex graph neighbours",
            "creo vertex orbit projection",
            "creo vertex graph start bindings",
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
