use crate::families::standard::topology::reconstruct_incidence;
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::EdgeRow;

#[test]
fn endpoint_incidence_builds_oriented_tetrahedron_cycles() {
    let rows: Vec<_> = (0..6)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge * 2, edge * 2 + 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        })
        .collect();
    let points = vec![
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let edge_faces = [[0, 1], [0, 2], [0, 3], [1, 3], [1, 2], [2, 3]];
    let edge_points = [[0, 1], [1, 2], [2, 0], [0, 3], [3, 1], [2, 3]];
    let topology = reconstruct_incidence(rows, points, &edge_faces, &edge_points, 4)
        .expect("closed oriented incidence");
    assert_eq!(topology.face_count(), 4);
    assert!(topology
        .faces()
        .iter()
        .all(|face| { face.boundaries.len() == 1 && face.boundaries[0].coedges.len() == 3 }));
    let mut uses = vec![Vec::new(); 6];
    for face in topology.faces() {
        for coedge in &face.boundaries[0].coedges {
            uses[coedge.edge_row].push(coedge.reversed);
        }
    }
    assert!(uses
        .iter()
        .all(|senses| senses == &[false, true] || senses == &[true, false]));
}
