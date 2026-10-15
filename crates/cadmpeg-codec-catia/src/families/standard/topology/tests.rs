use super::{incidence_cycles, solve_boundary_orientation_constraints, StandardTopologyDraft};
use std::collections::HashMap;

fn duplicate_face_slot_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

    let rows = (0..3)
        .map(|handle| {
            assert!(
                EdgeRow::new(0, vec![handle], EdgeBoundaryLayout::CompleteBoundaryRun).is_none()
            );
            EdgeRow::new(
                1,
                vec![handle, handle],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    complete_duplicate_face_slots(
        ctx,
        &rows,
        &[[0, 1], [0, 1], [0, 0]],
        &[[0, 1], [1, 2], [2, 0]],
        2,
        None,
        None,
    )
}

#[test]
fn standard_duplicate_search_refuses_recursion_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture fits input limit");
    assert!(matches!(
        duplicate_face_slot_fixture(&ctx),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RecursionDepth
                && limit.operation == "catia_standard_duplicate_face_search"
    ));
}

#[test]
fn body_kinds_rejects_an_overflowing_face_group_sum() {
    let topology = StandardTopologyDraft {
        faces: Vec::new(),
        edge_rows: Vec::new(),
        vertex_points: Vec::new(),
        logical_vertex_count: 0,
    };

    assert_eq!(
        topology
            .body_kinds(&[usize::MAX, 1])
            .expect("compute the body kinds"),
        None
    );
}

#[test]
fn incidence_cycles_rejects_duplicate_and_out_of_range_edges() {
    assert!(incidence_cycles(&[0, 0], &[[0, 0]])
        .expect("compute the incidence cycles")
        .is_none());
    assert!(incidence_cycles(&[1], &[[0, 0]])
        .expect("compute the incidence cycles")
        .is_none());
}

#[test]
fn boundary_orientation_constraints_retain_the_flip_assignment() {
    let edge_uses = HashMap::from([
        (0, vec![(0, false), (1, false)]),
        (1, vec![(1, false), (2, true)]),
    ]);

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            solve_boundary_orientation_constraints(ctx, 4, &edge_uses, false)
        })
        .expect("solve boundary orientation constraints"),
        Some(vec![false, true, true, false])
    );
}

#[test]
fn edge_row_admission_couples_table_kind_and_boundary_shape() {
    use super::{EdgeBoundaryLayout, EdgeRow};
    for kind in [0, 3, 0xff] {
        assert!(EdgeRow::new(kind, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
    }
    for handles in [Vec::new(), vec![0]] {
        assert!(EdgeRow::new(1, handles, EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
    }
    assert!(EdgeRow::new(
        1,
        vec![0, 1],
        EdgeBoundaryLayout::InteriorWithFlankingCorners
    )
    .is_none());
    for kind in [1, 2] {
        let row = EdgeRow::new(
            kind,
            vec![0, 1, 2],
            EdgeBoundaryLayout::InteriorWithFlankingCorners,
        )
        .expect("interior handle");
        assert_eq!(row.kind(), kind);
        assert_eq!(row.boundary_pattern(), Some(&[1][..]));
    }
    let mut complete =
        EdgeRow::new(1, vec![0, 1], EdgeBoundaryLayout::CompleteBoundaryRun).expect("complete run");
    assert!(!complete.select_flanking_corners());
    assert_eq!(
        complete.boundary_layout(),
        EdgeBoundaryLayout::CompleteBoundaryRun
    );
}

#[test]
fn duplicate_face_sort_assigns_free_face_slots() {
    assert_eq!(
        crate::test_support::with_service_context(duplicate_face_slot_fixture)
            .expect("service topology admission"),
        Some(vec![[0, 1], [0, 1], [0, 1]])
    );
}

fn ambiguous_duplicate_face_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_classes: Option<&[usize]>,
    mesh_bytes: Option<&[u8]>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

    let rows = (0..4)
        .map(|handle| {
            EdgeRow::new(
                1,
                vec![handle, handle],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    complete_duplicate_face_slots(
        ctx,
        &rows,
        &[[0, 1], [0, 1], [2, 2], [2, 2]],
        &[[0, 1], [1, 2], [2, 0], [0, 2]],
        3,
        edge_classes,
        mesh_bytes,
    )
}

#[test]
fn ambiguous_duplicate_faces_stay_unassigned_with_an_empty_mesh() {
    assert!(crate::test_support::with_service_context(|ctx| {
        ambiguous_duplicate_face_fixture(ctx, Some(&[0, 1, 2, 3]), Some(&[]))
    })
    .expect("assign the duplicate faces")
    .is_none());
}

fn two_face_topology() -> StandardTopologyDraft {
    use super::{BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopologyDraft};

    let face = || FaceTopologyDraft {
        boundaries: vec![BoundaryDraft::new(vec![CoedgeUse {
            edge_row: 0,
            reversed: false,
            start_vertex: 0,
            end_vertex: 1,
        }])
        .expect("nonempty boundary")],
    };
    StandardTopologyDraft {
        faces: vec![face(), face()],
        edge_rows: (0..2)
            .map(|edge| {
                EdgeRow::new(1, vec![edge, edge], EdgeBoundaryLayout::CompleteBoundaryRun)
                    .expect("admitted edge row")
            })
            .collect(),
        vertex_points: Vec::new(),
        logical_vertex_count: 2,
    }
}

#[test]
fn body_kinds_reject_groups_that_leave_an_edge_row_unused() {
    assert!(two_face_topology()
        .body_kinds(&[2])
        .expect("body kinds")
        .is_none());
}

#[test]
fn faces_sharing_an_edge_form_one_component() {
    assert_eq!(
        two_face_topology()
            .face_components()
            .expect("face components"),
        vec![vec![0, 1]]
    );
}

#[test]
fn native_edge_vertices_bind_only_with_one_pair_per_edge_row() {
    use super::{BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopologyDraft};
    use cadmpeg_ir::features::NonEmptyMembers;
    let topology = StandardTopologyDraft {
        faces: vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft {
                coedges: NonEmptyMembers::<CoedgeUse>::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }],
        edge_rows: vec![{
            assert!(EdgeRow::new(1, vec![7], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![7, 7], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        }],
        vertex_points: Vec::new(),
        logical_vertex_count: 2,
    };
    let bound = topology
        .with_native_edge_vertices(&[[4, 5]])
        .expect("native edge identities bind");
    assert_eq!(bound.logical_vertex_count, 2);
    assert_eq!(bound.faces[0].boundaries[0].coedges[0].start_vertex, 0);
    assert_eq!(bound.faces[0].boundaries[0].coedges[0].end_vertex, 1);
    assert!(topology.with_native_edge_vertices(&[]).is_none());
}

#[test]
fn duplicate_face_assignments_with_different_slots_are_not_equivalent() {
    use super::{duplicate_face_assignments_equivalent, EdgeBoundaryLayout, EdgeRow};

    let rows = [
        {
            assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        },
        {
            assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        },
    ];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        duplicate_face_assignments_equivalent(
            ctx,
            &[0, 1],
            &rows,
            &[[0, 0], [0, 0]],
            &[[0, 1], [0, 1]],
            None,
            [&[0, 0], &[0, 1]],
        )
    };
    assert!(!crate::test_support::with_service_context(run)
        .expect("check duplicate face assignments equivalent"));
}

#[test]
fn duplicate_face_slots_keep_an_already_complete_assignment() {
    use super::{complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow};

    let rows = [{
        assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    }];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        complete_duplicate_face_slots(ctx, &rows, &[[0, 1]], &[[0, 1]], 2, None, None)
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("complete duplicate face slots"),
        Some(vec![[0, 1]])
    );
}

#[test]
fn incidence_reconstructs_a_closed_edge_shared_by_two_faces() {
    use super::{reconstruct_incidence, EdgeBoundaryLayout, EdgeRow};

    let rows = || {
        vec![{
            assert!(EdgeRow::new(0, vec![7, 7], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![7, 7], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        }]
    };
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        reconstruct_incidence(ctx, rows(), vec![[1.0, 0.0, 0.0]], &[[0, 1]], &[[0, 0]], 2)
    };
    assert!(crate::test_support::with_service_context(run)
        .expect("reconstruct incidence")
        .is_some());
}

#[test]
fn edge_vertices_follow_the_coedge_direction() {
    use super::{BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopologyDraft};
    use cadmpeg_core::decode::DecodeArena;
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::DecodePolicy;

    let topology = StandardTopologyDraft {
        faces: vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft::new(vec![CoedgeUse {
                edge_row: 0,
                reversed: false,
                start_vertex: 0,
                end_vertex: 1,
            }])
            .expect("nonempty boundary")],
        }],
        edge_rows: vec![{
            assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
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
            .expect("compute the edge vertices"),
        Some(vec![[0, 1]])
    );
}

#[test]
fn edge_vertices_reject_an_edge_row_without_a_coedge() {
    use super::{BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopologyDraft};
    use cadmpeg_ir::features::NonEmptyMembers;

    let topology = StandardTopologyDraft {
        faces: vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft {
                coedges: NonEmptyMembers::<CoedgeUse>::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }],
        edge_rows: vec![
            {
                assert!(
                    EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none()
                );
                EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                    .expect("admitted edge row")
            },
            {
                assert!(
                    EdgeRow::new(1, vec![11], EdgeBoundaryLayout::CompleteBoundaryRun).is_none()
                );
                EdgeRow::new(1, vec![11, 11], EdgeBoundaryLayout::CompleteBoundaryRun)
                    .expect("admitted edge row")
            },
        ],
        vertex_points: Vec::new(),
        logical_vertex_count: 2,
    };
    assert!(
        crate::test_support::with_service_context(|ctx| topology.edge_vertices(ctx))
            .expect("compute the edge vertices")
            .is_none()
    );
}

#[test]
fn vertex_points_bind_by_endpoint_pairs_and_reject_out_of_range_points() {
    use super::{BoundaryDraft, CoedgeUse, EdgeBoundaryLayout, EdgeRow, FaceTopologyDraft};
    use cadmpeg_core::decode::DecodeArena;
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::DecodePolicy;

    let topology = StandardTopologyDraft {
        faces: vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft::new(vec![
                CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                },
                CoedgeUse {
                    edge_row: 1,
                    reversed: false,
                    start_vertex: 1,
                    end_vertex: 2,
                },
            ])
            .expect("nonempty boundary")],
        }],
        edge_rows: vec![
            {
                assert!(
                    EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none()
                );
                EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
                    .expect("admitted edge row")
            },
            {
                assert!(
                    EdgeRow::new(1, vec![11], EdgeBoundaryLayout::CompleteBoundaryRun).is_none()
                );
                EdgeRow::new(1, vec![11, 11], EdgeBoundaryLayout::CompleteBoundaryRun)
                    .expect("admitted edge row")
            },
        ],
        vertex_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
        logical_vertex_count: 3,
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    assert_eq!(
        topology
            .bind_vertex_points(&ctx, &[[0, 1], [1, 2]])
            .expect("bind vertex points"),
        Some(vec![0, 1, 2])
    );
    assert!(topology
        .bind_vertex_points(&ctx, &[[3, 1], [1, 2]])
        .expect("bind vertex points")
        .is_none());
}

#[test]
fn mesh_selection_rejects_an_assignment_naming_a_missing_edge_row() {
    use super::{reconstruct_mesh_selection, EdgeBoundaryLayout, EdgeRow};
    use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};

    let rows = vec![{
        assert!(EdgeRow::new(0, vec![0], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    }];
    let assignment_for = |edge| MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    };
    let directions = [vec![vec![false]]];
    let points = [[0.0, 0.0, 0.0]];

    assert!(
        reconstruct_mesh_selection(&rows, &points, &[assignment_for(1)], &directions)
            .expect("reconstruct mesh selection")
            .is_none()
    );
    assert!(
        reconstruct_mesh_selection(&rows, &points, &[assignment_for(0)], &directions)
            .expect("reconstruct mesh selection")
            .is_some()
    );
}

#[test]
fn incidence_cycles_close_two_opposed_edges_and_reject_repeats() {
    let points = [[0, 1], [1, 0]];
    assert!(incidence_cycles(&[0, 1], &points)
        .expect("compute the incidence cycles")
        .is_some());
    assert!(incidence_cycles(&[0, 0], &points)
        .expect("compute the incidence cycles")
        .is_none());
}

#[test]
fn empty_mesh_selection_reconstructs_with_rows_or_points_alone() {
    use super::{reconstruct_mesh_selection, EdgeBoundaryLayout, EdgeRow};
    use crate::solve::missing_edge::MeshFaceBoundaryAssignment;

    let row = {
        assert!(EdgeRow::new(1, vec![7], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![7, 7], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    };
    let selected: [MeshFaceBoundaryAssignment; 0] = [];
    assert!(reconstruct_mesh_selection(&[row], &[], &selected, &[])
        .expect("reconstruct mesh selection")
        .is_some());
    assert!(
        reconstruct_mesh_selection(&[], &[[0.0, 0.0, 0.0]], &selected, &[])
            .expect("reconstruct mesh selection")
            .is_some()
    );
}

#[test]
fn set_duplicate_degree_inserts_a_missing_point() {
    use super::set_duplicate_degree;

    let mut row = Vec::new();
    set_duplicate_degree(&mut row, 7, 1);
    assert_eq!(row, [(7, 1)]);
}

#[test]
fn face_orientation_rejects_an_unpaired_edge_use() {
    use super::{orient_face_cycles, BoundaryDraft, CoedgeUse, FaceTopologyDraft};
    use cadmpeg_ir::features::NonEmptyMembers;
    let faces = || {
        vec![FaceTopologyDraft {
            boundaries: vec![BoundaryDraft {
                coedges: NonEmptyMembers::<CoedgeUse>::one(CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }),
            }],
        }]
    };
    assert!(crate::test_support::with_service_context(|ctx| {
        orient_face_cycles(ctx, &mut faces())
    })
    .expect("orient face cycles")
    .is_none());
}
