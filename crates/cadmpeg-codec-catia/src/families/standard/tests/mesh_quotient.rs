use super::*;

#[test]
fn quotient_assignments_ignore_span_allocation_with_identical_edge_order() {
    let use_ = |edge, start, end| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end,
        reversed: None,
    };
    let mut faces = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(0, 0, 1),
                use_(1, 1, 2),
                use_(2, 2, 3),
                use_(3, 3, 4),
            ]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(0, 0, 2),
                use_(1, 2, 3),
                use_(2, 3, 4),
                use_(3, 4, 5),
            ]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(3, 0, 1),
                use_(2, 1, 2),
                use_(1, 2, 3),
                use_(0, 3, 4),
            ]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                use_(0, 0, 1),
                use_(2, 1, 2),
                use_(1, 2, 3),
                use_(3, 3, 4),
            ]],
        },
    ]];
    deduplicate_mesh_quotient_assignments(&mut faces);
    assert_eq!(faces[0].len(), 2);
    assert_eq!(faces[0][0].boundaries[0][0].edge, 0);
    assert_eq!(faces[0][1].boundaries[0][1].edge, 2);
}

#[test]
fn mesh_option_enumeration_does_not_scan_fixed_direction_gauges() {
    const EDGE_COUNT: usize = 10;
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..EDGE_COUNT)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: edge,
                end: (edge + 1) % EDGE_COUNT,
                reversed: None,
            })
            .collect()],
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(EDGE_COUNT * 2),
        domains: repeated_domain(HashSet::from([0]), EDGE_COUNT * 2),
        members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
    };
    let candidates = vec![vec![[0, 0]]; EDGE_COUNT];
    let budget = WorkBudget::new(30);

    let options = quotient.assignment_options_limited(
        &assignment,
        &candidates,
        &HashSet::new(),
        2,
        Some(&budget),
    );

    assert_eq!(options.len(), 1);
    assert_eq!(options[0].0, vec![vec![false; EDGE_COUNT]]);
    assert!(!budget.exhausted());
}

#[test]
fn mesh_option_enumeration_preserves_asymmetric_endpoint_directions() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: None,
            },
        ]],
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0], vec![1], vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    let options = quotient.assignment_options_limited(
        &assignment,
        &[vec![[0, 1]], vec![[0, 1]]],
        &HashSet::new(),
        4,
        None,
    );

    assert!(options
        .iter()
        .any(|(directions, _)| directions == &[vec![false, true]]));
}

#[test]
fn quotient_merge_preserves_physical_edge_pair_correlation() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0], vec![0, 1], vec![0], vec![2]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("nonempty port intersection");
    assert!(!quotient.edge_domains_viable(&[vec![[0, 1]], vec![[0, 2]]]));
}

#[test]
fn quotient_clones_share_unconstrained_point_domains() {
    let all = Arc::new((0..1_000).collect::<HashSet<_>>());
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![all.clone(), all.clone(), all.clone(), all.clone()],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    let clone = quotient.clone();
    assert!(Arc::ptr_eq(&quotient.domains[0], &clone.domains[0]));
    assert!(Arc::ptr_eq(&quotient.domains[0], &quotient.domains[3]));
}

#[test]
fn port_quotient_completes_only_supported_unknown_edge_pairs() {
    let candidates = [vec![[0, 1]], Vec::new(), vec![[2, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        5,
        &[[10, 11], [10, 12], [12, 13]],
    )
    .expect("initial quotient");
    let completed = crate::solve::mesh_quotient::complete_mesh_endpoint_candidates_from_quotient(
        &candidates,
        &mut quotient,
        16,
        32,
    )
    .expect("bounded quotient completion");

    assert_eq!(completed[0], vec![[0, 1]]);
    assert_eq!(completed[1], vec![[0, 2], [0, 3], [1, 2], [1, 3]]);
    assert_eq!(completed[2], vec![[2, 3]]);
}

#[test]
fn coordinate_root_fixpoint_removes_unsupported_edge_pairs() {
    let candidates = [
        vec![[0, 1], [0, 2]],
        vec![[0, 1]],
        vec![[0, 1]],
        vec![[2, 2]],
    ];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        3,
        &[[10, 11], [10, 12], [13, 11], [14, 14]],
    )
    .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(3, &candidates, None)
        .expect("coordinate root domains");

    assert_eq!(
        domains.edge_candidates(),
        &[vec![[0, 1]], vec![[0, 1]], vec![[0, 1]], vec![[2, 2]]]
    );
}

#[test]
fn selected_edge_pair_propagates_through_shared_coordinate_roots() {
    let candidates = [vec![[0, 1], [0, 2]], vec![[1, 3], [2, 3]], vec![[2, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        4,
        &[[10, 11], [11, 12], [13, 14]],
    )
    .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(4, &candidates, None)
        .expect("coordinate root domains")
        .refine_edge_candidate_arc(0, [0, 1], None)
        .expect("selected edge pair");

    assert!(domains.supports_edge_candidate(1, [1, 3]));
    assert!(!domains.supports_edge_candidate(1, [2, 3]));
}

#[test]
fn port_quotient_declines_unbounded_unknown_edge_pairs() {
    let candidates = [Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 100, &[[10, 11]])
            .expect("initial quotient");
    assert!(
        crate::solve::mesh_quotient::complete_mesh_endpoint_candidates_from_quotient(
            &candidates,
            &mut quotient,
            1_000,
            1_000,
        )
        .is_none()
    );
}

#[test]
fn coordinate_root_domains_keep_unknown_edge_pairs_implicit() {
    let candidates = [Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 2, &[[10, 11]])
            .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(2, &candidates, None)
        .expect("implicit coordinate domains");

    assert!(domains.edge_candidates()[0].is_empty());
    assert_eq!(domains.edge_candidate_points(0), Some(vec![0, 1]));
    assert_eq!(
        domains
            .implicit_edge_candidates(0, Some(0))
            .expect("implicit candidates")
            .collect::<Vec<_>>(),
        vec![[0, 1]]
    );
    assert!(domains.refine_edge_candidate_arc(0, [0, 1], None).is_some());
    assert!(domains.refine_edge_candidate_arc(0, [0, 0], None).is_none());
}

#[test]
fn required_implicit_coordinate_pairs_scale_with_root_domains_not_their_product() {
    let candidates = [Vec::new(), Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 4, &[[10, 11], [12, 13]])
            .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(4, &candidates, None)
        .expect("implicit coordinate domains");
    let implicit = domains
        .implicit_edge_candidates(0, Some(1))
        .expect("required implicit candidates");

    assert_eq!(implicit.width_upper_bound(), 3);
    assert_eq!(implicit.collect::<Vec<_>>(), vec![[0, 1], [1, 2], [1, 3]]);

    let mut visited = Vec::new();
    assert_eq!(
        domains.any_implicit_edge_candidate_with_point(0, 1, None, |pair| {
            visited.push(pair);
            pair == [1, 3]
        }),
        Some(true)
    );
    assert_eq!(visited, vec![[0, 1], [1, 2], [1, 3]]);

    let budget = WorkBudget::new(2);
    assert_eq!(
        domains.any_implicit_edge_candidate_with_point(0, 1, Some(&budget), |_| false),
        None
    );
    assert!(budget.exhausted());
}

#[test]
fn coordinate_domain_preparation_scales_with_constraint_graph_work() {
    let candidates = vec![Vec::new(); 100];
    let ports = (0..100)
        .map(|edge| [(edge * 2) as u32, (edge * 2 + 1) as u32])
        .collect::<Vec<_>>();
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 200, &ports)
        .expect("initial quotient");

    assert!(
        quotient
            .coordinate_domain_preparation_limit(200, &candidates)
            .expect("preparation limit")
            > crate::solve::mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS
    );
}

#[test]
fn incidence_search_consumes_implicit_coordinate_root_pairs() {
    use crate::solve::incidence::{component_incidence_pair_solution_outcome, IncidenceSolve};

    let candidates = vec![Vec::new(); 3];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        3,
        &[[10, 11], [11, 12], [12, 10]],
    )
    .expect("cycle quotient");
    let outcome = component_incidence_pair_solution_outcome(
        &candidates,
        &[[0, 0]; 3],
        1,
        3,
        None,
        Some(&quotient),
        None,
        &|_| true,
    );

    assert!(matches!(
        outcome,
        IncidenceSolve::Solved(_) | IncidenceSolve::Ambiguous
    ));
}

#[test]
fn ordered_face_equations_narrow_unknown_edge_roots_before_pair_completion() {
    let edge_candidates = vec![vec![[0, 1]], Vec::new(), vec![[0, 2]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &edge_candidates,
        3,
        &[[10, 11], [12, 13], [14, 15]],
    )
    .expect("initial quotient");
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![(0..3)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                })
                .collect()],
        },
    ])];
    let budget = WorkBudget::new(10_000);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &edge_candidates,
        &mut quotient,
        &budget,
    )
    .expect("common face equations");
    let completed = crate::solve::mesh_quotient::complete_mesh_endpoint_candidates_from_quotient(
        &edge_candidates,
        &mut quotient,
        16,
        32,
    )
    .expect("completed edge domain");

    assert_eq!(completed[1], vec![[1, 2]]);
}

#[test]
fn quotient_pair_domains_propagate_through_shared_components() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1], vec![2], vec![0, 1], vec![3, 4]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let root = quotient.merge(0, 2).expect("shared endpoint component");

    assert!(quotient.edge_domains_viable(&[vec![[0, 2]], vec![[0, 3], [1, 4]],]));
    assert_eq!(*quotient.domains[root], HashSet::from([0]));
    assert_eq!(
        *quotient.domains[quotient.union.find(3)],
        HashSet::from([3])
    );
}

#[test]
fn quotient_assignment_requires_one_consistent_closed_orientation() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0], vec![1], vec![2], vec![3]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 2,
                reversed: None,
            },
        ]],
    };
    assert!(!quotient.assignment_has_option(&assignment, &[vec![], vec![]], None));
    Arc::make_mut(&mut quotient.domains[2]).insert(1);
    assert!(!quotient.assignment_has_option(&assignment, &[vec![], vec![]], None));
    Arc::make_mut(&mut quotient.domains[3]).insert(0);
    assert!(quotient.assignment_has_option(&assignment, &[vec![], vec![]], None));
}

#[test]
fn quotient_assignment_declines_when_its_work_budget_is_exhausted() {
    let quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let budget = WorkBudget::new(0);

    assert!(!quotient.assignment_has_option(&assignment, &[vec![[0, 0]]], Some(&budget),));
    assert!(budget.exhausted());
}

#[test]
fn face_choice_materialization_declines_when_its_work_budget_is_exhausted() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let equations = possible_face_equations(&assignments);

    assert!(possible_face_choices_with_limit(&assignments, &equations, 0).is_none());
}

#[test]
fn fixed_boundary_option_has_no_recursive_depth_limit() {
    const EDGE_COUNT: usize = 10_000;
    let quotient = MeshQuotient {
        union: UnionFind::new(EDGE_COUNT * 2),
        domains: repeated_domain(HashSet::from([0]), EDGE_COUNT * 2),
        members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..EDGE_COUNT)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: edge,
                end: (edge + 1) % EDGE_COUNT,
                reversed: Some(false),
            })
            .collect()],
    };
    let candidates = vec![vec![[0, 0]]; EDGE_COUNT];

    assert!(quotient.assignment_has_option(&assignment, &candidates, None));
}

#[test]
fn quotient_options_reject_an_interior_pair_contradiction() {
    let quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [vec![0], vec![1, 2], vec![2], vec![3], vec![0, 3], vec![0]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..6).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 2,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 2,
                start: 2,
                end: 3,
                reversed: None,
            },
        ]],
    };
    let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 3]]];

    let options = quotient.assignment_options(&assignment, &candidates);

    assert!(!options
        .iter()
        .any(|(directions, _)| directions == &[vec![false, false, false]]));
    let unrestricted = [Vec::new(), Vec::new(), Vec::new()];
    let options = quotient.assignment_options(&assignment, &unrestricted);
    let limited =
        quotient.assignment_options_limited(&assignment, &unrestricted, &HashSet::new(), 1, None);
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].0, options[0].0);
    let unique = quotient.assignment_options_limited(
        &assignment,
        &unrestricted,
        &HashSet::new(),
        4_096,
        None,
    );
    assert!(unique
        .iter()
        .all(|option| options.iter().any(|candidate| candidate.0 == option.0)));
}

#[test]
fn quotient_options_decline_when_their_work_budget_is_exhausted() {
    let quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let budget = WorkBudget::new(0);

    let options = quotient.assignment_options_limited(
        &assignment,
        &[vec![[0, 0]]],
        &HashSet::new(),
        1,
        Some(&budget),
    );

    assert!(options.is_empty());
    assert!(budget.exhausted());
}

#[test]
fn quotient_point_assignment_preserves_endpoint_pair_relations() {
    let quotient = || MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1], vec![2], vec![0, 1], vec![3]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    assert!(quotient()
        .point_assignment(4, &[vec![], vec![]], None)
        .is_none());
    assert!(quotient().point_assignment_exists(4, &[vec![], vec![]], None));

    let assignment = quotient()
        .point_assignment(4, &[vec![[0, 2]], vec![[1, 3]]], None)
        .expect("edge-pair relations determine the coordinate bijection");
    assert_eq!(assignment[&0], 0);
    assert_eq!(assignment[&1], 2);
    assert_eq!(assignment[&2], 1);
    assert_eq!(assignment[&3], 3);
}

#[test]
fn quotient_point_existence_declines_when_its_work_budget_is_exhausted() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0, 1]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(0);

    assert!(!quotient.point_assignment_exists(2, &[vec![]], Some(&budget)));
    assert!(budget.exhausted());
}

#[test]
fn point_assignment_handles_deep_augmenting_paths_iteratively() {
    const ROOT_COUNT: usize = 10_000;
    let mut domains = (0..ROOT_COUNT - 1)
        .map(|root| Arc::new(HashSet::from([root, root + 1])))
        .collect::<Vec<_>>();
    domains.push(Arc::new(HashSet::from([0])));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains,
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };

    let assignment = quotient
        .point_assignment(ROOT_COUNT, &[], None)
        .expect("forced coordinate bijection");

    assert_eq!(assignment.len(), ROOT_COUNT);
    assert_eq!(assignment[&(ROOT_COUNT - 1)], 0);
    assert!((0..ROOT_COUNT - 1).all(|root| assignment[&root] == root + 1));
}

#[test]
fn quotient_point_existence_rejects_an_all_different_conflict() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![Arc::new(HashSet::from([0])), Arc::new(HashSet::from([0]))],
        members: vec![vec![0], vec![1]],
    };

    assert!(!quotient.point_assignment_exists(2, &[vec![]], None));
}

#[test]
fn quotient_point_existence_can_become_viable_after_a_root_merge() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([2])),
        ],
        members: vec![vec![0], vec![1], vec![2], vec![3]],
    };

    assert!(!quotient.point_assignment_exists(3, &[vec![], vec![]], None));
    quotient.merge(0, 1).expect("compatible roots merge");
    assert!(quotient.point_assignment_exists(3, &[vec![], vec![]], None));
}

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
            boundaries: vec![Boundary {
                coedges: vec![CoedgeUse {
                    edge_row: 0,
                    reversed: false,
                    start_vertex: 0,
                    end_vertex: 1,
                }],
            }],
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
                boundaries: vec![Boundary {
                    coedges: vec![use_(0), use_(1)],
                }],
            },
            FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![use_(0), use_(1)],
                }],
            },
            FaceTopology {
                boundaries: vec![Boundary {
                    coedges: vec![CoedgeUse {
                        edge_row: 2,
                        reversed: false,
                        start_vertex: 0,
                        end_vertex: 1,
                    }],
                }],
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

#[test]
fn mesh_selection_rejects_an_odd_boundary_orientation_cycle() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let assignments = vec![
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(2)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1), use_(2)]],
        }],
    ];
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 3],
        edge_candidates: &[],
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![
            Some((0, vec![vec![false, false]])),
            Some((0, vec![vec![false, false]])),
            Some((0, vec![vec![false, false]])),
        ],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.selected_orientable());
    search.selected[2] = Some((0, vec![vec![false, true]]));
    assert!(search.selected_orientable());
}

#[test]
fn partial_boundary_orientation_constraints_reject_an_odd_parity_cycle() {
    let mut edge_uses = HashMap::from([
        (0, vec![(0, false), (1, false)]),
        (1, vec![(1, false), (2, false)]),
        (2, vec![(2, false), (0, false)]),
        (3, vec![(3, false)]),
    ]);

    assert!(solve_boundary_orientation_constraints(4, &edge_uses, false).is_none());
    edge_uses.get_mut(&2).expect("third paired edge")[1].1 = true;
    assert!(solve_boundary_orientation_constraints(4, &edge_uses, false).is_some());
    assert!(solve_boundary_orientation_constraints(4, &edge_uses, true).is_none());
}

#[test]
fn partial_face_orientability_rejects_an_odd_open_path_cycle() {
    let edge_faces = [[0, 1], [1, 2], [2, 0]];
    let face_edges = vec![vec![0, 2], vec![0, 1], vec![1, 2]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let complete = [Some([0, 1]), Some([1, 2]), Some([0, 2])];

    assert!(!crate::solve::incidence::partial_face_orientability_viable(
        &complete,
        &edge_faces,
        &face_edges,
        &budget,
    ));
    let partial = [Some([0, 1]), Some([1, 2]), None];
    assert!(crate::solve::incidence::partial_face_orientability_viable(
        &partial,
        &edge_faces,
        &face_edges,
        &budget,
    ));
    assert!(!budget.exhausted());
}

#[test]
fn mesh_selection_rejects_a_branch_with_no_orientable_remaining_face() {
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed,
    };
    let assignments = vec![
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, None), use_(2, None)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, None), use_(1, None)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1, Some(false)), use_(2, Some(false))]],
        }],
    ];
    let edge_candidates = vec![Vec::new(); 3];
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 3],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![
            Some((0, vec![vec![false, false]])),
            Some((0, vec![vec![false, false]])),
            None,
        ],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    assert!(!search.fixed_remaining_faces_are_orientable());
    search.selected[1] = Some((0, vec![vec![false, true]]));
    assert!(search.fixed_remaining_faces_are_orientable());
}

#[test]
fn mesh_selection_checks_all_fixed_remaining_faces_together() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: Some(false),
    };
    let assignments = vec![
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(2), use_(0)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        }],
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1), use_(2)]],
        }],
    ];
    let edge_candidates = vec![Vec::new(); 3];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 3],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![Some((0, vec![vec![false, false]])), None, None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.fixed_remaining_faces_are_orientable());
}

#[test]
fn partial_mesh_selection_survives_optional_deduction_exhaustion() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: None,
            },
        ]],
    }]];
    let edge_candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_rows = vec![
        EdgeRow {
            kind: 1,
            handles: vec![0, 1],
            boundary_layout: EdgeBoundaryLayout::InteriorWithFlankingCorners,
        };
        2
    ];
    let vertex_points = vec![[0.0; 3], [1.0, 0.0, 0.0]];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &vertex_points,
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient =
        initial_mesh_quotient(&edge_candidates, 2, &[[0, 1], [2, 3]]).expect("initial quotient");
    quotient.merge(1, 2).expect("selected face corner");
    let propagation_budget = WorkBudget::new(0);
    let changed_edges = HashSet::from([0]);

    let mut prepared = search
        .prepare_selected_branch(&quotient, &changed_edges, &propagation_budget)
        .expect("partial quotient remains viable");

    assert_eq!(prepared.root_count(), 3);
    assert!(propagation_budget.exhausted());
}

#[test]
fn mesh_assignment_distinguishes_quotient_work_from_direction_only_work() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: Some(false),
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: Some(false),
            },
        ]],
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert!(mesh_assignment_can_merge(&assignment, &mut quotient));
    quotient.merge(1, 2).expect("first boundary corner");
    quotient.merge(3, 0).expect("second boundary corner");
    assert!(!mesh_assignment_can_merge(&assignment, &mut quotient));
}

#[test]
fn remaining_merge_capacity_counts_distinct_quotient_equations() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: Some(false),
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: Some(false),
            },
        ]],
    };
    let assignments = vec![vec![assignment.clone()], vec![assignment]];
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1); 2],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None; 2],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        Some(2)
    );
    quotient.merge(1, 2).expect("first repeated equation");
    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        Some(1)
    );
}

#[test]
fn remaining_merge_capacity_respects_mutually_exclusive_orientations() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: None,
            },
        ]],
    };
    let assignments = vec![vec![assignment]];
    let equations = possible_face_equations(&assignments);
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_choices: possible_face_choices(&assignments, &equations),
        possible_face_equations: equations,
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        Some(2)
    );
}

#[test]
fn remaining_equations_must_connect_equal_singleton_domains() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([2])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        None
    );
}

#[test]
fn remaining_equation_components_require_a_coordinate_matching() {
    let assignments = Vec::new();
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: Vec::new(),
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([0, 1])),
            Arc::new(HashSet::from([2, 3])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        None
    );
}

#[test]
fn coordinate_matching_reserves_unavoidable_roots_per_component() {
    let assignments = vec![Vec::new()];
    let edge_candidates = vec![Vec::new(); 2];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: vec![vec![[0, 1], [1, 2]]],
        possible_face_choices: vec![vec![vec![[0, 1]], vec![[1, 2]]]],
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[[0.0, 0.0, 0.0]; 3],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1, 2])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        search.remaining_equation_merge_capacity(&mut quotient),
        None
    );
}

#[test]
fn completed_mesh_search_continues_to_check_uniqueness() {
    let assignments = Vec::new();
    let edge_candidates = Vec::new();
    let edge_rows = Vec::new();
    let vertex_points = Vec::new();
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &vertex_points,
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: Vec::new(),
        visited_states: HashSet::new(),
        solution: Some((
            StandardTopology {
                faces: Vec::new(),
                edge_rows: Vec::new(),
                vertex_points: Vec::new(),
                logical_vertex_count: 0,
            },
            Vec::new(),
        )),
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.should_stop());
}

#[test]
fn mesh_selection_declines_when_its_work_budget_is_exhausted() {
    let mut search = MeshSelectionSearch {
        assignments: &[],
        possible_face_equations: Vec::new(),
        possible_face_choices: Vec::new(),
        face_work: Vec::new(),
        edge_candidates: &[],
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: Vec::new(),
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(0),
        domains: Vec::new(),
        members: Vec::new(),
    };

    search.search_with_limit(&quotient, 0);

    assert!(search.exhausted);
    assert!(search.solution.is_none());
}

#[test]
fn mesh_selection_finishes_the_active_face_component_first() {
    const UNRELATED_FACE_COUNT: usize = 1_000;
    let use_edge = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let selected_assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0)]],
    };
    let mut assignments = vec![vec![selected_assignment]];
    assignments.extend((0..UNRELATED_FACE_COUNT).map(|index| {
        vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_edge(index + 2)]],
        }]
    }));
    assignments.push(vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_edge(0), use_edge(1)]],
    }]);
    let face_count = assignments.len();
    let edge_count = UNRELATED_FACE_COUNT + 2;
    let mut selected = vec![None; face_count];
    selected[0] = Some((0, vec![vec![false]]));
    let mut edge_candidates = vec![vec![[0, 0]]; edge_count];
    edge_candidates[1] = vec![[1, 1]];
    let mut domains = Vec::with_capacity(edge_count * 2);
    for candidates in &edge_candidates {
        let domain = Arc::new(candidates.iter().flatten().copied().collect::<HashSet<_>>());
        domains.push(domain.clone());
        domains.push(domain);
    }
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: vec![Vec::new(); face_count],
        possible_face_choices: vec![Vec::new(); face_count],
        face_work: vec![Some(1); face_count],
        edge_candidates: &edge_candidates,
        edge_rows: &[],
        vertex_points: &[[0.0; 3], [1.0, 0.0, 0.0]],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected,
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(edge_count * 2),
        domains,
        members: (0..edge_count * 2).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(5);
    let propagation_budget = WorkBudget::new(0);

    search.search_from_state(&quotient, true, &budget, &propagation_budget);

    assert!(!search.exhausted);
    assert!(search.solution.is_none());
}

#[test]
fn forced_face_selection_does_not_exhaust_the_work_budget() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: Some(false),
        }]],
    }]];
    let edge_candidates = vec![vec![[0, 0]]];
    let edge_rows = vec![EdgeRow {
        kind: 1,
        handles: vec![0],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &[[0.0, 0.0, 0.0]],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };

    search.search(&quotient);

    assert!(!search.exhausted);
}

#[test]
fn overmerged_face_options_do_not_exhaust_the_work_budget() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: None,
            },
        ]],
    }]];
    let edge_candidates = vec![Vec::new(); 2];
    let edge_rows = vec![
        EdgeRow {
            kind: 1,
            handles: vec![0],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        };
        2
    ];
    let mut search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &edge_candidates,
        edge_rows: &edge_rows,
        vertex_points: &[[0.0, 0.0, 0.0]; 3],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1, 2]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    search.search(&quotient);

    assert!(!search.exhausted);
    assert!(search.solution.is_none());
}

#[test]
fn mesh_selection_merges_corner_equations_common_to_every_option() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: Some(false),
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 2,
                reversed: Some(false),
            },
            MeshBoundaryEdgeCandidate {
                edge: 2,
                start: 2,
                end: 3,
                reversed: Some(false),
            },
        ]],
    };
    let assignments = vec![vec![assignment]];
    let candidates = vec![vec![], vec![], vec![]];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(0));
    assert_eq!(quotient.root_count(), 3);
}

#[test]
fn mesh_selection_merges_equations_common_to_every_assignment() {
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: edge + 1,
        reversed: Some(reversed),
    };
    let assignments = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, false), use_(1, false), use_(2, false)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0, false), use_(1, false), use_(2, true)]],
        },
    ]];
    let candidates = vec![vec![], vec![], vec![]];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(2)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.root_count(), 5);
}

#[test]
fn mesh_selection_common_equations_ignore_infeasible_assignments() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: edge + 1,
        reversed: Some(false),
    };
    let assignments = vec![vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(2)]],
        },
    ]];
    let candidates = vec![vec![]; 3];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(2)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [1, 0, 0, 1, 2, 2]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(0));
    assert_eq!(quotient.root_count(), 4);
}

#[test]
fn mesh_selection_propagates_closed_ports_without_enumerating_directions() {
    let boundary = (0..13)
        .map(|edge| MeshBoundaryEdgeCandidate {
            edge,
            start: edge,
            end: (edge + 1) % 13,
            reversed: None,
        })
        .collect();
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![boundary],
    }]];
    let candidates = vec![vec![]; 13];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(26),
        domains: (0..26).map(|_| Arc::new((0..13).collect())).collect(),
        members: (0..26).map(|node| vec![node]).collect(),
    };
    for edge in 0..13 {
        quotient.merge(edge * 2, edge * 2 + 1).expect("closed port");
    }

    assert_eq!(quotient.root_count(), 13);
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(quotient.root_count(), 1);
}

#[test]
fn face_equation_cache_ignores_unrelated_quotient_components() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate {
                edge: 0,
                start: 0,
                end: 1,
                reversed: None,
            },
            MeshBoundaryEdgeCandidate {
                edge: 1,
                start: 1,
                end: 0,
                reversed: None,
            },
        ]],
    }]];
    let candidates = vec![vec![]; 3];
    let search = MeshSelectionSearch {
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
        candidate_gauge: None,
        port_identities: None,
        fixed_face_directions: Vec::new(),
        fixed_edge_orientations: Vec::new(),
        edge_has_fixed_direction: Vec::new(),
        selected: vec![None],
        visited_states: HashSet::new(),
        solution: None,
        ambiguous: false,
        exhausted: false,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient.merge(4, 5).expect("unrelated component merge");
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient
        .merge(0, 4)
        .expect("component joined to a face port");
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 2);
    {
        let mut cache = search.face_equation_cache.borrow_mut();
        for key in 1..=MAX_FACE_EQUATION_CACHE_ENTRIES {
            cache.insert((key, Vec::new()), Vec::new());
        }
    }
    quotient.merge(1, 2).expect("new face-component merge");
    assert!(search.propagate_forced_face_equations(&mut quotient));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
}
