use super::*;

#[test]
fn incidence_components_join_only_through_shared_face_vertices() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 3], [2, 3]],
        vec![[4, 5], [4, 6]],
        vec![[7, 8]],
    ];
    let edge_faces = [[0, 0], [0, 0], [0, 0], [0, 0]];
    assert_eq!(
        crate::solve::incidence::incidence_choice_components(&choices, &edge_faces, None, None),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn incidence_component_preflight_ignores_disjoint_unassigned_cycles_on_the_same_face() {
    let choices = vec![vec![[0, 0], [1, 1]], vec![[2, 2], [3, 3]]];
    let edge_faces = [[0, 0], [0, 0]];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("independent same-face cycle solutions");

    assert_eq!(solutions.len(), 4);
}

#[test]
fn incidence_component_composition_does_not_allocate_the_declared_point_product() {
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &[vec![[usize::MAX - 1, usize::MAX - 1]]],
        &[[0, 0]],
        1,
        usize::MAX,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("sparse component degree state");

    assert_eq!(solutions, vec![vec![[usize::MAX - 1, usize::MAX - 1]]]);
}

#[test]
fn incidence_component_preflight_retains_fixed_chain_frontiers() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 3]],
        vec![[3, 4], [3, 5]],
        vec![[4, 0]],
    ];
    let edge_faces = [[0, 0]; 4];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        6,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("fixed-chain component frontier");

    assert_eq!(solutions, vec![vec![[0, 1], [1, 3], [3, 4], [4, 0]]]);
}

#[test]
fn partial_incidence_constraint_joins_every_component_it_can_couple() {
    let components = vec![vec![0, 2], vec![1], vec![3, 5], vec![4]];
    let active = [true, false, false, true, false, false];

    assert_eq!(
        crate::solve::incidence::join_incidence_components_by_coupling(components, &active),
        vec![vec![0, 2, 3, 5], vec![1], vec![4]],
    );
}

#[test]
fn incidence_components_order_by_endpoint_branch_width() {
    let choices = vec![
        vec![[0, 0], [1, 1]],
        vec![[2, 2], [3, 3], [4, 4]],
        vec![[5, 5], [6, 6]],
        vec![[7, 7]],
    ];
    let mut components = vec![vec![0, 2], vec![1], vec![3]];

    crate::solve::incidence::order_incidence_components_by_branch_width(&mut components, &choices)
        .expect("valid component edges");

    assert_eq!(components, vec![vec![3], vec![1], vec![0, 2]]);
}

#[test]
fn incidence_components_order_prerequisites_without_joining_domains() {
    let choices = vec![vec![[0, 0], [1, 1]], vec![[2, 2], [3, 3]], vec![[4, 4]]];
    let mut components = vec![vec![0], vec![1], vec![2]];
    let dependencies = [Vec::new(), vec![0], Vec::new()];

    crate::solve::incidence::order_incidence_components_by_constraints(
        &mut components,
        &choices,
        None,
        Some(&dependencies),
    )
    .expect("acyclic component prerequisites");

    assert_eq!(components, vec![vec![2], vec![0], vec![1]]);
}

#[test]
fn incidence_components_reject_prerequisite_cycles() {
    let choices = vec![vec![[0, 0]], vec![[1, 1]]];
    let mut components = vec![vec![0], vec![1]];
    let predecessors = [Some(1), Some(0)];

    assert!(
        crate::solve::incidence::order_incidence_components_by_constraints(
            &mut components,
            &choices,
            Some(&predecessors),
            None,
        )
        .is_none()
    );

    let mut component = vec![vec![0, 1]];
    assert!(
        crate::solve::incidence::order_incidence_components_by_constraints(
            &mut component,
            &choices,
            Some(&predecessors),
            None,
        )
        .is_none()
    );
}

#[test]
fn incidence_components_keep_fixed_face_boundaries_independent() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 2], [1, 3]],
        vec![[4, 5], [4, 6]],
        vec![[5, 6], [5, 7]],
    ];
    let edge_faces = [[0, 0]; 4];
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 1,
        reversed: None,
    };
    let fixed = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)], vec![use_(2), use_(3)]],
        },
    ])];
    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &choices,
            &edge_faces,
            Some(&fixed),
            None,
        ),
        vec![vec![0, 1], vec![2, 3]]
    );

    let alternatives = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)], vec![use_(2), use_(3)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(2)], vec![use_(1), use_(3)]],
        },
    ])];
    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &choices,
            &edge_faces,
            Some(&alternatives),
            None,
        ),
        vec![vec![0, 1, 2, 3]]
    );
}

#[test]
fn incidence_components_include_overlapping_quotient_domains() {
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[3, 4], [3, 5]],
        vec![[6, 7], [6, 8]],
    ];
    let edge_faces = [[0, 0], [1, 1], [2, 2]];
    let quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [
            HashSet::from([0, 1]),
            HashSet::from([0, 2]),
            HashSet::from([1, 3]),
            HashSet::from([3, 4]),
            HashSet::from([6, 7]),
            HashSet::from([6, 8]),
        ]
        .map(Arc::new)
        .to_vec(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &choices,
            &edge_faces,
            None,
            Some(&quotient)
        ),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn incidence_components_solve_coupled_face_vertex_closures() {
    let a = vec![[0, 2], [0, 12], [2, 12]];
    let b = vec![[1, 3], [1, 1969], [3, 1969]];
    let c = vec![
        [0, 1],
        [0, 2],
        [0, 3],
        [0, 12],
        [0, 1969],
        [1, 2],
        [1, 3],
        [1, 12],
        [1, 1969],
        [2, 3],
        [2, 12],
        [2, 1969],
        [3, 12],
        [3, 1969],
        [12, 1969],
    ];
    let choices = vec![
        a.clone(),
        b.clone(),
        a,
        b,
        c,
        vec![[2, 3]],
        vec![[2, 12]],
        vec![[12, 1969]],
        vec![[3, 1969]],
    ];
    let edge_faces = [
        [1, 0],
        [3, 0],
        [2, 1],
        [2, 3],
        [2, 0],
        [0, 0],
        [1, 1],
        [2, 2],
        [3, 3],
    ];
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        4,
        1970,
        None,
        None,
        None,
        &|_| true,
    )
    .expect("component closure solution");
    assert!(solutions
        .iter()
        .any(|solution| { solution[..5] == [[0, 2], [1, 3], [0, 12], [1, 1969], [0, 1]] }));
}

#[test]
fn incidence_components_reject_degree_cycles_in_the_wrong_edge_order() {
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let edge_faces = [[0, 0]; 4];
    let mesh_assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![(0..4)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                })
                .collect()],
        },
    ])];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        Some(&mesh_assignments),
        None,
        None,
        &|_| true,
    )
    .expect("ordered component solution");

    assert_eq!(solutions.len(), 1);
    assert_eq!(solutions[0], [[0, 1], [1, 2], [2, 3], [3, 0]]);
}

#[test]
fn incidence_unordered_full_cycle_rejects_disconnected_degree_cycles() {
    let choices = vec![
        vec![[0, 1]],
        vec![[0, 1], [1, 2]],
        vec![[2, 3]],
        vec![[2, 3], [0, 3]],
    ];
    let edge_faces = [[0, 0]; 4];
    let domains = vec![MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3])];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        Some(&domains),
        None,
        None,
        &|_| true,
    )
    .expect("connected cycle solution");

    assert_eq!(solutions, vec![vec![[0, 1], [1, 2], [2, 3], [0, 3]]]);
}

#[test]
fn deferred_boundary_enforces_anchored_gap_capacities() {
    let domain = crate::solve::missing_edge::MeshDeferredFaceBoundary {
        cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
            length: 6,
            exact_uses: vec![
                (
                    MeshBoundaryEdgeCandidate {
                        edge: 0,
                        start: 0,
                        end: 1,
                        reversed: Some(false),
                    },
                    1,
                ),
                (
                    MeshBoundaryEdgeCandidate {
                        edge: 3,
                        start: 3,
                        end: 4,
                        reversed: Some(false),
                    },
                    1,
                ),
            ],
        }],
        missing_edges: vec![1, 2, 4, 5],
    };
    let valid = [[0, 1], [1, 2], [2, 3], [3, 4], [4, 5], [0, 5]];
    let overfilled_first_gap = [[0, 1], [1, 2], [2, 3], [4, 5], [3, 4], [0, 5]];

    assert!(crate::solve::incidence::deferred_boundary_closes(
        &domain, &valid
    ));
    let assignment = crate::solve::incidence::deferred_boundary_assignment(&domain, &valid)
        .expect("materialized deferred boundary");
    assert_eq!(
        assignment.boundaries[0]
            .iter()
            .map(|use_| (use_.edge, use_.reversed))
            .collect::<Vec<_>>(),
        vec![
            (0, Some(false)),
            (1, None),
            (2, None),
            (3, Some(false)),
            (4, None),
            (5, None),
        ]
    );
    assert!(!crate::solve::incidence::deferred_boundary_closes(
        &domain,
        &overfilled_first_gap
    ));
}

#[test]
fn deferred_anchored_runs_propagate_forced_adjacencies() {
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 2,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 2,
                exact_uses: vec![(use_(0, 0), 1), (use_(1, 1), 1)],
            }],
            missing_edges: Vec::new(),
        },
    )];
    let candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 2, &[[0, 1], [2, 3]])
            .expect("initial quotient");
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("forced deferred quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(3));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
}

#[test]
fn deferred_quotient_retains_unknown_exact_run_direction() {
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 2,
        reversed: None,
    };
    let domains = [MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 2,
                exact_uses: vec![(use_(0, 0), 1), (use_(1, 1), 1)],
            }],
            missing_edges: Vec::new(),
        },
    )];
    let candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 2, &[[0, 1], [2, 3]])
            .expect("initial quotient");
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("unknown exact direction is deferred");

    assert_ne!(quotient.union.find(0), quotient.union.find(3));
}

#[test]
fn deferred_gap_search_propagates_quotient_forced_edge_order() {
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 4,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 4,
                exact_uses: vec![(use_(0, 0), 1), (use_(1, 2), 1)],
            }],
            missing_edges: vec![2, 3],
        },
    )];
    let candidates = vec![vec![[0, 1]], vec![[2, 3]], vec![[1, 2]], vec![[0, 3]]];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(8),
        domains: (0..8)
            .map(|node| Arc::new(HashSet::from([[0, 1, 2, 3, 1, 2, 3, 0][node]])))
            .collect(),
        members: (0..8).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(10_000);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("deferred gap quotient");

    assert_eq!(quotient.union.find(1), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(6));
    assert_eq!(quotient.union.find(7), quotient.union.find(0));
}

#[test]
fn ordered_structural_equations_propagate_without_direction_enumeration() {
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
            ]],
        },
    ])];
    let candidates = vec![vec![[0, 0]], vec![[0, 0]]];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(0, 1).expect("first closed edge");
    quotient.merge(2, 3).expect("second closed edge");
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("structural quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(2));
}

#[test]
fn ordered_face_options_preflight_exact_signature_work() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0)]],
        },
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1)]],
        },
    ])];
    let candidates = vec![Vec::new(), Vec::new()];
    let broad = Arc::new((0..100).collect::<HashSet<_>>());
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![broad; 4],
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("bounded common quotient propagation");

    assert_eq!(quotient.root_count(), 4);
    assert!(!budget.exhausted());
}

#[test]
fn ordered_cycle_support_propagates_domain_forced_directions() {
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
            ]],
        },
    ])];
    let candidates = vec![vec![[0, 1]], vec![[0, 1]]];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([1])),
            Arc::new(HashSet::from([0])),
        ],
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("supported cycle quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(3));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
}

#[test]
fn ordered_components_retain_unknown_edges_in_the_abstract_quotient() {
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: Some(false),
                },
            ]],
        },
    ])];
    let candidates = vec![Vec::new(), Vec::new()];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0, 1]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("ordered component quotient");

    assert_eq!(quotient.union.find(0), quotient.union.find(3));
    assert_eq!(quotient.union.find(1), quotient.union.find(2));
}

#[test]
fn unordered_components_close_cycles_in_the_abstract_quotient() {
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![2, 0, 1])];
    let candidates = vec![Vec::new(); 3];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [0, 1, 1, 2, 2, 0]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
        members: (0..6).map(|node| vec![node]).collect(),
    };

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("unordered component quotient");

    assert_eq!(quotient.union.find(1), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(0));
}

#[test]
fn compact_unordered_boundary_rejects_partial_subtours() {
    let domain = MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3, 4]);

    assert!(!compact_boundary_domain_viable(
        &domain,
        &[Some([0, 1]), Some([1, 2]), Some([2, 0]), None, None],
        None,
    ));
    assert!(compact_boundary_domain_viable(
        &domain,
        &[Some([0, 1]), Some([1, 2]), Some([2, 3]), None, None],
        None,
    ));
    assert!(compact_boundary_domain_viable(
        &domain,
        &[
            Some([0, 1]),
            Some([1, 2]),
            Some([2, 3]),
            Some([3, 4]),
            Some([4, 0]),
        ],
        None,
    ));
}

#[test]
fn unordered_component_enumeration_is_atomic_at_its_state_limit() {
    let quotient = MeshQuotient {
        union: UnionFind::new(16),
        domains: repeated_domain(HashSet::from([0]), 16),
        members: (0..16).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(10_000);

    assert!(
        crate::solve::mesh_quotient::bounded_unordered_cycle_assignments(
            &(0..8).collect::<Vec<_>>(),
            &quotient,
            16,
            &budget,
        )
        .is_none()
    );
}

#[test]
fn deferred_components_select_gap_orders_in_the_abstract_quotient() {
    let use_ = |edge, start| MeshBoundaryEdgeCandidate {
        edge,
        start,
        end: (start + 1) % 4,
        reversed: Some(false),
    };
    let domains = [MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 4,
                exact_uses: vec![(use_(0, 0), 1), (use_(1, 2), 1)],
            }],
            missing_edges: vec![2, 3],
        },
    )];
    let candidates = vec![Vec::new(); 4];
    let mut quotient = MeshQuotient {
        union: UnionFind::new(8),
        domains: (0..8)
            .map(|node| Arc::new(HashSet::from([[0, 1, 2, 3, 1, 2, 3, 0][node]])))
            .collect(),
        members: (0..8).map(|node| vec![node]).collect(),
    };

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("deferred component quotient");

    assert_eq!(quotient.union.find(1), quotient.union.find(4));
    assert_eq!(quotient.union.find(5), quotient.union.find(2));
    assert_eq!(quotient.union.find(3), quotient.union.find(6));
    assert_eq!(quotient.union.find(7), quotient.union.find(0));
}

#[test]
fn deferred_faces_share_one_endpoint_quotient() {
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: (edge + 1) % 2,
        reversed: Some(reversed),
    };
    let domain = |second_reversed| {
        MeshFaceBoundaryDomain::DeferredValidation(
            crate::solve::missing_edge::MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                    length: 2,
                    exact_uses: vec![(use_(0, false), 1), (use_(1, second_reversed), 1)],
                }],
                missing_edges: Vec::new(),
            },
        )
    };
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&choices, 2, &[[0, 1], [2, 3]])
            .expect("initial quotient");
    let budget = WorkBudget::new(10_000);

    assert!(
        crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &[domain(false), domain(false)],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            &quotient,
            &budget,
        )
    );
    assert!(
        !crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &[domain(false), domain(true)],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            &quotient,
            &budget,
        )
    );
}

#[test]
fn compact_faces_share_one_physical_edge_direction_gauge() {
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2]],
        vec![[0, 2]],
        vec![[0, 3]],
        vec![[1, 3]],
    ];
    let assignment = choices
        .iter()
        .map(|choices| Some(choices[0]))
        .collect::<Vec<_>>();
    let domains = [
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2]),
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 3, 4]),
    ];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &choices,
        4,
        &[[0, 1], [2, 3], [4, 5], [6, 7], [8, 9]],
    )
    .expect("initial quotient");
    let budget = WorkBudget::new(10_000);

    assert!(
        crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &domains,
            &choices,
            &assignment,
            None,
            &quotient,
            &budget,
        )
    );
}

#[test]
fn compact_face_quotient_states_accumulate_across_calls() {
    let use_ = |edge, reversed| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: (edge + 1) % 2,
        reversed: Some(reversed),
    };
    let domain = |second_reversed| {
        MeshFaceBoundaryDomain::DeferredValidation(
            crate::solve::missing_edge::MeshDeferredFaceBoundary {
                cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                    length: 2,
                    exact_uses: vec![(use_(0, false), 1), (use_(1, second_reversed), 1)],
                }],
                missing_edges: Vec::new(),
            },
        )
    };
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let assignment = [Some([0, 1]), Some([0, 1])];
    let quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&choices, 2, &[[0, 1], [2, 3]])
            .expect("initial quotient");
    let budget = WorkBudget::new(10_000);
    let first = domain(false);
    let conflicting = domain(true);
    let initial = vec![(quotient.clone(), HashSet::new())];

    let crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(first_states) =
        crate::solve::incidence::advance_compact_boundary_domains(
            [&first],
            &choices,
            &assignment,
            None,
            initial.clone(),
            &budget,
        )
    else {
        panic!("first face quotient");
    };
    assert!(matches!(
        crate::solve::incidence::advance_compact_boundary_domains(
            [&conflicting],
            &choices,
            &assignment,
            None,
            initial,
            &budget,
        ),
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)
    ));
    assert!(matches!(
        crate::solve::incidence::advance_compact_boundary_domains(
            [&conflicting],
            &choices,
            &assignment,
            None,
            first_states,
            &budget,
        ),
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Rejected
    ));
}

#[test]
fn compact_face_quotient_state_cap_is_exhausted() {
    const EDGE_COUNT: usize = 14;
    let choices = vec![Vec::new(); EDGE_COUNT];
    let quotient = MeshQuotient {
        union: UnionFind::new(EDGE_COUNT * 2),
        domains: (0..EDGE_COUNT * 2)
            .map(|node| {
                Arc::new(if node % 2 == 0 {
                    HashSet::from([0, 1])
                } else {
                    HashSet::from([1, 2])
                })
            })
            .collect(),
        members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
    };
    let boundary = (0..EDGE_COUNT)
        .map(|edge| MeshBoundaryEdgeCandidate {
            edge,
            start: 0,
            end: 0,
            reversed: None,
        })
        .collect();
    let domain = MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
        boundaries: vec![boundary],
    }]);
    let assignment = (0..EDGE_COUNT).map(|_| Some([1, 1])).collect::<Vec<_>>();
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    let outcome = crate::solve::incidence::advance_compact_boundary_domains(
        [&domain],
        &choices,
        &assignment,
        None,
        vec![(quotient, HashSet::new())],
        &budget,
    );
    assert!(matches!(
        outcome,
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Exhausted
    ));
    assert!(!budget.exhausted());
}

#[test]
fn incidence_components_filter_complete_solutions_during_search() {
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let edge_faces = [[0, 0]; 4];
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        1,
        4,
        None,
        None,
        None,
        &|pairs| pairs[1] == [2, 3],
    )
    .expect("filtered component solution");

    assert_eq!(solutions, vec![vec![[0, 1], [2, 3], [1, 2], [3, 0]]]);
}

#[test]
fn incidence_components_apply_monotone_partial_constraints_before_solution_limits() {
    let choices = vec![
        (0..300).map(|point| [point, point]).collect::<Vec<_>>(),
        (300..600).map(|point| [point, point]).collect::<Vec<_>>(),
    ];
    let edge_faces = [[0, 0], [1, 1]];
    let partial = |assignment: &[Option<[usize; 2]>]| {
        assignment[0].is_none_or(|pair| pair == [0, 0])
            && assignment[1].is_none_or(|pair| pair == [300, 300])
    };
    let active_edges = [true, true];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        2,
        600,
        None,
        None,
        Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_predecessors: None,
            assignment_dependencies: None,
            valid: &partial,
        }),
        &|_| true,
    )
    .expect("partially constrained component solutions");

    assert_eq!(solutions, vec![vec![[0, 0], [300, 300]]]);
}

#[test]
fn incidence_components_reuse_independent_solution_domains() {
    use std::ops::ControlFlow;

    const COMPONENT_COUNT: usize = 15;
    let choices = (0..COMPONENT_COUNT)
        .map(|component| {
            let first = component * 2;
            vec![[first, first], [first + 1, first + 1]]
        })
        .collect::<Vec<_>>();
    let edge_faces = (0..COMPONENT_COUNT)
        .map(|face| [face, face])
        .collect::<Vec<_>>();
    let mut visited = 0usize;

    let outcome = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        COMPONENT_COUNT,
        COMPONENT_COUNT * 2,
        None,
        None,
        None,
        &|_| true,
        &mut |_| {
            visited += 1;
            ControlFlow::Continue(())
        },
    );

    assert_eq!(
        outcome,
        crate::solve::incidence::IncidenceSolve::Solved(1 << COMPONENT_COUNT)
    );
    assert_eq!(visited, 1 << COMPONENT_COUNT);
}

#[test]
fn incidence_components_include_fixed_incidence_chains() {
    let choices = vec![vec![[0, 0], [0, 1]], vec![[2, 2], [2, 3]], vec![[0, 2]]];
    let components =
        crate::solve::incidence::incidence_choice_components(&choices, &[[0, 0]; 3], None, None);

    assert_eq!(components, vec![vec![0, 1]]);
}

#[test]
fn incidence_components_preflight_independent_unsatisfiable_domains() {
    use std::ops::ControlFlow;

    const BROAD_COMPONENT_COUNT: usize = 15;
    let mut choices = (0..BROAD_COMPONENT_COUNT)
        .map(|component| {
            let first = component * 2;
            vec![[first, first], [first + 1, first + 1]]
        })
        .collect::<Vec<_>>();
    choices.push(vec![[30, 30], [31, 31], [32, 32], [33, 33]]);
    let edge_faces = (0..choices.len())
        .map(|face| [face, face])
        .collect::<Vec<_>>();
    let constrained_edge = choices.len() - 1;
    let active_edges = (0..choices.len())
        .map(|edge| edge == constrained_edge)
        .collect::<Vec<_>>();
    let partial = |assignment: &[Option<[usize; 2]>]| assignment[constrained_edge].is_none();
    let mut visited = false;

    let outcome = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        choices.len(),
        34,
        None,
        None,
        Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_predecessors: None,
            assignment_dependencies: None,
            valid: &partial,
        }),
        &|_| true,
        &mut |_| {
            visited = true;
            ControlFlow::Continue(())
        },
    );

    assert_eq!(
        outcome,
        crate::solve::incidence::IncidenceSolve::Rejected(
            crate::solve::incidence::IncidenceRejection::ComponentDomain
        )
    );
    assert!(!visited);
}

#[test]
fn incidence_components_discard_quotient_impossible_complete_solutions() {
    let choices = vec![vec![[0, 0], [1, 1]], vec![[1, 1]]];
    let edge_faces = [[0, 0], [1, 1]];
    let mut union = UnionFind::new(4);
    union.union(0, 1);
    union.union(2, 3);
    let quotient = MeshQuotient {
        union,
        domains: (0..4).map(|_| Arc::new(HashSet::from([0, 1]))).collect(),
        members: vec![vec![0, 1], Vec::new(), vec![2, 3], Vec::new()],
    };

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        2,
        2,
        None,
        Some(&quotient),
        None,
        &|_| true,
    )
    .expect("globally assignable component solution");

    assert_eq!(solutions, vec![vec![[0, 0], [1, 1]]]);
}

#[test]
fn incidence_components_preflight_quotient_impossible_domains() {
    use std::ops::ControlFlow;

    const BROAD_COMPONENT_COUNT: usize = 15;
    let mut choices = (0..BROAD_COMPONENT_COUNT)
        .map(|component| {
            let first = component * 2;
            vec![[first, first], [first + 1, first + 1]]
        })
        .collect::<Vec<_>>();
    choices.extend([vec![[30, 30], [31, 31]], vec![[30, 30]], vec![[31, 31]]]);
    let edge_faces = (0..choices.len())
        .map(|face| [face, face])
        .collect::<Vec<_>>();
    let mut union = UnionFind::new(choices.len() * 2);
    let mut domains = Vec::with_capacity(choices.len() * 2);
    let mut members = (0..choices.len() * 2)
        .map(|node| vec![node])
        .collect::<Vec<_>>();
    for edge in 0..choices.len() {
        union.union(edge * 2, edge * 2 + 1);
        members[edge * 2].push(edge * 2 + 1);
        members[edge * 2 + 1].clear();
        let points = if edge < BROAD_COMPONENT_COUNT {
            HashSet::from([edge * 2, edge * 2 + 1])
        } else {
            HashSet::from([30, 31])
        };
        let points = Arc::new(points);
        domains.extend([points.clone(), points]);
    }
    let quotient = MeshQuotient {
        union,
        domains,
        members,
    };
    let mut visited = false;

    let outcome = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        choices.len(),
        32,
        None,
        Some(&quotient),
        None,
        &|_| true,
        &mut |_| {
            visited = true;
            ControlFlow::Continue(())
        },
    );

    assert_eq!(
        outcome,
        crate::solve::incidence::IncidenceSolve::Rejected(
            crate::solve::incidence::IncidenceRejection::ComponentDomain
        )
    );
    assert!(!visited);
}

#[test]
fn fixed_incidence_assignments_must_satisfy_the_mesh_quotient() {
    let choices = vec![vec![[0, 0]], vec![[0, 0]]];
    let edge_faces = [[0, 0], [1, 1]];
    let quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: (0..4).map(|_| Arc::new(HashSet::from([0]))).collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert_eq!(
        crate::solve::incidence::component_incidence_pair_solution_outcome(
            &choices,
            &edge_faces,
            2,
            1,
            None,
            Some(&quotient),
            None,
            &|_| true,
        ),
        crate::solve::incidence::IncidenceSolve::Rejected(
            crate::solve::incidence::IncidenceRejection::FixedAssignment
        )
    );
}

#[test]
fn incidence_outcome_distinguishes_exhaustion_from_rejection() {
    use crate::solve::incidence::{component_incidence_pair_solution_outcome, IncidenceSolve};

    let choices = vec![(0..300).map(|point| [point, point]).collect::<Vec<_>>()];
    assert_eq!(
        component_incidence_pair_solution_outcome(
            &choices,
            &[[0, 0]],
            1,
            300,
            None,
            None,
            None,
            &|_| true,
        ),
        IncidenceSolve::Exhausted
    );
    assert_eq!(
        component_incidence_pair_solution_outcome(
            &[Vec::new()],
            &[[0, 0]],
            1,
            1,
            None,
            None,
            None,
            &|_| true,
        ),
        IncidenceSolve::Rejected(crate::solve::incidence::IncidenceRejection::FixedAssignment)
    );
}

#[test]
fn incidence_component_products_stream_until_the_consumer_stops() {
    use crate::solve::incidence::{visit_component_incidence_pair_solutions, IncidenceSolve};
    use std::ops::ControlFlow;

    let choices = (0..9)
        .map(|edge| vec![[edge * 2, edge * 2], [edge * 2 + 1, edge * 2 + 1]])
        .collect::<Vec<_>>();
    let edge_faces = (0..9).map(|face| [face, face]).collect::<Vec<_>>();
    let mut visited = 0usize;

    let outcome = visit_component_incidence_pair_solutions(
        &choices,
        &edge_faces,
        9,
        18,
        None,
        None,
        None,
        &|_| true,
        &mut |_| {
            visited += 1;
            if visited == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    );

    assert_eq!(outcome, IncidenceSolve::Solved(2));
    assert_eq!(visited, 2);
}

#[test]
fn incidence_component_prefix_can_prove_the_consumer_result_before_exhaustion() {
    use crate::solve::incidence::{visit_component_incidence_pair_solutions, IncidenceSolve};
    use std::cell::Cell;
    use std::ops::ControlFlow;

    let choices = vec![(0..300).map(|point| [point, point]).collect::<Vec<_>>()];
    let mut visited = 0usize;
    let validated = Cell::new(0usize);
    let outcome = visit_component_incidence_pair_solutions(
        &choices,
        &[[0, 0]],
        1,
        300,
        None,
        None,
        None,
        &|_| {
            validated.set(validated.get() + 1);
            true
        },
        &mut |_| {
            visited += 1;
            if visited == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        },
    );

    assert_eq!(outcome, IncidenceSolve::Solved(2));
    assert_eq!(visited, 2);
    assert_eq!(validated.get(), 2);
}
