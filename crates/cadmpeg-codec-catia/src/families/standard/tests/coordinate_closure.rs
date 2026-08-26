use super::*;

#[test]
fn endpoint_ports_propagate_resolved_pairs_to_unresolved_edges() {
    let ports = [[10, 11], [11, 12], [12, 13], [13, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), None, Some([3, 0])];
    assert_eq!(
        propagate_edge_port_points(&ports, &pairs),
        Some(vec![Some([0, 1]), Some([1, 2]), Some([2, 3]), Some([3, 0]),])
    );
}

#[test]
fn ordered_endpoint_seed_orients_a_seedless_port_component() {
    let ports = [[10, 11]];
    let pairs = [Some([0, 1])];
    let ordered = [Some([1, 0])];

    assert_eq!(
        propagate_edge_port_points_with_ordered_seeds(&ports, &pairs, &ordered),
        Some(vec![Some([1, 0])])
    );
}

#[test]
fn ordered_endpoint_seed_must_agree_with_the_unordered_candidate() {
    assert_eq!(
        propagate_edge_port_points_with_ordered_seeds(
            &[[10, 11]],
            &[Some([0, 1])],
            &[Some([0, 2])],
        ),
        None
    );
}

#[test]
fn deferred_port_component_closure_reaches_transitive_neighbors() {
    let ports = [[10, 11], [11, 12], [12, 13], [20, 21]];
    let mut deferred = [true, false, false, false];

    assert!(expand_deferred_edge_port_components(&ports, &mut deferred));
    assert_eq!(deferred, [true, true, true, false]);
}

#[test]
fn deferred_mesh_port_component_does_not_orient_unordered_neighbors() {
    let ports = [[10, 11], [11, 10], [12, 10], [13, 11], [10, 97], [11, 98]];
    let pairs = [
        Some([0, 1]),
        Some([0, 1]),
        Some([0, 2]),
        Some([0, 3]),
        Some([0, 79]),
        Some([0, 79]),
    ];

    assert_eq!(
        propagate_edge_port_points_with_ordered_seeds_and_deferred(
            &ports,
            &pairs,
            &[],
            &[true, false, false, false, false, false],
        ),
        Some(vec![None, None, None, None, None, None]),
    );
}

#[test]
fn partial_ordered_endpoint_seed_resolves_a_row_without_native_ports() {
    let ports = [Some([10, 11]), None];
    let pairs = [Some([0, 1]), None];
    let ordered = [Some([1, 0]), Some([2, 3])];

    assert_eq!(
        propagate_partial_edge_port_points_with_ordered_seeds(&ports, &pairs, &ordered),
        Some(vec![Some([1, 0]), Some([2, 3])])
    );
}

#[test]
fn partial_endpoint_ports_propagate_known_components_only() {
    let ports = [
        Some([10, 11]),
        Some([11, 12]),
        None,
        Some([12, 13]),
        Some([13, 10]),
    ];
    let pairs = [Some([0, 1]), Some([1, 2]), Some([8, 9]), None, Some([3, 0])];

    assert_eq!(
        propagate_partial_edge_port_points_with_ordered_seeds(&ports, &pairs, &[]),
        Some(vec![
            Some([0, 1]),
            Some([1, 2]),
            Some([8, 9]),
            Some([2, 3]),
            Some([3, 0]),
        ])
    );
}

#[test]
fn native_edge_carrier_binding_requires_equal_object_identity() {
    use crate::families::standard::decode::standard_native_support_edge_ids;
    use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};

    let supports = [
        StandardCurveSupport {
            pos: 0,
            tag: 70,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Line,
        },
        StandardCurveSupport {
            pos: 1,
            tag: 900,
            faces: [0, 0],
            geometry: StandardCurveGeometry::Line,
        },
    ];
    let native_support_ids = HashSet::from([70, 71]);
    assert_eq!(
        standard_native_support_edge_ids(&supports, &native_support_ids),
        vec![Some(70), None]
    );

    assert_eq!(
        standard_native_support_edge_ids(&supports[1..], &HashSet::from([900])),
        vec![Some(900)]
    );
}

#[test]
fn endpoint_port_propagation_requires_a_point_bijection() {
    assert_eq!(
        propagate_edge_port_points(&[[10, 11]], &[Some([0, 1])]),
        Some(vec![Some([0, 1])])
    );
    assert_eq!(
        propagate_edge_port_points(&[[10, 11], [10, 12]], &[Some([0, 1]), Some([0, 1])]),
        None
    );
    assert_eq!(
        propagate_edge_port_points(&[[10, 11]], &[Some([0, 0])]),
        None
    );
}

#[test]
fn endpoint_port_propagation_closes_equal_port_edges() {
    let ports = [[10, 11], [11, 12], [10, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), None];

    assert_eq!(
        propagate_edge_port_points(&ports, &pairs),
        Some(vec![Some([0, 1]), Some([1, 2]), Some([0, 0])])
    );
}

#[test]
fn equal_endpoint_ports_produce_closed_edge_candidates() {
    let ports = [[10, 10], [10, 11]];
    let candidates = [vec![[0, 0], [1, 1], [2, 2]], vec![[1, 3], [2, 4]]];
    assert_eq!(
        prune_edge_candidates_by_port_domains(&ports, &candidates),
        Some(vec![vec![[1, 1], [2, 2]], vec![[1, 3], [2, 4]]])
    );
    assert_eq!(
        prune_edge_candidates_by_port_domains(&[[10, 10]], &[vec![[0, 1], [0, 2]]]),
        None
    );
}

#[test]
fn endpoint_port_domains_propagate_pair_correlation_to_a_fixpoint() {
    let ports = [[10, 11], [11, 12], [12, 13]];
    let candidates = [vec![[0, 1], [2, 3]], vec![[1, 4], [3, 5]], vec![[4, 6]]];

    assert_eq!(
        prune_edge_candidates_by_port_domains(&ports, &candidates),
        Some(vec![vec![[0, 1]], vec![[1, 4]], vec![[4, 6]]])
    );
}

#[test]
fn deferred_port_rows_do_not_constrain_open_face_components() {
    let ports = [[10, 11], [11, 12], [12, 13]];
    let candidates = [vec![[0, 1]], vec![[1, 2]], vec![[3, 4]]];

    assert_eq!(
        prune_edge_candidates_by_port_domains(&ports, &candidates),
        None
    );
    assert_eq!(
        prune_edge_candidates_by_port_domains_with_deferred(
            &ports,
            &candidates,
            &[false, true, true],
        ),
        Some(candidates.to_vec())
    );
}

#[test]
fn mesh_endpoint_validation_accepts_equal_points_only_for_closed_ports() {
    assert!(mesh_edge_points_compatible(true, &[[2, 2]], [2, 2]));
    assert!(!mesh_edge_points_compatible(false, &[[2, 2]], [2, 2]));
    assert!(!mesh_edge_points_compatible(true, &[[1, 1]], [2, 2]));
}

#[test]
fn quotient_merges_roots_forced_to_one_coordinate_identity() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [0, 1, 0, 2]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };

    assert!(quotient.merge_singleton_coordinate_roots(&[Vec::new(), Vec::new()]));
    assert_eq!(quotient.root_count(), 3);
    assert_eq!(quotient.union.find(0), quotient.union.find(2));
}

#[test]
fn singleton_coordinate_root_merges_are_batched() {
    const ROOT_COUNT: usize = 10_000;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains: repeated_domain(HashSet::from([0]), ROOT_COUNT),
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); ROOT_COUNT / 2];

    assert!(quotient.merge_singleton_coordinate_roots(&candidates));
    assert_eq!(quotient.root_count(), 1);
}

#[test]
fn quotient_closes_coordinate_roots_forced_by_joint_edge_pairs() {
    let all = Arc::new(HashSet::from([0, 1, 2]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: vec![all.clone(); 6],
        members: (0..6).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared first corner");
    quotient.merge(3, 4).expect("shared second corner");
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];

    let assignment = quotient
        .close_coordinate_roots(3, &candidates, None)
        .expect("unique joint coordinate closure");

    assert_eq!(quotient.root_count(), 3);
    assert_eq!(quotient.union.find(0), quotient.union.find(5));
    assert_eq!(assignment[&quotient.union.find(0)], 0);
    assert_eq!(assignment[&quotient.union.find(1)], 1);
    assert_eq!(assignment[&quotient.union.find(3)], 2);
    for node in 0..6 {
        let root = quotient.union.find(node);
        assert_eq!(quotient.domains[root].len(), 1);
        assert_eq!(quotient.domains[root].iter().next(), assignment.get(&root));
    }
}

#[test]
fn quotient_coordinate_closure_declines_when_its_work_budget_is_exhausted() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: (0..2).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(0);

    assert!(quotient
        .close_coordinate_roots(1, &[vec![]], Some(&budget))
        .is_none());
    assert!(budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_does_not_rescan_assigned_roots() {
    const ROOT_COUNT: usize = 100;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains: repeated_domain(HashSet::from([0]), ROOT_COUNT),
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(2 * ROOT_COUNT + 1);

    let assignment = quotient
        .close_coordinate_roots(1, &[], Some(&budget))
        .expect("forced coordinate closure");

    assert_eq!(quotient.root_count(), 1);
    assert_eq!(assignment.values().copied().collect::<Vec<_>>(), [0]);
    assert!(!budget.exhausted());
}

#[test]
fn quotient_incidence_closure_updates_face_degrees_incrementally() {
    const EDGE_COUNT: usize = 64;
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2 * EDGE_COUNT),
        domains: (0..EDGE_COUNT)
            .flat_map(|edge| [singleton(edge), singleton((edge + 1) % EDGE_COUNT)])
            .collect(),
        members: (0..2 * EDGE_COUNT).map(|node| vec![node]).collect(),
    };
    for edge in 0..EDGE_COUNT {
        quotient
            .merge(edge * 2 + 1, ((edge + 1) % EDGE_COUNT) * 2)
            .expect("adjacent boundary ports share one coordinate root");
    }
    let edge_candidates = vec![Vec::new(); EDGE_COUNT];
    let edge_faces = vec![[0, 0]; EDGE_COUNT];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(
        (0..EDGE_COUNT).collect(),
    )];
    let budget = WorkBudget::new(45_000);

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            EDGE_COUNT,
            &edge_candidates,
            &edge_faces,
            1,
            &domains,
            Some(&budget),
        )
        .is_some());
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_enforces_sparse_endpoint_membership_before_search() {
    const EDGE_COUNT: usize = 50;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(EDGE_COUNT * 2),
        domains: repeated_domain(HashSet::from([0, 1]), EDGE_COUNT * 2),
        members: (0..EDGE_COUNT * 2).map(|node| vec![node]).collect(),
    };
    let candidates = (0..EDGE_COUNT)
        .map(|edge| vec![[edge % 2, edge % 2]])
        .collect::<Vec<_>>();
    let budget = WorkBudget::new(1_000);

    let assignment = quotient
        .close_coordinate_roots(2, &candidates, Some(&budget))
        .expect("arc-consistent coordinate closure");

    assert_eq!(quotient.root_count(), 2);
    assert_eq!(
        assignment.values().copied().collect::<HashSet<_>>(),
        HashSet::from([0, 1])
    );
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_propagates_edge_arc_consistency_to_a_fixpoint() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: repeated_domain(HashSet::from([0, 1]), 6),
        members: (0..6).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared relation root");
    let candidates = vec![vec![[0, 0], [1, 1]], vec![[0, 0]], vec![[1, 1]]];
    let budget = WorkBudget::new(1_000);

    let assignment = quotient
        .close_coordinate_roots(2, &candidates, Some(&budget))
        .expect("arc-consistent coordinate closure");

    assert_eq!(quotient.root_count(), 2);
    assert_eq!(assignment[&quotient.union.find(0)], 0);
    assert_eq!(assignment[&quotient.union.find(4)], 1);
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_forces_the_only_root_supporting_a_point() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1], vec![0], vec![0, 1, 2], vec![0]]
            .into_iter()
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(100);

    let assignment = quotient
        .close_coordinate_roots(3, &[Vec::new(), Vec::new()], Some(&budget))
        .expect("point-support-forced coordinate closure");

    assert_eq!(quotient.root_count(), 3);
    assert_eq!(assignment[&quotient.union.find(0)], 1);
    assert_eq!(assignment[&quotient.union.find(1)], 0);
    assert_eq!(assignment[&quotient.union.find(2)], 2);
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_rejects_a_coordinate_support_hall_conflict() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: [vec![0, 1, 2, 3], vec![0, 1, 2, 3], vec![3], vec![3]]
            .into_iter()
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .collect(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let budget = WorkBudget::new(1_000);

    assert!(quotient
        .close_coordinate_roots(4, &[Vec::new(), Vec::new()], Some(&budget))
        .is_none());
    assert!(!budget.exhausted());
}

#[test]
fn coordinate_support_matching_exposes_essential_and_unsupported_hall_edges() {
    let supports = [vec![0, 1], vec![0, 1], vec![0, 1, 2], vec![2, 3]];
    let matching = crate::solve::matching::distinct_domain_matching_with_budget(
        supports.iter().map(Vec::as_slice),
        4,
        None,
        None,
    )
    .expect("coordinate support matching");

    assert_eq!(matching[2], 2);
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            supports.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Exclude(
                2,
                matching[2]
            )),
        )
        .is_none()
    );

    let partitioned = [vec![0, 1, 2], vec![0, 1], vec![2, 3], vec![2, 3]];
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            partitioned.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Require(
                0, 2
            )),
        )
        .is_none()
    );
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            partitioned.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Require(
                0, 0
            )),
        )
        .is_some()
    );
}

#[test]
fn quotient_coordinate_closure_enforces_complete_face_degrees() {
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut open = MeshQuotient {
        union: UnionFind::new(4),
        domains: [singleton(0), singleton(1), singleton(0), singleton(2)].into(),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); 2];
    let edge_faces = [[0, 0]; 2];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1])];
    let budget = WorkBudget::new(1_000);
    open.merge(0, 2).expect("shared endpoint");

    assert!(open
        .close_coordinate_roots_for_incidence_with_budget(
            3,
            &candidates,
            &edge_faces,
            1,
            &domains,
            Some(&budget),
        )
        .is_none());

    let mut closed = MeshQuotient {
        union: UnionFind::new(6),
        domains: [
            singleton(0),
            singleton(1),
            singleton(1),
            singleton(2),
            singleton(2),
            singleton(0),
        ]
        .into(),
        members: (0..6).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); 3];
    let edge_faces = [[0, 0]; 3];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];

    assert!(closed
        .close_coordinate_roots_for_incidence_with_budget(
            3,
            &candidates,
            &edge_faces,
            1,
            &domains,
            Some(&WorkBudget::new(1_000)),
        )
        .is_some());
}

#[test]
fn quotient_coordinate_closure_rejects_sealed_unordered_subcycles() {
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(8),
        domains: [
            singleton(0),
            singleton(1),
            singleton(1),
            singleton(0),
            Arc::new(HashSet::from([0, 2])),
            singleton(3),
            singleton(3),
            singleton(2),
        ]
        .into(),
        members: (0..8).map(|node| vec![node]).collect(),
    };
    let candidates = vec![vec![[0, 1]], vec![[0, 1]], vec![[2, 3]], vec![[2, 3]]];
    let edge_faces = [[0, 0]; 4];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3])];
    let budget = WorkBudget::new(10_000);

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &domains,
            Some(&budget),
        )
        .is_none());
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_enforces_ordered_face_cycles() {
    let singleton = |point| Arc::new(HashSet::from([point]));
    let quotient = || MeshQuotient {
        union: UnionFind::new(8),
        domains: [
            singleton(0),
            singleton(1),
            singleton(2),
            singleton(3),
            singleton(1),
            singleton(2),
            singleton(3),
            singleton(0),
        ]
        .into(),
        members: (0..8).map(|node| vec![node]).collect(),
    };
    let candidates = vec![Vec::new(); 4];
    let edge_faces = [[0, 0]; 4];
    let domain = |order: [usize; 4]| {
        [MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![order
                    .into_iter()
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    })
                    .collect()],
            },
        ])]
    };

    assert!(quotient()
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &domain([0, 1, 2, 3]),
            Some(&WorkBudget::new(10_000)),
        )
        .is_none());
    assert!(quotient()
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &domain([0, 2, 1, 3]),
            Some(&WorkBudget::new(10_000)),
        )
        .is_some());

    let fixed = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![[0, 2, 1, 3]
                .into_iter()
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: Some(edge == 2),
                })
                .collect()],
        },
    ])];
    assert!(quotient()
        .close_coordinate_roots_for_incidence_with_budget(
            4,
            &candidates,
            &edge_faces,
            1,
            &fixed,
            Some(&WorkBudget::new(10_000)),
        )
        .is_none());
}

#[test]
fn quotient_closes_independent_coordinate_components_with_local_budgets() {
    const COMPONENT_COUNT: usize = 100;
    let point_count = COMPONENT_COUNT * 3;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(COMPONENT_COUNT * 6),
        domains: (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
        members: (0..COMPONENT_COUNT * 6).map(|node| vec![node]).collect(),
    };
    let mut candidates = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        quotient
            .merge(node + 1, node + 2)
            .expect("shared first corner");
        quotient
            .merge(node + 3, node + 4)
            .expect("shared second corner");
        candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point, point + 2]],
        ]);
    }

    let assignment = quotient
        .close_coordinate_roots(point_count, &candidates, None)
        .expect("independent coordinate closures");

    assert_eq!(quotient.root_count(), point_count);
    assert_eq!(assignment.len(), point_count);
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        assert_eq!(quotient.union.find(node), quotient.union.find(node + 5));
    }
}

#[test]
fn quotient_counts_global_face_incidence_once_across_coordinate_components() {
    const COMPONENT_COUNT: usize = 40;
    let point_count = COMPONENT_COUNT * 3;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(COMPONENT_COUNT * 6),
        domains: (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
        members: (0..COMPONENT_COUNT * 6).map(|node| vec![node]).collect(),
    };
    let mut candidates = Vec::new();
    let mut edge_faces = Vec::new();
    let mut domains = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        let first_edge = candidates.len();
        quotient
            .merge(node + 1, node + 2)
            .expect("shared first corner");
        quotient
            .merge(node + 3, node + 4)
            .expect("shared second corner");
        candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point, point + 2]],
        ]);
        edge_faces.extend([[component, component]; 3]);
        domains.push(MeshFaceBoundaryDomain::Ordered(vec![
            MeshFaceBoundaryAssignment {
                boundaries: vec![(first_edge..first_edge + 3)
                    .map(|edge| MeshBoundaryEdgeCandidate {
                        edge,
                        start: 0,
                        end: 0,
                        reversed: None,
                    })
                    .collect()],
            },
        ]));
    }
    let budget = WorkBudget::new(20_000);

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            point_count,
            &candidates,
            &edge_faces,
            COMPONENT_COUNT,
            &domains,
            Some(&budget),
        )
        .is_some());
    assert!(!budget.exhausted());
}

#[test]
fn quotient_closure_does_not_budget_forced_component_depth() {
    const ROOT_COUNT: usize = 10_000;
    let mut quotient = MeshQuotient {
        union: UnionFind::new(ROOT_COUNT),
        domains: repeated_domain(HashSet::from([0]), ROOT_COUNT),
        members: (0..ROOT_COUNT).map(|node| vec![node]).collect(),
    };
    let candidates = vec![vec![[0, 0]]; ROOT_COUNT / 2];

    let assignment = quotient
        .close_coordinate_roots(1, &candidates, None)
        .expect("forced coordinate component");

    assert_eq!(quotient.root_count(), 1);
    assert_eq!(assignment.values().copied().collect::<Vec<_>>(), [0]);
}

#[test]
fn quotient_does_not_guess_an_ambiguous_coordinate_closure() {
    let all = Arc::new(HashSet::from([0, 1]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: vec![all.clone(); 4],
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared middle corner");

    assert!(quotient
        .close_coordinate_roots(2, &[vec![[0, 1]], vec![[0, 1]]], None)
        .is_none());
    assert_eq!(quotient.root_count(), 3);
}

#[test]
fn quotient_closure_requires_every_coordinate_row_in_a_domain() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(4),
        domains: repeated_domain(HashSet::from([0]), 4),
        members: (0..4).map(|node| vec![node]).collect(),
    };
    quotient.merge(1, 2).expect("shared endpoint");

    assert!(quotient
        .close_coordinate_roots(2, &[vec![[0, 0]], vec![[0, 0]]], None)
        .is_none());
    assert_eq!(quotient.root_count(), 3);
}

#[test]
fn quotient_accepts_diagonal_domain_for_closed_edge() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![Arc::new(HashSet::from([2])), Arc::new(HashSet::from([2]))],
        members: vec![vec![0], vec![1]],
    };
    quotient.merge(0, 1).expect("closed endpoint merge");
    assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
    assert!(!quotient.edge_domains_viable(&[vec![[1, 2]]]));
}

#[test]
fn quotient_point_assignment_accepts_a_closed_diagonal_edge() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: repeated_domain(HashSet::from([0]), 2),
        members: vec![vec![0], vec![1]],
    };
    let root = quotient.merge(0, 1).expect("closed endpoint merge");

    assert_eq!(
        quotient.point_assignment(1, &[vec![[0, 0]]], None),
        Some(HashMap::from([(root, 0)]))
    );
}

#[test]
fn quotient_retains_diagonal_pairs_until_ports_are_merged() {
    let mut quotient = MeshQuotient {
        union: UnionFind::new(2),
        domains: vec![
            Arc::new(HashSet::from([1, 2])),
            Arc::new(HashSet::from([1, 2])),
        ],
        members: vec![vec![0], vec![1]],
    };

    assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
    assert_eq!(
        quotient.domains,
        vec![Arc::new(HashSet::from([2])), Arc::new(HashSet::from([2]))]
    );
    quotient.merge(0, 1).expect("closed endpoint merge");
    assert!(quotient.edge_domains_viable(&[vec![[2, 2]]]));
}

#[test]
fn closed_edge_is_a_single_coedge_boundary_on_each_incident_face() {
    let topology = reconstruct_incidence(
        vec![EdgeRow {
            kind: 0,
            handles: vec![7, 7],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        }],
        vec![[1.0, 0.0, 0.0]],
        &[[0, 1]],
        &[[0, 0]],
        2,
    )
    .expect("closed radial edge");
    assert!(topology
        .faces()
        .iter()
        .all(|face| face.boundaries.len() == 1 && face.boundaries[0].coedges.len() == 1));
    assert_ne!(
        topology.faces()[0].boundaries[0].coedges[0].reversed,
        topology.faces()[1].boundaries[0].coedges[0].reversed
    );
}

#[test]
fn duplicate_face_reference_slot_is_completed_by_face_closure() {
    let rows = (0..3)
        .map(|handle| EdgeRow {
            kind: 0,
            handles: vec![handle],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let faces = complete_duplicate_face_slots(
        &rows,
        &[[0, 1], [0, 1], [0, 0]],
        &[[0, 1], [1, 2], [2, 0]],
        2,
        None,
        Some(&[]),
    )
    .expect("unique face-closing slot assignment");

    assert_eq!(faces, vec![[0, 1], [0, 1], [0, 1]]);
}

#[test]
fn duplicate_face_completion_keeps_sparse_endpoint_identities() {
    let rows = (0..3)
        .map(|handle| EdgeRow {
            kind: 0,
            handles: vec![handle],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let faces = complete_duplicate_face_slots(
        &rows,
        &[[0, 1], [0, 1], [0, 0]],
        &[
            [usize::MAX, usize::MAX - 1],
            [usize::MAX - 1, 0],
            [0, usize::MAX],
        ],
        2,
        None,
        Some(&[]),
    )
    .expect("sparse endpoint identities");

    assert_eq!(faces, vec![[0, 1], [0, 1], [0, 1]]);
}

#[test]
fn vertex_table_rejects_unbacked_extended_count_before_allocation() {
    assert!(parse_vertex_table(&[0x01, 0x06, 0xff, 0xff, 0xff, 0xff, 0xff], 0).is_none());
}

#[test]
fn vertex_table_rejects_an_overflowing_start_offset() {
    assert!(parse_vertex_table(&[], usize::MAX).is_none());
}

#[test]
fn mesh_assignment_endpoint_cycles_reject_crossed_edge_order() {
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: None,
    };
    let assignment = |edges: &[usize]| MeshFaceBoundaryAssignment {
        boundaries: vec![edges.iter().copied().map(use_).collect()],
    };
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]];

    assert!(mesh_assignment_endpoint_cycles_viable(
        &assignment(&[0, 1, 2, 3]),
        &candidates,
    ));
    assert!(!mesh_assignment_endpoint_cycles_viable(
        &assignment(&[0, 2, 1, 3]),
        &candidates,
    ));
}

#[test]
fn quotient_ordered_cycles_use_physical_ports_for_sorted_pairs() {
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient {
        union: UnionFind::new(6),
        domains: [
            singleton(1),
            singleton(2),
            singleton(1),
            singleton(0),
            singleton(0),
            singleton(2),
        ]
        .into(),
        members: (0..6).map(|node| vec![node]).collect(),
    };
    quotient.merge(4, 3).expect("first fixed-direction corner");
    quotient.merge(2, 0).expect("second fixed-direction corner");
    quotient.merge(1, 5).expect("third boundary corner");
    let candidates = vec![vec![[1, 2]], vec![[0, 1]], vec![[0, 2]]];
    let domain = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![
                MeshBoundaryEdgeCandidate {
                    edge: 2,
                    start: 0,
                    end: 0,
                    reversed: Some(true),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 1,
                    start: 0,
                    end: 0,
                    reversed: Some(true),
                },
                MeshBoundaryEdgeCandidate {
                    edge: 0,
                    start: 0,
                    end: 0,
                    reversed: None,
                },
            ]],
        },
    ])];

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            3,
            &candidates,
            &[[0, 0]; 3],
            1,
            &domain,
            Some(&WorkBudget::new(10_000)),
        )
        .is_some());
}

#[test]
fn mesh_assignment_endpoint_cycles_index_incident_candidates() {
    let dense = (0..10)
        .flat_map(|left| ((left + 1)..10).map(move |right| [left, right]))
        .collect::<Vec<_>>();
    let candidates = vec![dense.clone(), dense.clone(), dense];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..3)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: None,
            })
            .collect()],
    };
    // The slice covers the indexed state transitions and the 3 * 45
    // candidate pairs used to build the three adjacency maps.
    let adjacency_work = candidates.len() * candidates[0].len();
    let budget = WorkBudget::new(1_800 + adjacency_work);

    assert_eq!(
        crate::solve::mesh_quotient::mesh_assignment_endpoint_cycles_viable_where(
            &assignment,
            &candidates,
            Some(&budget),
            |_, _| true,
        ),
        Some(true),
    );
    assert!(!budget.exhausted());
}

#[test]
fn mesh_assignment_endpoint_cycle_support_removes_open_layered_paths() {
    let candidates = [vec![[0, 1], [0, 2]], vec![[1, 3], [2, 4]], vec![[0, 3]]];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..3)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: None,
            })
            .collect()],
    };
    let budget = WorkBudget::new(1_000);
    let support = crate::solve::mesh_quotient::mesh_assignment_endpoint_cycle_support_by(
        &assignment,
        Some(&budget),
        |edge| {
            candidates
                .get(edge)
                .map(|values| crate::solve::mesh_quotient::MeshEndpointCandidates::Explicit(values))
        },
        |_, _| true,
    )
    .expect("bounded layered-cycle support");

    assert_eq!(support[&0], HashSet::from([[0, 1]]));
    assert_eq!(support[&1], HashSet::from([[1, 3]]));
    assert_eq!(support[&2], HashSet::from([[0, 3]]));
    assert!(!budget.exhausted());
}

#[test]
fn mesh_assignment_endpoint_cycle_support_requires_one_complete_traversal() {
    let candidates = [vec![[0, 1]]];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let support = crate::solve::mesh_quotient::mesh_assignment_endpoint_cycle_support_by(
        &assignment,
        None,
        |edge| {
            candidates
                .get(edge)
                .map(|values| crate::solve::mesh_quotient::MeshEndpointCandidates::Explicit(values))
        },
        |_, _| true,
    )
    .expect("one-layer support");

    assert!(support.is_empty());
}

#[test]
fn ordered_face_cycle_support_materializes_only_supported_implicit_pairs() {
    let mut choices = vec![vec![[0, 1], [0, 2]], Vec::new(), vec![[0, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &choices,
        4,
        &[[10, 11], [12, 13], [14, 15]],
    )
    .expect("initial quotient");
    let coordinate_domains = quotient
        .prepare_coordinate_root_domains(4, &choices, None)
        .expect("implicit coordinate domains");
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..3)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: None,
            })
            .collect()],
    };
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![assignment])];
    let budget = WorkBudget::new(10_000);

    assert!(
        crate::solve::incidence::prune_implicit_ordered_face_endpoint_support(
            &domains,
            &mut choices,
            &coordinate_domains,
            &budget,
        )
    );
    assert_eq!(choices[1], vec![[1, 3], [2, 3]]);
    assert!(!budget.exhausted());
}

#[test]
fn mesh_face_endpoint_configurations_preserve_pair_correlation() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..4)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: None,
            })
            .collect()],
    };
    let candidates = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let budget = WorkBudget::new(4_096);
    let configurations = mesh_face_endpoint_configurations(
        std::slice::from_ref(&assignment),
        &candidates,
        &[None; 4],
        &budget,
    )
    .expect("bounded face configurations");

    assert_eq!(
        configurations,
        vec![vec![(0, [0, 1]), (1, [1, 2]), (2, [2, 3]), (3, [0, 3])]],
    );

    let exhausted = WorkBudget::new(1);
    assert!(
        mesh_face_endpoint_configurations(&[assignment], &candidates, &[None; 4], &exhausted)
            .is_none()
    );
    assert!(exhausted.exhausted());
}

#[test]
fn mesh_assignment_endpoint_cycles_preserve_unconstrained_boundaries() {
    let assignment = MeshFaceBoundaryAssignment {
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
    };
    assert!(mesh_assignment_endpoint_cycles_viable(
        &assignment,
        &[vec![[0, 1]], Vec::new()],
    ));
}

#[test]
fn mesh_endpoint_pair_support_propagates_across_incident_faces() {
    let assignment = |edges: &[usize]| MeshFaceBoundaryAssignment {
        boundaries: vec![edges
            .iter()
            .copied()
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: None,
            })
            .collect()],
    };
    let mut assignments = vec![
        vec![assignment(&[0, 1, 2])],
        vec![assignment(&[0, 3, 4]), assignment(&[0, 5, 6])],
    ];
    let mut candidates = vec![
        vec![[0, 1], [0, 3]],
        vec![[1, 2]],
        vec![[2, 0]],
        vec![[1, 4]],
        vec![[4, 0]],
        vec![[3, 5]],
        vec![[5, 0]],
    ];

    assert!(prune_mesh_endpoint_pair_support(
        &mut assignments,
        &mut candidates,
    ));
    assert_eq!(candidates[0], vec![[0, 1]]);
    assert_eq!(assignments[1], vec![assignment(&[0, 3, 4])]);
}

#[test]
fn mesh_endpoint_pair_support_does_not_treat_budget_exhaustion_as_a_contradiction() {
    let mut assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    }]];
    let mut candidates = vec![vec![[0, 0]]];

    assert!(prune_mesh_endpoint_pair_support_with_limit(
        &mut assignments,
        &mut candidates,
        0,
    ));
}

#[test]
fn independent_duplicate_face_slots_have_one_canonical_search_order() {
    let rows = (0..12)
        .map(|edge| EdgeRow {
            kind: 1,
            handles: vec![edge],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        })
        .collect::<Vec<_>>();
    let serialized = vec![[0, 0]; rows.len()];
    let points = (0..rows.len())
        .map(|point| [point, point])
        .collect::<Vec<_>>();

    let completed = complete_duplicate_face_slots(&rows, &serialized, &points, 2, None, None)
        .expect("independent closed edges have one face completion");
    assert_eq!(completed, vec![[0, 1]; rows.len()]);
}

#[test]
fn duplicate_face_slot_requires_one_joint_carrier_and_mesh_assignment() {
    let serialized = [[0, 0], [0, 1], [1, 1]];
    let allowed = [vec![1, 2], Vec::new(), vec![0, 2]];
    let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 3, |faces| {
        faces == [[0, 2], [0, 1], [1, 0]]
    })
    .expect("one complete assignment");
    assert_eq!(resolved, [[0, 2], [0, 1], [1, 0]]);

    assert!(unique_duplicate_face_assignment(&serialized, &allowed, 3, |_| true).is_none());
    assert!(unique_duplicate_face_assignment(
        &serialized,
        &[vec![3], Vec::new(), vec![0]],
        3,
        |_| true,
    )
    .is_none());
}

#[test]
fn duplicate_face_slot_without_admitted_alternate_remains_unresolved() {
    let serialized = [[0, 0], [0, 0]];
    let allowed = [Vec::new(), vec![1]];

    let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 2, |faces| {
        faces == [[0, 0], [0, 1]]
    })
    .expect("the unresolved same-face slot remains in the joint assignment");

    assert_eq!(resolved, [[0, 0], [0, 1]]);
}

#[test]
fn duplicate_face_assignment_visitor_keeps_alternates_correlated() {
    let serialized = [[0, 0], [1, 1], [0, 2]];
    let allowed = [vec![2, 1, 0], Vec::new(), Vec::new()];
    let mut assignments = Vec::new();

    let outcome = visit_duplicate_face_assignments(&serialized, &allowed, 3, 4, |assignment| {
        assignments.push(assignment.to_vec());
        true
    });

    assert_eq!(outcome, Some(DuplicateFaceAssignmentVisit::Complete));
    assert_eq!(
        assignments,
        vec![
            vec![[0, 0], [1, 1], [0, 2]],
            vec![[0, 1], [1, 1], [0, 2]],
            vec![[0, 2], [1, 1], [0, 2]],
        ]
    );
}

#[test]
fn duplicate_face_assignment_visitor_reports_the_bound() {
    let serialized = [[0, 0], [0, 0]];
    let allowed = [vec![1, 2], vec![1, 2]];
    let mut visits = 0;

    let outcome = visit_duplicate_face_assignments(&serialized, &allowed, 3, 3, |_| {
        visits += 1;
        true
    });

    assert_eq!(outcome, Some(DuplicateFaceAssignmentVisit::Exhausted));
    assert_eq!(visits, 3);
}

#[test]
fn one_admitted_alternate_does_not_force_a_second_face() {
    const EDGE_COUNT: usize = 8;
    let serialized = vec![[0, 0]; EDGE_COUNT];
    let allowed = vec![vec![1, 1]; EDGE_COUNT];

    assert!(unique_duplicate_face_assignment(&serialized, &allowed, 2, |_| true).is_none());
}

#[test]
fn counted_edge_arities_are_bounded_by_remaining_bytes() {
    let oversized_row = [0x01, 0x01, 0x01, 0x02, 0xff, 0xff, 0xff, 0xff, 0xff];
    assert!(parse_edge_tables_scoped_at(&oversized_row, 0).is_none());
    assert!(parse_fbb_edge_tables_width(&oversized_row, 0, 3).is_none());
}

#[test]
fn fbb_edge_width_requires_a_complete_counted_vertex_table() {
    let mut bytes = Vec::new();
    for kind in [1, 2] {
        bytes.extend_from_slice(&[0x01, kind, 0x01, 0x02, 0x02]);
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
        bytes.extend_from_slice(&EDGE_DELIMITER);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0x01]);

    assert!(parse_fbb_edge_tables_width(&bytes, 0, 3).is_some());
    assert!(parse_fbb_edge_tables(&bytes, 0).is_none());
}

#[test]
fn standard_edge_tables_use_the_fixed_u16_width_when_rows_are_empty() {
    let mut bytes = vec![
        0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2, 0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2,
        0xd2,
    ];
    for kind in [1, 2] {
        bytes.extend_from_slice(&[0x01, kind, 0x00]);
        bytes.extend_from_slice(&EDGE_DELIMITER);
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0x03]);
    for xyz in [[0.0f32, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in xyz {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let points = crate::families::standard::fbb::standard_vertex_points(&bytes)
        .expect("standard vertex table");
    assert_eq!(points.len(), 3);
}

#[test]
fn trim_primitive_counts_are_bounded_by_remaining_bytes() {
    let oversized_primitives = [
        0x01, 0x46, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00,
        0x00,
    ];
    assert!(parse_trim_record(&oversized_primitives, 0, 2).is_none());
}

#[test]
fn duplicate_face_completion_rejects_out_of_range_faces() {
    let rows = vec![EdgeRow {
        kind: 0,
        handles: vec![0, 1],
        boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
    }];
    assert!(complete_duplicate_face_slots(&rows, &[[0, 2]], &[[0, 1]], 2, None, None,).is_none());
}

#[test]
fn exact_mesh_occurrences_complete_duplicate_face_slot() {
    let run = |edge, face| MeshEdgeRun {
        edge,
        face,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };
    let faces = resolve_edge_faces_from_runs(
        &[[1, 1], [2, 2], [3, 4]],
        &[run(0, 1), run(0, 5), run(1, 2), run(2, 3), run(2, 4)],
    )
    .expect("consistent exact face occurrences");

    assert_eq!(faces, vec![[1, 5], [2, 2], [3, 4]]);
}

#[test]
fn one_mesh_occurrence_keeps_duplicate_face_slot_unresolved() {
    let run = MeshEdgeRun {
        edge: 0,
        face: 1,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };

    let faces = resolve_edge_faces_from_runs(&[[1, 1]], &[run])
        .expect("a single occurrence does not conflict with the serialized wildcard");

    assert_eq!(faces, vec![[1, 1]]);
}

#[test]
fn ambiguous_mesh_occurrences_defer_duplicate_face_slot() {
    let run = |face| MeshEdgeRun {
        edge: 0,
        face,
        cycle: 0,
        start: 0,
        segment_count: 1,
        reversed: false,
    };

    let faces = resolve_edge_faces_from_runs(&[[1, 1]], &[run(1), run(5), run(6)])
        .expect("ambiguous occurrences remain a deferred face domain");

    assert_eq!(faces, vec![[1, 1]]);
}

#[test]
fn equivalent_edge_rows_share_one_incidence_assignment_gauge() {
    let rows = vec![
        EdgeRow {
            kind: 0,
            handles: vec![0],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![1],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![2, 3],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
        EdgeRow {
            kind: 0,
            handles: vec![4, 5],
            boundary_layout: EdgeBoundaryLayout::CompleteBoundaryRun,
        },
    ];
    let faces = complete_duplicate_face_slots(
        &rows,
        &[[0, 1], [0, 1], [2, 2], [2, 2]],
        &[[0, 1], [1, 2], [2, 0], [0, 2]],
        3,
        Some(&[0, 1, 2, 2]),
        None,
    )
    .expect("one assignment modulo equivalent edge rows");

    let mut assigned = [faces[2][1], faces[3][1]];
    assigned.sort_unstable();
    assert_eq!(assigned, [0, 1]);
}

#[test]
fn endpoint_ports_reject_contradictory_pair_constraints() {
    let ports = [[10, 11], [11, 12], [12, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), Some([0, 3])];
    assert_eq!(propagate_edge_port_points(&ports, &pairs), None);
}

#[test]
fn native_edge_identities_bind_ambiguous_coordinate_pairs() {
    let ports = [[10, 11], [12, 13], [10, 12], [11, 13]];
    let candidates = [vec![[0, 1]], vec![[2, 3]], vec![[0, 2]], vec![[1, 3]]];
    assert_eq!(
        bind_edge_port_candidates(&ports, &candidates),
        Some(vec![[0, 1], [2, 3], [0, 2], [1, 3]])
    );
}

#[test]
fn mesh_edge_ports_allow_one_coordinate_row_at_multiple_ports() {
    let ports = [[10, 11], [12, 13]];
    let candidates = [vec![[0, 1]], vec![[0, 2]]];

    assert_eq!(
        unique_mesh_edge_port_candidate_pairs(&ports, &candidates),
        Some(vec![[0, 1], [0, 2]])
    );
}

#[test]
fn mesh_edge_ports_reject_multiple_unordered_assignments() {
    let ports = [[10, 11]];
    let candidates = [vec![[0, 1], [0, 2]]];

    assert_eq!(
        unique_mesh_edge_port_candidate_pairs(&ports, &candidates),
        None
    );
}

#[test]
fn mesh_edge_ports_resolve_shared_port_without_point_bijection() {
    let ports = [[10, 11], [10, 12], [11, 13]];
    let candidates = [vec![[0, 1]], vec![[0, 2]], vec![[1, 3]]];

    assert_eq!(
        unique_mesh_edge_port_candidate_pairs(&ports, &candidates),
        Some(vec![[0, 1], [0, 2], [1, 3]])
    );
}

#[test]
fn deferred_mesh_edge_ports_do_not_constrain_settled_rows() {
    let ports = [[10, 11], [20, 21]];
    let candidates = [vec![[0, 1]], vec![[2, 3]]];

    assert_eq!(
        unique_mesh_edge_port_candidate_pairs_with_deferred(&ports, &candidates, &[false, true],),
        Some(vec![Some([0, 1]), None])
    );
}

#[test]
fn deferred_mesh_edge_port_components_leave_all_connected_rows_unresolved() {
    let ports = [[10, 11], [11, 10], [12, 10], [13, 11], [10, 97], [11, 98]];
    let candidates = [
        vec![[0, 1]],
        vec![[0, 1]],
        vec![[0, 2]],
        vec![[0, 3], [1, 3]],
        vec![[0, 79]],
        vec![[0, 79]],
    ];

    assert_eq!(
        unique_mesh_edge_port_candidate_pairs_with_deferred(
            &ports,
            &candidates,
            &[true, false, false, false, false, false],
        ),
        Some(vec![None, None, None, None, None, None]),
    );
}

#[test]
fn native_edge_identities_reject_multiple_coordinate_bijections() {
    let ports = [[10, 11]];
    let candidates = [vec![[0, 1], [2, 3]]];
    assert_eq!(bind_edge_port_candidates(&ports, &candidates), None);
}

#[test]
fn native_edge_identities_preserve_endpoint_equality() {
    assert_eq!(
        bind_edge_port_candidates(&[[10, 11]], &[vec![[0, 0]]]),
        None
    );
    assert_eq!(
        bind_edge_port_candidates(&[[10, 10]], &[vec![[0, 1]]]),
        None
    );
    assert_eq!(
        bind_edge_port_candidates(&[[10, 10]], &[vec![[0, 0]]]),
        Some(vec![[0, 0]])
    );
}

#[test]
fn native_edge_identities_bind_independent_components_with_local_budgets() {
    const COMPONENT_COUNT: usize = 100;
    let ports = (0..COMPONENT_COUNT)
        .map(|component| {
            let port = u32::try_from(component * 2).expect("bounded port identity");
            [port, port + 1]
        })
        .collect::<Vec<_>>();
    let candidates = (0..COMPONENT_COUNT)
        .map(|component| vec![[component * 2, component * 2 + 1]])
        .collect::<Vec<_>>();

    let solution =
        bind_edge_port_candidates(&ports, &candidates).expect("independent port components");

    assert_eq!(solution.len(), COMPONENT_COUNT);
    assert!(solution
        .iter()
        .zip(&candidates)
        .all(|(pair, candidates)| same_unordered_pair(*pair, candidates[0])));
}

#[test]
fn native_edge_identities_do_not_charge_forced_chain_depth() {
    const EDGE_COUNT: usize = 10_000;
    let ports = (0..EDGE_COUNT)
        .map(|edge| {
            let port = u32::try_from(edge).expect("bounded port identity");
            [port, port + 1]
        })
        .collect::<Vec<_>>();
    let candidates = (0..EDGE_COUNT)
        .map(|edge| vec![[edge, edge + 1]])
        .collect::<Vec<_>>();

    let solution =
        bind_edge_port_candidates(&ports, &candidates).expect("forced connected port chain");

    assert_eq!(
        solution,
        candidates.into_iter().flatten().collect::<Vec<_>>()
    );
}

#[test]
fn duplicate_coordinate_rows_have_one_geometric_bijection() {
    let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
    assert_eq!(
        unique_coordinate_bijection(&domains, &[[1.0, 2.0, 3.0], [1.0, 2.0, 3.0]]),
        Some(vec![0, 1])
    );
}

#[test]
fn forced_coordinate_bijection_has_no_recursive_depth_limit() {
    const POINT_COUNT: usize = 10_000;
    let domains = (0..POINT_COUNT)
        .map(|point| HashSet::from([point]))
        .collect::<Vec<_>>();
    let points = (0..POINT_COUNT)
        .map(|point| {
            [
                f64::from(u32::try_from(point).expect("bounded point index")),
                0.0,
                0.0,
            ]
        })
        .collect::<Vec<_>>();

    assert_eq!(
        unique_coordinate_bijection(&domains, &points),
        Some((0..POINT_COUNT).collect())
    );
}

#[test]
fn coordinate_bijection_respects_duplicate_class_capacity() {
    let domains = [
        HashSet::from([0, 2]),
        HashSet::from([0, 1]),
        HashSet::from([0, 1]),
    ];
    let points = [[1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];

    assert_eq!(
        unique_coordinate_bijection(&domains, &points),
        Some(vec![2, 0, 1])
    );
}

#[test]
fn distinct_coordinate_bijections_remain_ambiguous() {
    let domains = [HashSet::from([0, 1]), HashSet::from([0, 1])];
    assert_eq!(
        unique_coordinate_bijection(&domains, &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]),
        None
    );
}
