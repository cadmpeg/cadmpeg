use super::{incidence_cycles, solve_boundary_orientation_constraints, StandardTopology};
use std::collections::HashMap;

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
