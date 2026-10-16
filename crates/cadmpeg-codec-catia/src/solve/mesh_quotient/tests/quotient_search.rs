use crate::families::standard::topology::{
    solve_boundary_orientation_constraints, EdgeBoundaryLayout, EdgeRow, StandardTopologyDraft,
};
use crate::solve::mesh_quotient::selection_search::mesh_assignment_can_merge;
use crate::solve::mesh_quotient::{
    admit_orientation_option, deduplicate_mesh_quotient_assignments, initial_mesh_quotient,
    orientation_fingerprint, orientation_options_equivalent, possible_face_choices,
    possible_face_choices_with_limit, possible_face_equations, MeshQuotient, MeshSelectionSearch,
    SearchOutcome, MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::missing_edge::{
    MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
};
use crate::solve::tests::repeated_domain;
use cadmpeg_core::decode::WorkBudget;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[test]
fn orientation_fingerprint_preserves_exact_quotient_and_direction_equality() {
    let domains = vec![
        Arc::new(HashSet::from([0, 1])),
        Arc::new(HashSet::from([1, 2])),
    ];
    let mut left = MeshQuotient::new(domains.clone());
    let mut right = MeshQuotient::new(domains);
    assert!(left.merge(0, 1).expect("merge the components").is_some());
    assert!(right.merge(1, 0).expect("merge the components").is_some());
    let first = vec![vec![false, true]];
    let complement = vec![vec![true, false]];
    assert_eq!(
        orientation_fingerprint(&left, &first).expect("build the orientation fingerprint"),
        orientation_fingerprint(&right, &complement).expect("build the orientation fingerprint")
    );
    assert!(
        orientation_options_equivalent(&left, &first, &right, &complement)
            .expect("check that the orientation options are equivalent")
    );
    assert!(
        !orientation_options_equivalent(&left, &first, &right, &[vec![false, false]])
            .expect("check that the orientation options are equivalent")
    );

    let mut seen = HashMap::new();
    let mut output = Vec::new();
    assert!(admit_orientation_option(&mut seen, &output, &first, &left)
        .expect("admit the orientation option"));
    output.push((first.clone(), left.clone()));
    assert!(
        !admit_orientation_option(&mut seen, &output, &complement, &right)
            .expect("admit the orientation option")
    );
}

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
    catia_test_context!(ctx);
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
    let quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), EDGE_COUNT * 2));
    let candidates = vec![vec![[0, 0]]; EDGE_COUNT];
    let budget = WorkBudget::new(30);

    let options = quotient
        .assignment_options_limited(
            &ctx,
            &assignment,
            &candidates,
            &HashSet::new(),
            2,
            Some(&budget),
        )
        .expect("find the assignment orientation options");

    assert_eq!(options.len(), 1);
    assert_eq!(options[0].0, vec![vec![false; EDGE_COUNT]]);
    assert!(!budget.exhausted());
}

#[test]
fn mesh_option_enumeration_preserves_asymmetric_endpoint_directions() {
    catia_test_context!(ctx);
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
    let quotient = MeshQuotient::new(
        [vec![0], vec![1], vec![0], vec![1]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );

    let options = quotient
        .assignment_options_limited(
            &ctx,
            &assignment,
            &[vec![[0, 1]], vec![[0, 1]]],
            &HashSet::new(),
            4,
            None,
        )
        .expect("find the assignment orientation options");

    assert!(options
        .iter()
        .any(|(directions, _)| directions == &[vec![false, true]]));
}

#[test]
fn quotient_merge_preserves_physical_edge_pair_correlation() {
    let mut quotient = MeshQuotient::new(
        [vec![0], vec![0, 1], vec![0], vec![2]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("nonempty port intersection");
    assert!(!quotient
        .edge_domains_viable(&[vec![[0, 1]], vec![[0, 2]]])
        .expect("check that the edge domains are viable"));
}

#[test]
fn quotient_clones_share_unconstrained_point_domains() {
    let all = Arc::new((0..1_000).collect::<HashSet<_>>());
    let quotient = MeshQuotient::new(vec![all.clone(), all.clone(), all.clone(), all.clone()]);

    let clone = quotient.clone();
    assert!(Arc::ptr_eq(&quotient.domains()[0], &clone.domains()[0]));
    assert!(Arc::ptr_eq(&quotient.domains()[0], &quotient.domains()[3]));
}

#[test]
fn port_quotient_completes_only_supported_unknown_edge_pairs() {
    let candidates = [vec![[0, 1]], Vec::new(), vec![[2, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        5,
        &[[10, 11], [10, 12], [12, 13]],
    )
    .expect("build the initial mesh quotient")
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
    catia_test_context!(ctx);
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
    .expect("build the initial mesh quotient")
    .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(&ctx, 3, &candidates, None)
        .expect("prepare the coordinate root domains")
        .expect("coordinate root domains");

    assert_eq!(
        domains.edge_candidates(),
        &[vec![[0, 1]], vec![[0, 1]], vec![[0, 1]], vec![[2, 2]]]
    );
}

#[test]
fn coordinate_root_preparation_skips_on_an_exhausted_local_slice() {
    let candidates = [
        vec![[0, 1], [0, 2]],
        vec![[0, 1]],
        vec![[0, 1]],
        vec![[2, 2]],
    ];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        3,
        &[[10, 11], [10, 12], [13, 11], [14, 14]],
    )
    .expect("build the initial mesh quotient")
    .expect("initial quotient");
    // Every local slice too small for the preparation ends it without
    // domains; none of them refuses the session.
    let mut skipped = 0;
    let mut prepared = false;
    for units in 0..4096 {
        let (result, refusal) = crate::test_support::with_service_context(|ctx| {
            let budget = WorkBudget::new(units);
            let result = quotient.clone().prepare_coordinate_root_domains(
                ctx,
                3,
                &candidates,
                Some(&budget),
            );
            (
                result.map(|domains| domains.is_some()),
                ctx.resource_refusal(),
            )
        });
        assert_eq!(refusal, None, "{units}");
        if result.expect("local slice exhaustion is not a session refusal") {
            prepared = true;
            break;
        }
        skipped += 1;
    }
    assert!(skipped > 0);
    assert!(prepared, "a sufficient slice prepares the domains");
}

#[test]
fn selected_edge_pair_propagates_through_shared_coordinate_roots() {
    catia_test_context!(ctx);
    let candidates = [vec![[0, 1], [0, 2]], vec![[1, 3], [2, 3]], vec![[2, 3]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        4,
        &[[10, 11], [11, 12], [13, 14]],
    )
    .expect("build the initial mesh quotient")
    .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(&ctx, 4, &candidates, None)
        .expect("prepare the coordinate root domains")
        .expect("coordinate root domains")
        .refine_edge_candidate_arc(&ctx, 0, [0, 1], None)
        .expect("refine edge candidate arc")
        .expect("selected edge pair");

    assert!(domains.supports_edge_candidate(1, [1, 3]));
    assert!(!domains.supports_edge_candidate(1, [2, 3]));
}

#[test]
fn port_quotient_declines_unbounded_unknown_edge_pairs() {
    let candidates = [Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 100, &[[10, 11]])
            .expect("build the initial mesh quotient")
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
    catia_test_context!(ctx);
    let candidates = [Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 2, &[[10, 11]])
            .expect("build the initial mesh quotient")
            .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(&ctx, 2, &candidates, None)
        .expect("prepare the coordinate root domains")
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
    assert!(domains
        .refine_edge_candidate_arc(&ctx, 0, [0, 1], None)
        .expect("refine edge candidate arc")
        .is_some());
    assert!(domains
        .refine_edge_candidate_arc(&ctx, 0, [0, 0], None)
        .expect("refine edge candidate arc")
        .is_none());
}

#[test]
fn required_implicit_coordinate_pairs_scale_with_root_domains_not_their_product() {
    catia_test_context!(ctx);
    let candidates = [Vec::new(), Vec::new()];
    let mut quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 4, &[[10, 11], [12, 13]])
            .expect("build the initial mesh quotient")
            .expect("initial quotient");
    let domains = quotient
        .prepare_coordinate_root_domains(&ctx, 4, &candidates, None)
        .expect("prepare the coordinate root domains")
        .expect("implicit coordinate domains");
    let implicit = domains
        .implicit_edge_candidates(0, Some(1))
        .expect("required implicit candidates");

    assert_eq!(implicit.width_upper_bound(), 3);
    assert_eq!(implicit.collect::<Vec<_>>(), vec![[0, 1], [1, 2], [1, 3]]);

    let mut visited = Vec::new();
    assert!(domains
        .implicit_edge_candidate_with_point(0, 1, None, |pair| {
            visited.push(pair);
            pair == [1, 3]
        })
        .is_some());
    assert_eq!(visited, vec![[0, 1], [1, 2], [1, 3]]);

    let budget = WorkBudget::new(2);
    assert!(domains
        .implicit_edge_candidate_with_point(0, 1, Some(&budget), |_| false)
        .is_none());
    assert!(budget.exhausted());
}

#[test]
fn coordinate_domain_preparation_scales_with_constraint_graph_work() {
    let candidates = vec![Vec::new(); 100];
    let ports = (0..100)
        .map(|edge| {
            [
                u32::try_from(edge * 2).expect("fixture value fits u32"),
                u32::try_from(edge * 2 + 1).expect("fixture value fits u32"),
            ]
        })
        .collect::<Vec<_>>();
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(&candidates, 200, &ports)
        .expect("build the initial mesh quotient")
        .expect("initial quotient");

    assert!(
        quotient
            .coordinate_domain_preparation_limit(200, &candidates)
            .expect("determine the coordinate domain preparation limit")
            .expect("preparation limit")
            > crate::solve::mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS
    );
}

#[test]
fn incidence_search_consumes_implicit_coordinate_root_pairs() {
    use crate::solve::incidence::{component_incidence_pair_solution_outcome, IncidenceSolve};

    catia_test_context!(ctx);
    let candidates = vec![Vec::new(); 3];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        3,
        &[[10, 11], [11, 12], [12, 10]],
    )
    .expect("build the initial mesh quotient")
    .expect("cycle quotient");
    let outcome = component_incidence_pair_solution_outcome(
        &ctx,
        &candidates,
        &[[0, 0]; 3],
        1,
        3,
        None,
        Some(&quotient),
        None,
        &|_| Ok(true),
    )
    .expect("compute the component incidence pair solution outcome");

    assert!(matches!(
        outcome,
        IncidenceSolve::Solved(_) | IncidenceSolve::Ambiguous
    ));
}

#[test]
fn ordered_face_equations_narrow_unknown_edge_roots_before_pair_completion() {
    catia_test_context!(ctx);
    let edge_candidates = vec![vec![[0, 1]], Vec::new(), vec![[0, 2]]];
    let mut quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &edge_candidates,
        3,
        &[[10, 11], [12, 13], [14, 15]],
    )
    .expect("build the initial mesh quotient")
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
        &ctx,
        &domains,
        &edge_candidates,
        &mut quotient,
        &budget,
    )
    .expect("propagate common ordered face quotients")
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
    let mut quotient = MeshQuotient::new(
        [vec![0, 1], vec![2], vec![0, 1], vec![3, 4]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    let root = quotient
        .merge(0, 2)
        .expect("merge the components")
        .expect("shared endpoint component");

    assert!(quotient
        .edge_domains_viable(&[vec![[0, 2]], vec![[0, 3], [1, 4]],])
        .expect("check that the edge domains are viable"));
    assert_eq!(*quotient.domains()[root], HashSet::from([0]));
    let third_root = quotient.find(3).expect("find the component root");
    assert_eq!(*quotient.domains()[third_root], HashSet::from([3]));
}

#[test]
fn quotient_assignment_requires_one_consistent_closed_orientation() {
    let mut quotient = MeshQuotient::new(
        [vec![0], vec![1], vec![2], vec![3]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
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
    assert!(!quotient
        .assignment_has_option(&assignment, &[vec![], vec![]], None)
        .expect("check assignment has option"));
    quotient = MeshQuotient::new(
        [vec![0], vec![1], vec![1, 2], vec![3]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    assert!(!quotient
        .assignment_has_option(&assignment, &[vec![], vec![]], None)
        .expect("check assignment has option"));
    quotient = MeshQuotient::new(
        [vec![0], vec![1], vec![1, 2], vec![0, 3]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
    assert!(quotient
        .assignment_has_option(&assignment, &[vec![], vec![]], None)
        .expect("check assignment has option"));
}

#[test]
fn quotient_assignment_declines_when_its_work_budget_is_exhausted() {
    let quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 2));
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let budget = WorkBudget::new(0);

    assert!(!quotient
        .assignment_has_option(&assignment, &[vec![[0, 0]]], Some(&budget))
        .expect("check assignment has option"));
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

    let mut choices = Vec::new();
    assert!(!possible_face_choices_with_limit(
        &assignments,
        &equations,
        0,
        &mut choices
    ));
}

#[test]
fn fixed_boundary_option_has_no_recursive_depth_limit() {
    const EDGE_COUNT: usize = 10_000;
    let quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), EDGE_COUNT * 2));
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

    assert!(quotient
        .assignment_has_option(&assignment, &candidates, None)
        .expect("check assignment has option"));
}

#[test]
fn quotient_options_reject_an_interior_pair_contradiction() {
    catia_test_context!(ctx);
    let quotient = MeshQuotient::new(
        [vec![0], vec![1, 2], vec![2], vec![3], vec![0, 3], vec![0]]
            .map(|domain| Arc::new(domain.into_iter().collect()))
            .into(),
    );
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

    let options = quotient.assignment_options(&ctx, &assignment, &candidates);

    assert!(!options
        .iter()
        .any(|(directions, _)| directions == &[vec![false, false, false]]));
    let unrestricted = [Vec::new(), Vec::new(), Vec::new()];
    let options = quotient.assignment_options(&ctx, &assignment, &unrestricted);
    let limited = quotient
        .assignment_options_limited(&ctx, &assignment, &unrestricted, &HashSet::new(), 1, None)
        .expect("find the assignment orientation options");
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].0, options[0].0);
    let unique = quotient
        .assignment_options_limited(
            &ctx,
            &assignment,
            &unrestricted,
            &HashSet::new(),
            4_096,
            None,
        )
        .expect("find the assignment orientation options");
    assert!(unique
        .iter()
        .all(|option| options.iter().any(|candidate| candidate.0 == option.0)));
}

#[test]
fn quotient_options_decline_when_their_work_budget_is_exhausted() {
    catia_test_context!(ctx);
    let quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 2));
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    };
    let budget = WorkBudget::new(0);

    let options = quotient
        .assignment_options_limited(
            &ctx,
            &assignment,
            &[vec![[0, 0]]],
            &HashSet::new(),
            1,
            Some(&budget),
        )
        .expect("find the assignment orientation options");

    assert!(options.is_empty());
    assert!(budget.exhausted());
}

#[test]
fn quotient_point_assignment_preserves_endpoint_pair_relations() {
    catia_test_context!(ctx);
    let quotient = || {
        MeshQuotient::new(
            [vec![0, 1], vec![2], vec![0, 1], vec![3]]
                .map(|domain| Arc::new(domain.into_iter().collect()))
                .into(),
        )
    };
    assert!(quotient()
        .point_assignment(&ctx, 4, &[vec![], vec![]], None)
        .expect("compute the point assignment")
        .is_none());
    assert!(quotient()
        .point_assignment_exists(&ctx, 4, &[vec![], vec![]], None)
        .expect("check point assignment exists"));

    let assignment = quotient()
        .point_assignment(&ctx, 4, &[vec![[0, 2]], vec![[1, 3]]], None)
        .expect("compute the point assignment")
        .expect("edge-pair relations determine the coordinate bijection");
    assert_eq!(assignment[&0], 0);
    assert_eq!(assignment[&1], 2);
    assert_eq!(assignment[&2], 1);
    assert_eq!(assignment[&3], 3);
}

#[test]
fn quotient_point_existence_declines_when_its_work_budget_is_exhausted() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 2));
    let budget = WorkBudget::new(0);

    assert!(!quotient
        .point_assignment_exists(&ctx, 2, &[vec![]], Some(&budget))
        .expect("check point assignment exists"));
    assert!(budget.exhausted());
}

#[test]
fn point_assignment_handles_deep_augmenting_paths_iteratively() {
    const ROOT_COUNT: usize = 10_000;
    catia_test_context!(ctx);
    let mut domains = (0..ROOT_COUNT - 1)
        .map(|root| Arc::new(HashSet::from([root, root + 1])))
        .collect::<Vec<_>>();
    domains.push(Arc::new(HashSet::from([0])));
    let mut quotient = MeshQuotient::new(domains);

    let assignment = quotient
        .point_assignment(&ctx, ROOT_COUNT, &[], None)
        .expect("compute the point assignment")
        .expect("forced coordinate bijection");

    assert_eq!(assignment.len(), ROOT_COUNT);
    assert_eq!(assignment[&(ROOT_COUNT - 1)], 0);
    assert!((0..ROOT_COUNT - 1).all(|root| assignment[&root] == root + 1));
}

#[test]
fn quotient_point_existence_rejects_an_all_different_conflict() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([0])),
    ]);

    assert!(!quotient
        .point_assignment_exists(&ctx, 2, &[vec![]], None)
        .expect("check point assignment exists"));
}

#[test]
fn quotient_point_existence_can_become_viable_after_a_root_merge() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([1])),
        Arc::new(HashSet::from([2])),
    ]);

    assert!(!quotient
        .point_assignment_exists(&ctx, 3, &[vec![], vec![]], None)
        .expect("check point assignment exists"));
    quotient
        .merge(0, 1)
        .expect("merge the components")
        .expect("compatible roots merge");
    assert!(quotient
        .point_assignment_exists(&ctx, 3, &[vec![], vec![]], None)
        .expect("check point assignment exists"));
}

#[test]
fn mesh_selection_rejects_an_odd_boundary_orientation_cycle() {
    catia_test_context!(ctx);
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
        ctx: &ctx,
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
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.selected_orientable());
    search.selected[2] = Some((0, vec![vec![false, true]]));
    assert!(search.selected_orientable());
}

#[test]
fn partial_boundary_orientation_constraints_reject_an_odd_parity_cycle() {
    catia_test_context!(ctx);
    let mut edge_uses = HashMap::from([
        (0, vec![(0, false), (1, false)]),
        (1, vec![(1, false), (2, false)]),
        (2, vec![(2, false), (0, false)]),
        (3, vec![(3, false)]),
    ]);

    assert!(
        solve_boundary_orientation_constraints(&ctx, 4, &edge_uses, false)
            .expect("solve boundary orientation constraints")
            .is_none()
    );
    edge_uses.get_mut(&2).expect("third paired edge")[1].1 = true;
    assert!(
        solve_boundary_orientation_constraints(&ctx, 4, &edge_uses, false)
            .expect("solve boundary orientation constraints")
            .is_some()
    );
    assert!(
        solve_boundary_orientation_constraints(&ctx, 4, &edge_uses, true)
            .expect("solve boundary orientation constraints")
            .is_none()
    );
}

#[test]
fn partial_face_orientability_rejects_an_odd_open_path_cycle() {
    catia_test_context!(ctx);
    let edge_faces = [[0, 1], [1, 2], [2, 0]];
    let face_edges = vec![vec![0, 2], vec![0, 1], vec![1, 2]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let complete = [Some([0, 1]), Some([1, 2]), Some([0, 2])];

    assert!(!crate::solve::incidence::partial_face_orientability_viable(
        &ctx,
        &complete,
        &edge_faces,
        &face_edges,
        &budget,
    )
    .expect("check partial face orientability viable"));
    let partial = [Some([0, 1]), Some([1, 2]), None];
    assert!(crate::solve::incidence::partial_face_orientability_viable(
        &ctx,
        &partial,
        &edge_faces,
        &face_edges,
        &budget,
    )
    .expect("check partial face orientability viable"));
    assert!(!budget.exhausted());
}

#[test]
fn mesh_selection_rejects_a_branch_with_no_orientable_remaining_face() {
    catia_test_context!(ctx);
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
        ctx: &ctx,
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
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    assert!(!search.fixed_remaining_faces_are_orientable());
    search.selected[1] = Some((0, vec![vec![false, true]]));
    assert!(search.fixed_remaining_faces_are_orientable());
}

#[test]
fn mesh_selection_checks_all_fixed_remaining_faces_together() {
    catia_test_context!(ctx);
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
        ctx: &ctx,
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
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };

    assert!(!search.fixed_remaining_faces_are_orientable());
}

#[test]
fn partial_mesh_selection_survives_optional_deduction_exhaustion() {
    catia_test_context!(ctx);
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
        {
            assert!(EdgeRow::new(
                1,
                vec![0, 1],
                EdgeBoundaryLayout::InteriorWithFlankingCorners
            )
            .is_none());
            EdgeRow::new(
                1,
                vec![0, 0, 1],
                EdgeBoundaryLayout::InteriorWithFlankingCorners,
            )
            .expect("admitted edge row")
        };
        2
    ];
    let vertex_points = vec![[0.0; 3], [1.0, 0.0, 0.0]];
    let search = MeshSelectionSearch {
        ctx: &ctx,
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
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = initial_mesh_quotient(&edge_candidates, 2, &[[0, 1], [2, 3]])
        .expect("build the initial mesh quotient")
        .expect("initial quotient");
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("selected face corner");
    let propagation_budget = WorkBudget::new(0);
    let changed_edges = HashSet::from([0]);

    let mut prepared = search
        .prepare_selected_branch(&quotient, &changed_edges, &propagation_budget)
        .expect("prepare selected branch")
        .expect("partial quotient remains viable");

    assert_eq!(prepared.root_count().expect("count the component roots"), 3);
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
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 4));

    assert!(mesh_assignment_can_merge(&assignment, &mut quotient)
        .expect("check that the mesh assignment can merge"));
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("first boundary corner");
    quotient
        .merge(3, 0)
        .expect("merge the components")
        .expect("second boundary corner");
    assert!(!mesh_assignment_can_merge(&assignment, &mut quotient)
        .expect("check that the mesh assignment can merge"));
}

#[test]
fn remaining_merge_capacity_counts_distinct_quotient_equations() {
    catia_test_context!(ctx);
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
        ctx: &ctx,
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
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 4));

    assert_eq!(
        search
            .remaining_equation_merge_capacity(&mut quotient)
            .expect("count the possible remaining equation merges"),
        Some(2)
    );
    quotient
        .merge(1, 2)
        .expect("merge the components")
        .expect("first repeated equation");
    assert_eq!(
        search
            .remaining_equation_merge_capacity(&mut quotient)
            .expect("count the possible remaining equation merges"),
        Some(1)
    );
}

mod selection_search;

#[test]
fn quotient_assignment_deduplication_keeps_one_identical_assignment() {
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
    let run = || {
        let mut faces = vec![vec![assignment.clone(), assignment.clone()]];
        deduplicate_mesh_quotient_assignments(&mut faces);
        Ok::<_, cadmpeg_core::CodecError>(faces[0].len())
    };
    assert_eq!(run().expect("deduplicate mesh quotient assignments"), 1);
}

#[test]
fn possible_face_equations_list_corners_of_a_two_edge_boundary() {
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
    let run = || possible_face_equations(&assignments);
    let service = run();
    assert_eq!(service.len(), 1);
    assert!(!service[0].is_empty());
}

#[test]
fn selection_state_signature_copies_the_selected_choices() {
    let assignments = [Vec::new()];
    let quotient = MeshQuotient::new(vec![Arc::new(HashSet::from([0]))]);
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let search = MeshSelectionSearch {
            ctx,
            assignments: &assignments,
            possible_face_equations: Vec::new(),
            possible_face_choices: Vec::new(),
            face_work: vec![Some(1)],
            edge_candidates: &[],
            edge_rows: &[],
            vertex_points: &[],
            candidate_gauge: None,
            port_identities: None,
            fixed_face_directions: Vec::new(),
            fixed_edge_orientations: vec![Some(true)],
            edge_has_fixed_direction: Vec::new(),
            selected: vec![Some((0, vec![vec![true]]))],
            visited_states: HashSet::new(),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        search.selection_state_signature(&quotient, false)
    };
    let signature = crate::test_support::with_service_context(run)
        .expect("compute the selection state signature");
    assert_eq!(signature.1, vec![Some((0, vec![vec![true]]))]);
}

#[test]
fn coordinate_root_domains_drop_an_unsupported_candidate_pair() {
    let candidates = [
        vec![[0, 1], [0, 2]],
        vec![[0, 1]],
        vec![[0, 1]],
        vec![[2, 2]],
    ];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &candidates,
        3,
        &[[10, 11], [10, 12], [13, 11], [14, 14]],
    )
    .expect("build the initial mesh quotient")
    .expect("initial quotient");
    let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        quotient
            .clone()
            .prepare_coordinate_root_domains(ctx, 3, &candidates, None)
    };
    let selected = crate::test_support::with_service_context(run)
        .expect("prepare the coordinate root domains")
        .expect("coordinate root domains");
    assert_eq!(selected.edge_candidates()[0], [[0, 1]]);
}

#[test]
fn ordered_triangle_face_supports_three_corner_equations() {
    let edge_candidates = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]];
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
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![assignment.clone()])];
    catia_test_context!(service_ctx);
    let initial_quotient =
        initial_mesh_quotient(&edge_candidates, 3, &[[10, 11], [12, 13], [14, 15]])
            .expect("build the initial mesh quotient")
            .expect("initial quotient");
    let mut quotient = initial_quotient.clone();
    let budget = WorkBudget::new(1_000);
    assert!(
        crate::solve::mesh_quotient::common_supported_corner_equations(
            &mut quotient,
            std::slice::from_ref(&assignment),
            &budget,
        )
        .expect("compute the common supported corner equations")
        .is_some()
    );
    let fixed_assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..3)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 0,
                reversed: Some(false),
            })
            .collect()],
    };
    let mut quotient = initial_quotient.clone();
    let budget = WorkBudget::new(1_000);
    assert_eq!(
        crate::solve::mesh_quotient::common_supported_corner_equations(
            &mut quotient,
            std::slice::from_ref(&fixed_assignment),
            &budget,
        )
        .expect("compute the common supported corner equations")
        .expect("fixed cycle has supported corners")
        .len(),
        3
    );
    let mut quotient = initial_quotient.clone();
    let budget = WorkBudget::new(1_000);
    assert_eq!(
        crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
            &service_ctx,
            &domains,
            &edge_candidates,
            &mut quotient,
            &budget,
        )
        .expect("propagate common ordered face quotients"),
        Some(())
    );
}

#[test]
fn single_full_quotient_alternative_merges_into_the_common_quotient() {
    use cadmpeg_core::CodecError;
    let domains = repeated_domain(HashSet::from([0, 1]), 4);
    let base = MeshQuotient::new(domains.clone());
    let mut alternative = MeshQuotient::new(domains);
    alternative
        .merge(0, 2)
        .expect("merge the components")
        .expect("shared domain");
    alternative
        .merge(1, 3)
        .expect("merge the components")
        .expect("shared domain");
    let run = || {
        let mut quotient = base.clone();
        let result = crate::solve::mesh_quotient::propagate_common_full_quotients(
            vec![alternative.clone()],
            &[Vec::new(), Vec::new()],
            &mut quotient,
        )?;
        Ok::<_, CodecError>((
            result,
            [
                quotient.find(0).expect("find the component root"),
                quotient.find(1).expect("find the component root"),
                quotient.find(2).expect("find the component root"),
                quotient.find(3).expect("find the component root"),
            ],
        ))
    };
    let (result, roots) = run().expect("propagate common full quotients");
    assert_eq!(result, Some(()));
    assert_eq!(roots[0], roots[2]);
    assert_eq!(roots[1], roots[3]);
}

#[test]
fn face_choice_materialization_declines_when_budget_ends_between_masks() {
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        }]],
    }]];
    let fallback = vec![vec![[0, 1]]];
    let run = |work_limit| {
        let mut choices = Vec::new();
        let complete =
            possible_face_choices_with_limit(&assignments, &fallback, work_limit, &mut choices);
        complete.then_some(choices)
    };
    assert!(run(2).is_none());
}

#[test]
fn face_choices_use_the_fallback_equations_for_13_unknown_edges() {
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..13)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: edge,
                end: edge + 1,
                reversed: None,
            })
            .collect()],
    };
    let faces = [vec![assignment]];
    let fallback = [vec![[0, 1]]];
    let mut choices = Vec::new();
    let complete = possible_face_choices_with_limit(&faces, &fallback, 2, &mut choices);
    assert_eq!(complete.then_some(choices), Some(vec![vec![vec![[0, 1]]]]));
}

#[test]
fn point_assignment_maps_two_distinct_endpoints_of_one_edge() {
    let make_quotient = || {
        MeshQuotient::new(vec![
            Arc::new(HashSet::from([0])),
            Arc::new(HashSet::from([1])),
        ])
    };
    assert!(crate::test_support::with_service_context(|ctx| {
        make_quotient().point_assignment(ctx, 2, &[vec![[0, 1]]], None)
    })
    .expect("compute the point assignment")
    .is_some());
}
