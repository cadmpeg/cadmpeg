use crate::families::standard::topology::reconstruct_incidence;
use crate::families::standard::topology::Boundary;
use crate::families::standard::topology::CoedgeUse;
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::EdgeRow;
use crate::families::standard::topology::FaceTopology;
use crate::families::standard::topology::StandardTopology;
use cadmpeg_ir::topology::BodyKind;

#[test]
fn radial_orientation_solves_each_face_boundary_independently() {
    let rows = (0..18)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge * 2, edge * 2 + 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        })
        .collect();
    let points = (0..12).map(|point| [f64::from(point), 0.0, 0.0]).collect();
    let edge_faces = [
        [8, 2],
        [8, 3],
        [4, 0],
        [7, 0],
        [4, 1],
        [7, 1],
        [2, 4],
        [3, 4],
        [7, 6],
        [7, 5],
        [8, 6],
        [8, 5],
        [1, 0],
        [1, 0],
        [3, 2],
        [3, 2],
        [6, 5],
        [6, 5],
    ];
    let edge_points = [
        [0, 1],
        [0, 1],
        [2, 4],
        [3, 5],
        [2, 4],
        [3, 5],
        [6, 7],
        [6, 7],
        [8, 9],
        [8, 9],
        [10, 11],
        [10, 11],
        [2, 3],
        [4, 5],
        [0, 6],
        [1, 7],
        [8, 10],
        [9, 11],
    ];
    let topology = reconstruct_incidence(rows, points, &edge_faces, &edge_points, 9)
        .expect("orientable multi-boundary shell");
    assert_eq!(topology.body_kinds(&[9]), Some(vec![BodyKind::Solid]));
    assert_eq!(topology.body_kinds(&[4, 5]), None);
    assert_eq!(topology.faces()[4].boundaries.len(), 2);
    let mut uses = vec![Vec::new(); 18];
    for face in topology.faces() {
        for boundary in &face.boundaries {
            for coedge in &boundary.coedges {
                uses[coedge.edge_row].push(coedge.reversed);
            }
        }
    }
    assert!(uses
        .iter()
        .all(|senses| senses == &[false, true] || senses == &[true, false]));
}

#[test]
fn open_standard_edge_incidence_classifies_a_sheet_body() {
    let mut topology = StandardTopology {
        faces: vec![FaceTopology {
            boundaries: vec![Boundary::new(vec![CoedgeUse {
                edge_row: 0,
                reversed: false,
                start_vertex: 0,
                end_vertex: 1,
            }])
            .expect("nonempty topology boundary")],
        }],
        edge_rows: vec![
            EdgeRow {
                kind: 1,
                handles: vec![0, 1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
            EdgeRow {
                kind: 1,
                handles: vec![2, 3],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            },
        ],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };

    assert_eq!(topology.body_kinds(&[1]), None);
    topology.edge_rows.pop();
    assert_eq!(topology.body_kinds(&[1]), Some(vec![BodyKind::Sheet]));
}

#[test]
fn solid_body_cycles_orient_independently_from_an_open_sheet_body() {
    let use_ = |edge_row| CoedgeUse {
        edge_row,
        reversed: false,
        start_vertex: edge_row,
        end_vertex: 1 - edge_row,
    };
    let mut topology = StandardTopology {
        faces: vec![
            FaceTopology {
                boundaries: vec![
                    Boundary::new(vec![use_(0), use_(1)]).expect("nonempty topology boundary")
                ],
            },
            FaceTopology {
                boundaries: vec![
                    Boundary::new(vec![use_(0), use_(1)]).expect("nonempty topology boundary")
                ],
            },
            FaceTopology {
                boundaries: vec![Boundary::new(vec![CoedgeUse {
                    edge_row: 2,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }])
                .expect("nonempty topology boundary")],
            },
        ],
        edge_rows: (0..3)
            .map(|edge| EdgeRow {
                kind: 1,
                handles: vec![edge * 2, edge * 2 + 1],
                boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
            })
            .collect(),
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
        logical_vertex_count: 2,
    };

    assert_eq!(
        topology.body_kinds(&[2, 1]),
        Some(vec![BodyKind::Solid, BodyKind::Sheet])
    );
    assert_eq!(topology.body_kinds(&[3]), Some(vec![BodyKind::General]));
    assert_eq!(topology.face_components(), vec![vec![0, 1], vec![2]]);
    topology
        .orient_solid_body_cycles(&[2, 1])
        .expect("closed group orientation");

    for edge in 0..2 {
        assert_ne!(
            topology.faces[0].boundaries[0].coedges[edge].reversed,
            topology.faces[1].boundaries[0].coedges[1 - edge].reversed,
        );
    }
    assert!(!topology.faces[2].boundaries[0].coedges[0].reversed);
}
