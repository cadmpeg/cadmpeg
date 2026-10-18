use super::{incidence_cycles, solve_boundary_orientation_constraints, StandardTopologyDraft};
use std::collections::HashMap;

fn duplicate_face_slot_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    use super::{
        complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow, StandardIncidenceSearch,
    };

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
        StandardIncidenceSearch {
            budget: &crate::test_support::topology_budget(),
            edge_classes: None,
            mesh_bytes: None,
        },
    )
}

#[test]
fn standard_duplicate_search_past_its_recursion_guard_exhausts_only_the_topology_search() {
    use super::{
        complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow, StandardIncidenceSearch,
    };

    let edges = cadmpeg_core::decode::WorkBudget::RECURSION_DEPTH + 44;
    let rows = (0..u32::try_from(edges).expect("fixture edge count"))
        .map(|handle| {
            EdgeRow::new(
                1,
                vec![handle, handle],
                EdgeBoundaryLayout::CompleteBoundaryRun,
            )
            .expect("admitted edge row")
        })
        .collect::<Vec<_>>();
    let points = (0..edges)
        .map(|edge| [edge, (edge + 1) % edges])
        .collect::<Vec<_>>();
    let budget = crate::test_support::topology_budget();
    let completed = crate::test_support::with_service_context(|ctx| {
        complete_duplicate_face_slots(
            ctx,
            &rows,
            &vec![[0, 0]; edges],
            &points,
            2,
            StandardIncidenceSearch {
                budget: &budget,
                edge_classes: None,
                mesh_bytes: None,
            },
        )
    })
    .expect("a deep duplicate-face search is not a session refusal");
    assert_eq!(completed, None);
    assert!(budget.exhausted());
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
            .expect("classify the bodies"),
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
            .expect("complete the duplicate face slots"),
        Some(vec![[0, 1], [0, 1], [0, 1]])
    );
}

fn ambiguous_duplicate_face_fixture(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edge_classes: Option<&[usize]>,
    mesh_bytes: Option<&[u8]>,
) -> Result<Option<Vec<[usize; 2]>>, cadmpeg_core::CodecError> {
    use super::{
        complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow, StandardIncidenceSearch,
    };

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
        StandardIncidenceSearch {
            budget: &crate::test_support::topology_budget(),
            edge_classes,
            mesh_bytes,
        },
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
    use super::{
        complete_duplicate_face_slots, EdgeBoundaryLayout, EdgeRow, StandardIncidenceSearch,
    };

    let rows = [{
        assert!(EdgeRow::new(1, vec![10], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
        EdgeRow::new(1, vec![10, 10], EdgeBoundaryLayout::CompleteBoundaryRun)
            .expect("admitted edge row")
    }];
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        complete_duplicate_face_slots(
            ctx,
            &rows,
            &[[0, 1]],
            &[[0, 1]],
            2,
            StandardIncidenceSearch {
                budget: &crate::test_support::topology_budget(),
                edge_classes: None,
                mesh_bytes: None,
            },
        )
    };
    assert_eq!(
        crate::test_support::with_service_context(run).expect("complete the duplicate face slots"),
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
        reconstruct_incidence(
            ctx,
            &crate::test_support::topology_budget(),
            rows(),
            vec![[1.0, 0.0, 0.0]],
            &[[0, 1]],
            &[[0, 0]],
            2,
        )
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
            .expect("resolve the edge vertices"),
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
            .expect("resolve the edge vertices")
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

/// Vertex-map walk over `incident`: the specification `incidence_cycles`
/// must reproduce, cycle for cycle and member for member.
fn incidence_cycles_by_vertex_maps(
    incident: &[usize],
    edge_points: &[[usize; 2]],
) -> Option<Vec<Vec<(usize, bool)>>> {
    use std::collections::HashSet;

    #[derive(Clone, Copy)]
    struct AdjacentEdge {
        edge: usize,
        vertex: usize,
        reversed: bool,
    }

    if incident.is_empty() {
        return None;
    }
    let mut vertex_indices = HashMap::<usize, usize>::new();
    let mut at_vertex = Vec::<Vec<AdjacentEdge>>::new();
    let mut unseen = Vec::<(usize, [usize; 2])>::new();
    let mut seen_edges = HashSet::new();
    let mut cycles = Vec::new();
    for &edge in incident {
        if !seen_edges.insert(edge) {
            return None;
        }
        let &[start, end] = edge_points.get(edge)?;
        if start == end {
            cycles.push(vec![(edge, false)]);
            continue;
        }
        let mut endpoints = [0; 2];
        for (slot, vertex) in [start, end].into_iter().enumerate() {
            let index = if let Some(&index) = vertex_indices.get(&vertex) {
                index
            } else {
                let index = at_vertex.len();
                vertex_indices.insert(vertex, index);
                at_vertex.push(Vec::new());
                index
            };
            endpoints[slot] = index;
        }
        let [start, end] = endpoints;
        at_vertex[start].push(AdjacentEdge {
            edge,
            vertex: end,
            reversed: false,
        });
        at_vertex[end].push(AdjacentEdge {
            edge,
            vertex: start,
            reversed: true,
        });
        unseen.push((edge, [start, end]));
    }
    if at_vertex.iter().any(|edges| edges.len() != 2) {
        return None;
    }
    unseen.sort_unstable_by_key(|left| std::cmp::Reverse(left.0));
    let mut consumed = HashSet::new();
    while let Some((first, [start_vertex, mut vertex])) = unseen.pop() {
        if !consumed.insert(first) {
            continue;
        }
        let mut members = vec![(first, false)];
        let mut previous_edge = first;
        while vertex != start_vertex {
            let [left, right] = at_vertex[vertex].as_slice() else {
                return None;
            };
            let next = if left.edge == previous_edge {
                right
            } else {
                left
            };
            if !consumed.insert(next.edge) {
                return None;
            }
            vertex = next.vertex;
            previous_edge = next.edge;
            members.push((next.edge, next.reversed));
        }
        cycles.push(members);
    }
    Some(cycles)
}

#[test]
fn incidence_cycles_match_the_vertex_map_walk() {
    struct SplitMix(u64);
    impl SplitMix {
        fn below(&mut self, bound: usize) -> usize {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut value = self.0;
            value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^= value >> 31;
            usize::try_from(value % u64::try_from(bound).expect("bound fits u64"))
                .expect("value below bound fits usize")
        }
    }

    let mut random = SplitMix(0x5eed);
    let mut closed = 0usize;
    for case in 0..20_000 {
        let mut edge_points = Vec::new();
        let mut incident = Vec::new();
        if case % 2 == 0 {
            let point_count = 1 + random.below(6);
            for _ in 0..random.below(9) {
                edge_points.push([random.below(point_count), random.below(point_count)]);
            }
            for _ in 0..random.below(9) {
                incident.push(random.below(edge_points.len() + 2));
            }
        } else {
            // A disjoint union of cycles over shuffled vertex labels, some
            // edges reversed, then incident in shuffled order.
            let mut next_point = 0usize;
            for _ in 0..=random.below(3) {
                let length = 1 + random.below(5);
                let first = next_point;
                next_point += length;
                for offset in 0..length {
                    let start = first + offset;
                    let end = first + (offset + 1) % length;
                    edge_points.push(if random.below(3) == 0 {
                        [end, start]
                    } else {
                        [start, end]
                    });
                }
            }
            for index in (1..next_point).rev() {
                let other = random.below(index + 1);
                for points in &mut edge_points {
                    for point in points {
                        if *point == index {
                            *point = other;
                        } else if *point == other {
                            *point = index;
                        }
                    }
                }
            }
            incident.extend(0..edge_points.len());
            for index in (1..incident.len()).rev() {
                incident.swap(index, random.below(index + 1));
            }
            match random.below(8) {
                0 => incident.truncate(random.below(incident.len() + 1)),
                1 => incident.push(incident[random.below(incident.len())]),
                _ => {}
            }
        }
        let expected = incidence_cycles_by_vertex_maps(&incident, &edge_points);
        let actual = incidence_cycles(&incident, &edge_points)
            .expect("compute the incidence cycles")
            .map(|cycles| {
                cycles
                    .iter()
                    .map(|cycle| cycle.to_vec())
                    .collect::<Vec<_>>()
            });
        closed += usize::from(expected.is_some());
        assert_eq!(
            actual, expected,
            "incident {incident:?} over {edge_points:?}"
        );
    }
    assert!(closed > 5_000, "only {closed} cases closed");
}
