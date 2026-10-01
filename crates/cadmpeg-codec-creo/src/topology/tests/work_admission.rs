// SPDX-License-Identifier: Apache-2.0
use super::row;

#[test]
fn faceless_rows_refuse_identity_and_graph_work() {
    let mut rows = [row(1, 2), row(2, 1)];
    for row in &mut rows { row.faces = [None, None]; }
    let components = crate::test_support::assert_work_boundaries(&["creo unique-row identity work", "creo face graph assembly"], |ctx| super::super::face_components(ctx, &rows));
    assert!(components.is_empty());
}

#[test]
fn face_component_graph_refuses_assembly_and_traversal_work() {
    let rows = [row(1, 2), row(2, 1)];
    let components = crate::test_support::assert_work_boundaries(&["creo face graph assembly", "creo face graph seeds", "creo face graph traversal", "creo face graph curve memberships", "creo face graph neighbours", "creo face component projection"], |ctx| super::super::face_components(ctx, &rows));
    assert_eq!(components.len(), 1);
}

#[test]
fn vertex_orbits_refuse_before_graph_assembly_and_at_traversal() {
    let ids = [super::super::HalfEdgeId { curve_id: 1, side: super::super::Side::Zero }, super::super::HalfEdgeId { curve_id: 1, side: super::super::Side::One }];
    let edges = ids.map(|id| super::super::HalfEdge { id, face_id: None, next: Some(id) });
    let orbits = crate::test_support::assert_work_boundaries(&["creo vertex graph assembly", "creo vertex graph adjacency", "creo vertex graph seeds", "creo vertex graph traversal", "creo vertex graph neighbours", "creo vertex orbit projection", "creo vertex graph start bindings"], |ctx| super::super::vertex_orbits(ctx, &edges));
    assert_eq!(orbits.vertices.len(), 1);
    assert_eq!(orbits.vertices[0].half_edges(), ids);
}
