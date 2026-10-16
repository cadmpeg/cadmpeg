// SPDX-License-Identifier: Apache-2.0
use crate::families::standard::fbb::parse_vertex_table;
use crate::families::standard::topology::reconstruct_incidence;
use crate::families::standard::topology::EdgeBoundaryLayout;
use crate::families::standard::topology::EdgeRow;
use crate::solve::matching::unique_coordinate_bijection;
use crate::solve::mesh_quotient::mesh_assignment_endpoint_cycles_viable;
use crate::solve::mesh_quotient::mesh_face_endpoint_configurations;
use crate::solve::mesh_quotient::prune_mesh_endpoint_pair_support;
use crate::solve::mesh_quotient::prune_mesh_endpoint_pair_support_with_limit;
use crate::solve::mesh_quotient::selection_search::mesh_edge_points_compatible;
use crate::solve::mesh_quotient::MeshIncidenceBoundary;
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::missing_edge::bind_edge_port_candidates;
use crate::solve::missing_edge::expand_deferred_edge_port_components;
use crate::solve::missing_edge::propagate_edge_port_points;
use crate::solve::missing_edge::propagate_edge_port_points_with_ordered_seeds;
use crate::solve::missing_edge::propagate_edge_port_points_with_ordered_seeds_and_deferred;
use crate::solve::missing_edge::propagate_partial_edge_port_points_with_ordered_seeds;
use crate::solve::missing_edge::resolve_edge_faces_from_runs;
use crate::solve::missing_edge::same_unordered_pair;
use crate::solve::missing_edge::unique_duplicate_face_assignment;
use crate::solve::missing_edge::unique_mesh_edge_port_candidate_pairs;
use crate::solve::missing_edge::unique_mesh_edge_port_candidate_pairs_with_deferred;
use crate::solve::missing_edge::visit_duplicate_face_assignments;
use crate::solve::missing_edge::DuplicateFaceAssignmentVisit;
use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
use crate::solve::missing_edge::MeshEdgeRun;
use crate::solve::missing_edge::MeshFaceBoundaryAssignment;
use crate::solve::missing_edge::MeshFaceBoundaryDomain;
use crate::solve::tests::repeated_domain;
use cadmpeg_core::decode::WorkBudget;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn endpoint_ports_propagate_resolved_pairs_to_unresolved_edges() {
    let ports = [[10, 11], [11, 12], [12, 13], [13, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), None, Some([3, 0])];
    assert_eq!(
        crate::test_support::with_service_context(|ctx| propagate_edge_port_points(
            ctx, &ports, &pairs
        ))
        .expect("propagate edge port points"),
        Some(vec![Some([0, 1]), Some([1, 2]), Some([2, 3]), Some([3, 0]),])
    );
}

#[test]
fn ordered_endpoint_seed_orients_a_seedless_port_component() {
    let ports = [[10, 11]];
    let pairs = [Some([0, 1])];
    let ordered = [Some([1, 0])];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            propagate_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &ordered)
        })
        .expect("propagate edge port points with ordered seeds"),
        Some(vec![Some([1, 0])])
    );
}

#[test]
fn ordered_endpoint_seed_must_agree_with_the_unordered_candidate() {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            propagate_edge_port_points_with_ordered_seeds(
                ctx,
                &[[10, 11]],
                &[Some([0, 1])],
                &[Some([0, 2])],
            )
        })
        .expect("propagate edge port points with ordered seeds"),
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
        crate::test_support::with_service_context(|ctx| {
            propagate_edge_port_points_with_ordered_seeds_and_deferred(
                ctx,
                &ports,
                &pairs,
                &[],
                &[true, false, false, false, false, false],
            )
        })
        .expect("propagate edge port points with ordered seeds"),
        Some(vec![None, None, None, None, None, None]),
    );
}

#[test]
fn partial_ordered_endpoint_seed_resolves_a_row_without_native_ports() {
    let ports = [Some([10, 11]), None];
    let pairs = [Some([0, 1]), None];
    let ordered = [Some([1, 0]), Some([2, 3])];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            propagate_partial_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &ordered)
        })
        .expect("propagate partial edge port points with ordered seeds"),
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
        crate::test_support::with_service_context(|ctx| {
            propagate_partial_edge_port_points_with_ordered_seeds(ctx, &ports, &pairs, &[])
        })
        .expect("propagate partial edge port points with ordered seeds"),
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
fn endpoint_port_propagation_requires_a_point_bijection() {
    assert_eq!(
        crate::test_support::with_service_context(|ctx| propagate_edge_port_points(
            ctx,
            &[[10, 11]],
            &[Some([0, 1])]
        ))
        .expect("propagate edge port points"),
        Some(vec![Some([0, 1])])
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| propagate_edge_port_points(
            ctx,
            &[[10, 11], [10, 12]],
            &[Some([0, 1]), Some([0, 1])]
        ))
        .expect("propagate edge port points"),
        None
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| propagate_edge_port_points(
            ctx,
            &[[10, 11]],
            &[Some([0, 0])]
        ))
        .expect("propagate edge port points"),
        None
    );
}

#[test]
fn endpoint_port_propagation_closes_equal_port_edges() {
    let ports = [[10, 11], [11, 12], [10, 10]];
    let pairs = [Some([0, 1]), Some([1, 2]), None];

    assert_eq!(
        crate::test_support::with_service_context(|ctx| propagate_edge_port_points(
            ctx, &ports, &pairs
        ))
        .expect("propagate edge port points"),
        Some(vec![Some([0, 1]), Some([1, 2]), Some([0, 0])])
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
    let mut quotient = MeshQuotient::new(
        [0, 1, 0, 2]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
    );

    assert!(quotient
        .merge_singleton_coordinate_roots(&[Vec::new(), Vec::new()])
        .expect("merge singleton coordinate roots"));
    assert_eq!(quotient.root_count().expect("count the component roots"), 3);
    assert_eq!(
        quotient.find(0).expect("find the component root"),
        quotient.find(2).expect("find the component root")
    );
}

#[test]
fn singleton_coordinate_root_merges_are_batched() {
    const ROOT_COUNT: usize = 10_000;
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), ROOT_COUNT));
    let candidates = vec![Vec::new(); ROOT_COUNT / 2];

    assert!(quotient
        .merge_singleton_coordinate_roots(&candidates)
        .expect("merge singleton coordinate roots"));
    assert_eq!(quotient.root_count().expect("count the component roots"), 1);
}

#[test]
fn quotient_closes_coordinate_roots_forced_by_joint_edge_pairs() {
    catia_test_context!(ctx);
    let all = Arc::new(HashSet::from([0, 1, 2]));
    let mut quotient = MeshQuotient::new(vec![all.clone(); 6]);
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("shared first corner");
    quotient
        .merge(3, 4)
        .expect("merge the components")
        .expect("shared second corner");
    let candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[0, 2]]];

    let assignment = quotient
        .close_coordinate_roots(&ctx, 3, &candidates, None)
        .expect("close the coordinate roots")
        .expect("unique joint coordinate closure");

    assert_eq!(quotient.root_count().expect("count the component roots"), 3);
    assert_eq!(
        quotient.find(0).expect("find the component root"),
        quotient.find(5).expect("find the component root")
    );
    assert_eq!(
        assignment[&quotient.find(0).expect("find the component root")],
        0
    );
    assert_eq!(
        assignment[&quotient.find(1).expect("find the component root")],
        1
    );
    assert_eq!(
        assignment[&quotient.find(3).expect("find the component root")],
        2
    );
    for node in 0..6 {
        let root = quotient.find(node).expect("find the component root");
        assert_eq!(quotient.domains()[root].len(), 1);
        assert_eq!(
            quotient.domains()[root].iter().next(),
            assignment.get(&root)
        );
    }
}

#[test]
fn quotient_coordinate_closure_declines_when_its_work_budget_is_exhausted() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 2));
    let budget = WorkBudget::new(0);

    assert!(quotient
        .close_coordinate_roots(&ctx, 1, &[vec![]], Some(&budget))
        .expect("close the coordinate roots")
        .is_none());
    assert!(budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_does_not_rescan_assigned_roots() {
    const ROOT_COUNT: usize = 100;
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), ROOT_COUNT));
    let budget = WorkBudget::new(2 * ROOT_COUNT + 1);

    let assignment = quotient
        .close_coordinate_roots(&ctx, 1, &[], Some(&budget))
        .expect("close the coordinate roots")
        .expect("forced coordinate closure");

    assert_eq!(quotient.root_count().expect("count the component roots"), 1);
    assert_eq!(assignment.values().copied().collect::<Vec<_>>(), [0]);
    assert!(!budget.exhausted());
}

#[test]
fn quotient_incidence_closure_updates_face_degrees_incrementally() {
    const EDGE_COUNT: usize = 64;
    catia_test_context!(ctx);
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient::new(
        (0..EDGE_COUNT)
            .flat_map(|edge| [singleton(edge), singleton((edge + 1) % EDGE_COUNT)])
            .collect(),
    );
    for edge in 0..EDGE_COUNT {
        quotient
            .merge(edge * 2 + 1, ((edge + 1) % EDGE_COUNT) * 2)
            .expect("merge the components")
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
            &ctx,
            EDGE_COUNT,
            &edge_candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: 1,
                domains: &domains,
            },
            Some(&budget),
        )
        .expect("close the coordinate roots for incidence")
        .is_some());
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_enforces_sparse_endpoint_membership_before_search() {
    const EDGE_COUNT: usize = 50;
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), EDGE_COUNT * 2));
    let candidates = (0..EDGE_COUNT)
        .map(|edge| vec![[edge % 2, edge % 2]])
        .collect::<Vec<_>>();
    let budget = WorkBudget::new(1_000);

    let assignment = quotient
        .close_coordinate_roots(&ctx, 2, &candidates, Some(&budget))
        .expect("close the coordinate roots")
        .expect("arc-consistent coordinate closure");

    assert_eq!(quotient.root_count().expect("count the component roots"), 2);
    assert_eq!(
        assignment.values().copied().collect::<HashSet<_>>(),
        HashSet::from([0, 1])
    );
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_propagates_edge_arc_consistency_to_a_fixpoint() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 6));
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("shared relation root");
    let candidates = vec![vec![[0, 0], [1, 1]], vec![[0, 0]], vec![[1, 1]]];
    let budget = WorkBudget::new(1_000);

    let assignment = quotient
        .close_coordinate_roots(&ctx, 2, &candidates, Some(&budget))
        .expect("close the coordinate roots")
        .expect("arc-consistent coordinate closure");

    assert_eq!(quotient.root_count().expect("count the component roots"), 2);
    assert_eq!(
        assignment[&quotient.find(0).expect("find the component root")],
        0
    );
    assert_eq!(
        assignment[&quotient.find(4).expect("find the component root")],
        1
    );
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_forces_the_only_root_supporting_a_point() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(
        [vec![0, 1], vec![0], vec![0, 1, 2], vec![0]]
            .into_iter()
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .collect(),
    );
    let budget = WorkBudget::new(100);

    let assignment = quotient
        .close_coordinate_roots(&ctx, 3, &[Vec::new(), Vec::new()], Some(&budget))
        .expect("close the coordinate roots")
        .expect("point-support-forced coordinate closure");

    assert_eq!(quotient.root_count().expect("count the component roots"), 3);
    assert_eq!(
        assignment[&quotient.find(0).expect("find the component root")],
        1
    );
    assert_eq!(
        assignment[&quotient.find(1).expect("find the component root")],
        0
    );
    assert_eq!(
        assignment[&quotient.find(2).expect("find the component root")],
        2
    );
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_rejects_a_coordinate_support_hall_conflict() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(
        [vec![0, 1, 2, 3], vec![0, 1, 2, 3], vec![3], vec![3]]
            .into_iter()
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .collect(),
    );
    let budget = WorkBudget::new(1_000);

    assert!(quotient
        .close_coordinate_roots(&ctx, 4, &[Vec::new(), Vec::new()], Some(&budget))
        .expect("close the coordinate roots")
        .is_none());
    assert!(!budget.exhausted());
}

#[test]
fn coordinate_support_matching_exposes_essential_and_unsupported_hall_edges() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[0],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("build the decode context");
    let supports = [vec![0, 1], vec![0, 1], vec![0, 1, 2], vec![2, 3]];
    let matching = crate::solve::matching::distinct_domain_matching_with_budget(
        &ctx,
        supports.iter().map(Vec::as_slice),
        4,
        None,
        None,
    )
    .expect("find a distinct domain matching")
    .expect("coordinate support matching");

    assert_eq!(matching[2], 2);
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            &ctx,
            supports.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Exclude(
                2,
                matching[2]
            )),
        )
        .expect("find a distinct domain matching")
        .is_none()
    );

    let partitioned = [vec![0, 1, 2], vec![0, 1], vec![2, 3], vec![2, 3]];
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            &ctx,
            partitioned.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Require(
                0, 2
            )),
        )
        .expect("find a distinct domain matching")
        .is_none()
    );
    assert!(
        crate::solve::matching::distinct_domain_matching_with_budget(
            &ctx,
            partitioned.iter().map(Vec::as_slice),
            4,
            None,
            Some(crate::solve::matching::MatchingEdgeConstraint::Require(
                0, 0
            )),
        )
        .expect("find a distinct domain matching")
        .is_some()
    );
}

#[test]
fn quotient_coordinate_closure_enforces_complete_face_degrees() {
    catia_test_context!(ctx);
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut open =
        MeshQuotient::new([singleton(0), singleton(1), singleton(0), singleton(2)].into());
    let candidates = vec![Vec::new(); 2];
    let edge_faces = [[0, 0]; 2];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1])];
    let budget = WorkBudget::new(1_000);
    open.merge(0, 2)
        .expect("merge the components")
        .expect("shared endpoint");

    assert!(open
        .close_coordinate_roots_for_incidence_with_budget(
            &ctx,
            3,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: 1,
                domains: &domains,
            },
            Some(&budget),
        )
        .expect("close the coordinate roots for incidence")
        .is_none());

    let mut closed = MeshQuotient::new(
        [
            singleton(0),
            singleton(1),
            singleton(1),
            singleton(2),
            singleton(2),
            singleton(0),
        ]
        .into(),
    );
    let candidates = vec![Vec::new(); 3];
    let edge_faces = [[0, 0]; 3];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];

    assert!(closed
        .close_coordinate_roots_for_incidence_with_budget(
            &ctx,
            3,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: 1,
                domains: &domains,
            },
            Some(&WorkBudget::new(1_000)),
        )
        .expect("close the coordinate roots for incidence")
        .is_some());
}

#[test]
fn quotient_coordinate_closure_rejects_sealed_unordered_subcycles() {
    catia_test_context!(ctx);
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient::new(
        [
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
    );
    let candidates = vec![vec![[0, 1]], vec![[0, 1]], vec![[2, 3]], vec![[2, 3]]];
    let edge_faces = [[0, 0]; 4];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3])];
    let budget = WorkBudget::new(10_000);

    assert!(quotient
        .close_coordinate_roots_for_incidence_with_budget(
            &ctx,
            4,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: 1,
                domains: &domains,
            },
            Some(&budget),
        )
        .expect("close the coordinate roots for incidence")
        .is_none());
    assert!(!budget.exhausted());
}

#[test]
fn quotient_coordinate_closure_enforces_ordered_face_cycles() {
    catia_test_context!(ctx);
    let singleton = |point| Arc::new(HashSet::from([point]));
    let quotient = || {
        MeshQuotient::new(
            [
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
        )
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
            &ctx,
            4,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: 1,
                domains: &domain([0, 1, 2, 3]),
            },
            Some(&WorkBudget::new(10_000)),
        )
        .expect("close the coordinate roots for incidence")
        .is_none());
    assert!(quotient()
        .close_coordinate_roots_for_incidence_with_budget(
            &ctx,
            4,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: 1,
                domains: &domain([0, 2, 1, 3]),
            },
            Some(&WorkBudget::new(10_000)),
        )
        .expect("close the coordinate roots for incidence")
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
            &ctx,
            4,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: 1,
                domains: &fixed,
            },
            Some(&WorkBudget::new(10_000)),
        )
        .expect("close the coordinate roots for incidence")
        .is_none());
}

#[test]
fn quotient_closes_independent_coordinate_components_with_local_budgets() {
    const COMPONENT_COUNT: usize = 100;
    catia_test_context!(ctx);
    let point_count = COMPONENT_COUNT * 3;
    let mut quotient = MeshQuotient::new(
        (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
    );
    let mut candidates = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        quotient
            .merge(node + 1, node + 2)
            .expect("merge the components")
            .expect("shared first corner");
        quotient
            .merge(node + 3, node + 4)
            .expect("merge the components")
            .expect("shared second corner");
        candidates.extend([
            vec![[point, point + 1]],
            vec![[point + 1, point + 2]],
            vec![[point, point + 2]],
        ]);
    }

    let assignment = quotient
        .close_coordinate_roots(&ctx, point_count, &candidates, None)
        .expect("close the coordinate roots")
        .expect("independent coordinate closures");

    assert_eq!(
        quotient.root_count().expect("count the component roots"),
        point_count
    );
    assert_eq!(assignment.len(), point_count);
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        assert_eq!(
            quotient.find(node).expect("find the component root"),
            quotient.find(node + 5).expect("find the component root")
        );
    }
}

#[test]
fn quotient_counts_global_face_incidence_once_across_coordinate_components() {
    const COMPONENT_COUNT: usize = 40;
    catia_test_context!(ctx);
    let point_count = COMPONENT_COUNT * 3;
    let mut quotient = MeshQuotient::new(
        (0..COMPONENT_COUNT)
            .flat_map(|component| {
                let points = Arc::new((component * 3..component * 3 + 3).collect::<HashSet<_>>());
                std::iter::repeat_n(points, 6)
            })
            .collect(),
    );
    let mut candidates = Vec::new();
    let mut edge_faces = Vec::new();
    let mut domains = Vec::new();
    for component in 0..COMPONENT_COUNT {
        let node = component * 6;
        let point = component * 3;
        let first_edge = candidates.len();
        quotient
            .merge(node + 1, node + 2)
            .expect("merge the components")
            .expect("shared first corner");
        quotient
            .merge(node + 3, node + 4)
            .expect("merge the components")
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
            &ctx,
            point_count,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &edge_faces,
                face_count: COMPONENT_COUNT,
                domains: &domains,
            },
            Some(&budget),
        )
        .expect("close the coordinate roots for incidence")
        .is_some());
    assert!(!budget.exhausted());
}

#[test]
fn quotient_closure_does_not_budget_forced_component_depth() {
    const ROOT_COUNT: usize = 10_000;
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), ROOT_COUNT));
    let candidates = vec![vec![[0, 0]]; ROOT_COUNT / 2];

    let assignment = quotient
        .close_coordinate_roots(&ctx, 1, &candidates, None)
        .expect("close the coordinate roots")
        .expect("forced coordinate component");

    assert_eq!(quotient.root_count().expect("count the component roots"), 1);
    assert_eq!(assignment.values().copied().collect::<Vec<_>>(), [0]);
}

#[test]
fn quotient_does_not_guess_an_ambiguous_coordinate_closure() {
    catia_test_context!(ctx);
    let all = Arc::new(HashSet::from([0, 1]));
    let mut quotient = MeshQuotient::new(vec![all.clone(); 4]);
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("shared middle corner");

    assert!(quotient
        .close_coordinate_roots(&ctx, 2, &[vec![[0, 1]], vec![[0, 1]]], None)
        .expect("close the coordinate roots")
        .is_none());
    assert_eq!(quotient.root_count().expect("count the component roots"), 3);
}

#[test]
fn quotient_closure_requires_every_coordinate_row_in_a_domain() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 4));
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("shared endpoint");

    assert!(quotient
        .close_coordinate_roots(&ctx, 2, &[vec![[0, 0]], vec![[0, 0]]], None)
        .expect("close the coordinate roots")
        .is_none());
    assert_eq!(quotient.root_count().expect("count the component roots"), 3);
}

#[test]
fn quotient_accepts_diagonal_domain_for_closed_edge() {
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([2])),
        Arc::new(HashSet::from([2])),
    ]);
    quotient
        .merge(0, 1)
        .expect("merge the components")
        .expect("closed endpoint merge");
    assert!(quotient
        .edge_domains_viable(&[vec![[2, 2]]])
        .expect("check that the edge domains are viable"));
    assert!(!quotient
        .edge_domains_viable(&[vec![[1, 2]]])
        .expect("check that the edge domains are viable"));
}

#[test]
fn quotient_point_assignment_accepts_a_closed_diagonal_edge() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 2));
    let root = quotient
        .merge(0, 1)
        .expect("merge the components")
        .expect("closed endpoint merge");

    assert_eq!(
        quotient
            .point_assignment(&ctx, 1, &[vec![[0, 0]]], None)
            .expect("compute the point assignment"),
        Some(HashMap::from([(root, 0)]))
    );
}

#[test]
fn quotient_retains_diagonal_pairs_until_ports_are_merged() {
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([1, 2])),
        Arc::new(HashSet::from([1, 2])),
    ]);

    assert!(quotient
        .edge_domains_viable(&[vec![[2, 2]]])
        .expect("check that the edge domains are viable"));
    assert_eq!(
        quotient.domains(),
        vec![Arc::new(HashSet::from([2])), Arc::new(HashSet::from([2]))]
    );
    quotient
        .merge(0, 1)
        .expect("merge the components")
        .expect("closed endpoint merge");
    assert!(quotient
        .edge_domains_viable(&[vec![[2, 2]]])
        .expect("check that the edge domains are viable"));
}

#[test]
fn closed_edge_is_a_single_coedge_boundary_on_each_incident_face() {
    catia_test_context!(ctx);
    let topology = reconstruct_incidence(
        &ctx,
        &crate::test_support::topology_budget(),
        vec![{
            assert!(EdgeRow::new(0, vec![7, 7], EdgeBoundaryLayout::CompleteBoundaryRun).is_none());
            EdgeRow::new(1, vec![7, 7], EdgeBoundaryLayout::CompleteBoundaryRun)
                .expect("admitted edge row")
        }],
        vec![[1.0, 0.0, 0.0]],
        &[[0, 1]],
        &[[0, 0]],
        2,
    )
    .expect("reconstruct incidence")
    .expect("closed radial edge");
    assert!(topology
        .faces
        .iter()
        .all(|face| face.boundaries.len() == 1 && face.boundaries[0].coedges.len() == 1));
    assert_ne!(
        topology.faces[0].boundaries[0].coedges[0].reversed,
        topology.faces[1].boundaries[0].coedges[0].reversed
    );
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
    catia_test_context!(ctx);
    let singleton = |point| Arc::new(HashSet::from([point]));
    let mut quotient = MeshQuotient::new(
        [
            singleton(1),
            singleton(2),
            singleton(1),
            singleton(0),
            singleton(0),
            singleton(2),
        ]
        .into(),
    );
    quotient
        .merge(4, 3)
        .expect("merge the components")
        .expect("first fixed-direction corner");
    quotient
        .merge(2, 0)
        .expect("merge the components")
        .expect("second fixed-direction corner");
    quotient
        .merge(1, 5)
        .expect("merge the components")
        .expect("third boundary corner");
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
            &ctx,
            3,
            &candidates,
            MeshIncidenceBoundary {
                edge_faces: &[[0, 0]; 3],
                face_count: 1,
                domains: &domain,
            },
            Some(&WorkBudget::new(10_000)),
        )
        .expect("close the coordinate roots for incidence")
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
            |_, _| true
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

    assert_eq!(support.by_edge[&0], HashSet::from([[0, 1]]));
    assert_eq!(support.by_edge[&1], HashSet::from([[1, 3]]));
    assert_eq!(support.by_edge[&2], HashSet::from([[0, 3]]));
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

    assert!(support.by_edge.is_empty());
}

#[test]
fn ordered_face_cycle_support_materializes_only_supported_implicit_pairs() {
    catia_test_context!(ctx);
    let mut choices = vec![vec![[0, 1], [0, 2]], Vec::new(), vec![[0, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &choices,
        4,
        &[[10, 11], [12, 13], [14, 15]],
    )
    .expect("build the initial mesh quotient")
    .expect("initial quotient");
    let coordinate_domains = quotient
        .prepare_coordinate_root_domains(&ctx, 4, &choices, None)
        .expect("prepare the coordinate root domains")
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
        .expect("prune implicit ordered face endpoint support")
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
        &mut candidates
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
fn duplicate_face_slot_requires_one_joint_carrier_and_mesh_assignment() {
    let serialized = [[0, 0], [0, 1], [1, 1]];
    let allowed = [vec![1, 2], Vec::new(), vec![0, 2]];
    let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 3, |faces| {
        Ok(faces == [[0, 2], [0, 1], [1, 0]])
    })
    .expect("find the unique duplicate face assignment")
    .expect("one complete assignment");
    assert_eq!(resolved, [[0, 2], [0, 1], [1, 0]]);

    assert!(
        unique_duplicate_face_assignment(&serialized, &allowed, 3, |_| Ok(true))
            .expect("find the unique duplicate face assignment")
            .is_none()
    );
    assert!(unique_duplicate_face_assignment(
        &serialized,
        &[vec![3], Vec::new(), vec![0]],
        3,
        |_| Ok(true),
    )
    .expect("find the unique duplicate face assignment")
    .is_none());
}

#[test]
fn duplicate_face_slot_without_admitted_alternate_remains_unresolved() {
    let serialized = [[0, 0], [0, 0]];
    let allowed = [Vec::new(), vec![1]];

    let resolved = unique_duplicate_face_assignment(&serialized, &allowed, 2, |faces| {
        Ok(faces == [[0, 0], [0, 1]])
    })
    .expect("find the unique duplicate face assignment")
    .expect("the unresolved same-face slot remains in the joint assignment");

    assert_eq!(resolved, [[0, 0], [0, 1]]);
}

#[test]
fn duplicate_face_assignment_visitor_keeps_alternates_correlated() {
    catia_test_context!(ctx);
    let serialized = [[0, 0], [1, 1], [0, 2]];
    let allowed = [vec![2, 1, 0], Vec::new(), Vec::new()];
    let mut assignments = Vec::new();

    let outcome =
        visit_duplicate_face_assignments(&ctx, &serialized, &allowed, 3, 4, |assignment| {
            assignments.push(assignment.to_vec());
            Ok(true)
        })
        .expect("visit duplicate face assignments");

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

mod endpoint_ports;

#[test]
fn endpoint_cycle_support_keeps_only_the_closing_pair() {
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
    let run = || {
        crate::solve::mesh_quotient::mesh_assignment_endpoint_cycle_support_by(
            &assignment,
            None,
            |edge| {
                candidates.get(edge).map(|values| {
                    crate::solve::mesh_quotient::MeshEndpointCandidates::Explicit(values)
                })
            },
            |_, _| true,
        )
    };
    let support = run().expect("bounded layered support");
    assert_eq!(support.by_edge[&0], HashSet::from([[0, 1]]));
}

#[test]
fn endpoint_cycle_support_is_empty_for_one_open_edge() {
    let candidates = [vec![[0, 1]]];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let run = || {
        crate::solve::mesh_quotient::mesh_assignment_endpoint_cycle_support_by(
            &assignment,
            None,
            |edge| {
                candidates.get(edge).map(|values| {
                    crate::solve::mesh_quotient::MeshEndpointCandidates::Explicit(values)
                })
            },
            |_, _| true,
        )
    };
    assert!(run().expect("one-layer support").by_edge.is_empty());
}

#[test]
fn ordered_face_pruning_uses_implicit_coordinate_candidates() {
    catia_test_context!(service_ctx);
    let choices = vec![vec![[0, 1], [0, 2]], Vec::new(), vec![[0, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &choices,
        4,
        &[[10, 11], [12, 13], [14, 15]],
    )
    .expect("build the initial mesh quotient")
    .expect("initial quotient");
    let coordinate_domains = quotient
        .prepare_coordinate_root_domains(&service_ctx, 4, &choices, None)
        .expect("prepare the coordinate root domains")
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
    let mut service_choices = choices.clone();
    let budget = WorkBudget::new(10_000);
    assert!(
        crate::solve::incidence::prune_implicit_ordered_face_endpoint_support(
            &domains,
            &mut service_choices,
            &coordinate_domains,
            &budget,
        )
        .expect("prune implicit ordered face endpoint support")
    );
}

#[test]
fn mesh_endpoint_pair_support_accepts_consistent_alternate_faces() {
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
        &mut candidates
    ));
}
