use crate::solve::incidence::compact_boundary_domain_viable;
use crate::solve::mesh_quotient::AssignmentOrder;
use crate::solve::mesh_quotient::MeshPartialEndpointConstraint;
use crate::solve::mesh_quotient::MeshQuotient;
use crate::solve::mesh_quotient::MAX_MESH_CONSTRAINT_OPERATIONS;
use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
use crate::solve::missing_edge::MeshFaceBoundaryAssignment;
use crate::solve::missing_edge::MeshFaceBoundaryDomain;
use crate::solve::tests::repeated_domain;
use cadmpeg_core::decode::WorkBudget;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn incidence_components_join_only_through_shared_face_vertices() {
    catia_test_context!(ctx);
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 3], [2, 3]],
        vec![[4, 5], [4, 6]],
        vec![[7, 8]],
    ];
    let edge_faces = [[0, 0], [0, 0], [0, 0], [0, 0]];
    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &ctx,
            &choices,
            &edge_faces,
            None,
            None
        )
        .expect("service resource budget"),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn incidence_component_preflight_ignores_disjoint_unassigned_cycles_on_the_same_face() {
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0], [1, 1]], vec![[2, 2], [3, 3]]];
    let edge_faces = [[0, 0], [0, 0]];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        1,
        4,
        None,
        None,
        None,
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("independent same-face cycle solutions");

    assert_eq!(solutions.len(), 4);
}

#[test]
fn incidence_component_composition_does_not_allocate_the_declared_point_product() {
    catia_test_context!(ctx);
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &ctx,
        &[vec![[usize::MAX - 1, usize::MAX - 1]]],
        &[[0, 0]],
        1,
        usize::MAX,
        None,
        None,
        None,
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("sparse component degree state");

    assert_eq!(solutions, vec![vec![[usize::MAX - 1, usize::MAX - 1]]]);
}

#[test]
fn incidence_component_preflight_retains_fixed_chain_frontiers() {
    catia_test_context!(ctx);
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[1, 3]],
        vec![[3, 4], [3, 5]],
        vec![[4, 0]],
    ];
    let edge_faces = [[0, 0]; 4];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        1,
        6,
        None,
        None,
        None,
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("fixed-chain component frontier");

    assert_eq!(solutions, vec![vec![[0, 1], [1, 3], [3, 4], [4, 0]]]);
}

#[test]
fn partial_incidence_constraint_joins_every_component_it_can_couple() {
    catia_test_context!(ctx);
    let components = vec![vec![0, 2], vec![1], vec![3, 5], vec![4]];
    let active = [true, false, false, true, false, false];

    assert_eq!(
        crate::solve::incidence::join_incidence_components_by_coupling(&ctx, components, &active)
            .expect("service resource budget"),
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
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0], [1, 1]], vec![[2, 2], [3, 3]], vec![[4, 4]]];
    let mut components = vec![vec![0], vec![1], vec![2]];
    let dependencies = [Vec::new(), vec![0], Vec::new()];

    crate::solve::incidence::order_incidence_components_by_constraints(
        &ctx,
        &mut components,
        &choices,
        AssignmentOrder::new(None, Some(&dependencies)),
    )
    .expect("service resource budget")
    .expect("acyclic component prerequisites");

    assert_eq!(components, vec![vec![2], vec![0], vec![1]]);
}

#[test]
fn incidence_component_order_refuses_incoming_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let choices = vec![vec![[0, 0]], vec![[1, 1]]];
    let dependencies = [Vec::new(), vec![0]];
    let components = vec![vec![0], vec![1]];
    catia_test_context!(service_ctx);
    let mut service_components = components.clone();
    assert!(
        crate::solve::incidence::order_incidence_components_by_constraints(
            &service_ctx,
            &mut service_components,
            &choices,
            AssignmentOrder::new(None, Some(&dependencies)),
        )
        .expect("service budget")
        .is_some()
    );

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let mut limited_components = components;
    let error = crate::solve::incidence::order_incidence_components_by_constraints(
        &ctx,
        &mut limited_components,
        &choices,
        AssignmentOrder::new(None, Some(&dependencies)),
    )
    .expect_err("incoming collection exceeds the limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_incidence_component_in"));
}

#[test]
fn incidence_component_order_charges_outgoing_and_local_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let choices = vec![vec![[0, 0]], vec![[1, 1]]];
    let dependencies = [Vec::new(), vec![0]];
    let original = vec![vec![0], vec![1]];
    let mut operations = BTreeSet::new();
    let mut completed = false;
    for limit in 0..=64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let mut components = original.clone();
        match crate::solve::incidence::order_incidence_components_by_constraints(
            &ctx,
            &mut components,
            &choices,
            AssignmentOrder::new(None, Some(&dependencies)),
        ) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(Some(())) => {
                completed = true;
                break;
            }
            Ok(None) => panic!("acyclic component prerequisites must remain viable"),
            Err(error) => panic!("unexpected component refusal: {error}"),
        }
    }
    assert!(
        completed,
        "collection limit 64 must admit the component fixture"
    );
    assert!(operations.contains("catia_incidence_component_out"));
    assert!(operations.contains("catia_incidence_local_in"));
    assert!(operations.contains("catia_incidence_local_out"));
}

#[test]
fn incidence_component_search_charges_face_and_component_arrays() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;
    use std::ops::ControlFlow;

    let choices = vec![vec![[0, 0], [1, 1]], vec![[2, 2], [3, 3]]];
    let edge_faces = [[0, 0], [0, 0]];
    let mut operations = BTreeSet::new();
    let mut limit = 0;
    let mut completed = false;
    for _ in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match crate::solve::incidence::visit_component_incidence_pair_solutions(
            &ctx,
            &choices,
            &edge_faces,
            1,
            4,
            None,
            None,
            None,
            &|_| Ok(true),
            &mut |_| Ok(ControlFlow::Break(())),
        ) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
                let next = error.used + error.additional;
                assert!(next > limit, "a refusal must advance the collection cap");
                limit = next;
            }
            Ok(crate::solve::incidence::IncidenceSolve::Solved(1)) => {
                completed = true;
                break;
            }
            Ok(outcome) => panic!("independent edge choices must solve, got {outcome:?}"),
            Err(error) => panic!("unexpected component refusal: {error}"),
        }
    }
    assert!(
        completed,
        "adaptive caps must admit the independent edge fixture"
    );
    for operation in [
        "catia incidence active edges",
        "catia incidence point support edges",
        "catia incidence face edges",
        "catia incidence fixed edges",
        "catia incidence face degrees",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn incidence_components_reject_prerequisite_cycles() {
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0]], vec![[1, 1]]];
    let mut components = vec![vec![0], vec![1]];
    let predecessors = [Some(1), Some(0)];

    assert!(
        crate::solve::incidence::order_incidence_components_by_constraints(
            &ctx,
            &mut components,
            &choices,
            AssignmentOrder::new(Some(&predecessors), None),
        )
        .expect("service resource budget")
        .is_none()
    );

    let mut component = vec![vec![0, 1]];
    assert!(
        crate::solve::incidence::order_incidence_components_by_constraints(
            &ctx,
            &mut component,
            &choices,
            AssignmentOrder::new(Some(&predecessors), None),
        )
        .expect("service resource budget")
        .is_none()
    );
}

#[test]
fn incidence_components_keep_fixed_face_boundaries_independent() {
    catia_test_context!(ctx);
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
            &ctx,
            &choices,
            &edge_faces,
            Some(&fixed),
            None,
        )
        .expect("service resource budget"),
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
            &ctx,
            &choices,
            &edge_faces,
            Some(&alternatives),
            None,
        )
        .expect("service resource budget"),
        vec![vec![0, 1, 2, 3]]
    );
}

#[test]
fn incidence_components_include_overlapping_quotient_domains() {
    catia_test_context!(ctx);
    let choices = vec![
        vec![[0, 1], [0, 2]],
        vec![[3, 4], [3, 5]],
        vec![[6, 7], [6, 8]],
    ];
    let edge_faces = [[0, 0], [1, 1], [2, 2]];
    let quotient = MeshQuotient::new(
        [
            HashSet::from([0, 1]),
            HashSet::from([0, 2]),
            HashSet::from([1, 3]),
            HashSet::from([3, 4]),
            HashSet::from([6, 7]),
            HashSet::from([6, 8]),
        ]
        .map(Arc::new)
        .to_vec(),
    );

    assert_eq!(
        crate::solve::incidence::incidence_choice_components(
            &ctx,
            &choices,
            &edge_faces,
            None,
            Some(&quotient)
        )
        .expect("service resource budget"),
        vec![vec![0, 1], vec![2]]
    );
}

#[test]
fn incidence_components_solve_coupled_face_vertex_closures() {
    catia_test_context!(ctx);
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
        &ctx,
        &choices,
        &edge_faces,
        4,
        1970,
        None,
        None,
        None,
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("component closure solution");
    assert!(solutions
        .iter()
        .any(|solution| { solution[..5] == [[0, 2], [1, 3], [0, 12], [1, 1969], [0, 1]] }));
}

#[test]
fn incidence_components_reject_degree_cycles_in_the_wrong_edge_order() {
    catia_test_context!(ctx);
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
        &ctx,
        &choices,
        &edge_faces,
        1,
        4,
        Some(&mesh_assignments),
        None,
        None,
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("ordered component solution");

    assert_eq!(solutions.len(), 1);
    assert_eq!(solutions[0], [[0, 1], [1, 2], [2, 3], [3, 0]]);
}

#[test]
fn incidence_unordered_full_cycle_rejects_disconnected_degree_cycles() {
    catia_test_context!(ctx);
    let choices = vec![
        vec![[0, 1]],
        vec![[0, 1], [1, 2]],
        vec![[2, 3]],
        vec![[2, 3], [0, 3]],
    ];
    let edge_faces = [[0, 0]; 4];
    let domains = vec![MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3])];

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        1,
        4,
        Some(&domains),
        None,
        None,
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("connected cycle solution");

    assert_eq!(solutions, vec![vec![[0, 1], [1, 2], [2, 3], [0, 3]]]);
}

#[test]
fn deferred_boundary_enforces_anchored_gap_capacities() {
    catia_test_context!(ctx);
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

    assert!(
        crate::solve::incidence::deferred_boundary_closes(&ctx, &domain, &valid)
            .expect("service resource budget")
    );
    let assignment = crate::solve::incidence::deferred_boundary_assignment(&ctx, &domain, &valid)
        .expect("service resource budget")
        .expect("materialized deferred boundary");
    assert_eq!(
        assignment.boundaries[0]
            .iter()
            .map(|use_| (use_.edge, use_.start, use_.end, use_.reversed))
            .collect::<Vec<_>>(),
        vec![
            (0, 0, 1, Some(false)),
            (1, 1, 2, None),
            (2, 2, 3, None),
            (3, 3, 4, Some(false)),
            (4, 4, 5, None),
            (5, 5, 0, None),
        ]
    );
    assert!(!crate::solve::incidence::deferred_boundary_closes(
        &ctx,
        &domain,
        &overfilled_first_gap
    )
    .expect("service resource budget"));
}

#[test]
fn deferred_boundary_materialization_assigns_canonical_positive_spans() {
    catia_test_context!(ctx);
    let domain = crate::solve::missing_edge::MeshDeferredFaceBoundary {
        cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
            length: 4,
            exact_uses: Vec::new(),
        }],
        missing_edges: vec![0, 1],
    };
    let assignment =
        crate::solve::incidence::deferred_boundary_assignment(&ctx, &domain, &[[0, 1], [0, 1]])
            .expect("service resource budget")
            .expect("materialized deferred boundary");

    assert_eq!(
        assignment.boundaries[0]
            .iter()
            .map(|use_| (use_.edge, use_.start, use_.end))
            .collect::<Vec<_>>(),
        vec![(0, 0, 3), (1, 3, 0)]
    );
}

#[test]
fn deferred_boundary_materialization_refuses_match_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let domain = crate::solve::missing_edge::MeshDeferredFaceBoundary {
        cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
            length: 4,
            exact_uses: Vec::new(),
        }],
        missing_edges: vec![0, 1],
    };
    let pairs = [[0, 1], [0, 1]];
    catia_test_context!(service_ctx);
    assert!(
        crate::solve::incidence::deferred_boundary_assignment(&service_ctx, &domain, &pairs)
            .expect("service resource budget")
            .is_some()
    );

    let mut refused = HashSet::new();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match crate::solve::incidence::deferred_boundary_assignment(&ctx, &domain, &pairs) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(Some(_)) => break,
            Ok(None) => panic!("closed deferred boundary must materialize"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    for operation in [
        "catia_deferred_match",
        "catia_deferred_visit",
        "catia_deferred_boundaries",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn deferred_boundary_closure_refuses_matching_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let domain = crate::solve::missing_edge::MeshDeferredFaceBoundary {
        cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
            length: 4,
            exact_uses: Vec::new(),
        }],
        missing_edges: vec![0, 1],
    };
    let pairs = [[0, 1], [0, 1]];
    catia_test_context!(service_ctx);
    assert!(
        crate::solve::incidence::deferred_boundary_closes(&service_ctx, &domain, &pairs)
            .expect("service resource budget")
    );

    let mut refused = HashSet::new();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match crate::solve::incidence::deferred_boundary_closes(&ctx, &domain, &pairs) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(true) => break,
            Ok(false) => panic!("closed deferred boundary must match"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    for operation in ["catia_deferred_close_match", "catia_deferred_close_visit"] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn deferred_anchored_runs_propagate_forced_adjacencies() {
    catia_test_context!(ctx);
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
        crate::solve::mesh_quotient::initial_mesh_quotient(&ctx, &candidates, 2, &[[0, 1], [2, 3]])
            .expect("service resource budget")
            .expect("initial quotient");
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("service resource budget")
    .expect("forced deferred quotient");

    assert_eq!(quotient.find(0), quotient.find(3));
    assert_eq!(quotient.find(1), quotient.find(2));
}

#[test]
fn deferred_quotient_retains_unknown_exact_run_direction() {
    catia_test_context!(ctx);
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
        crate::solve::mesh_quotient::initial_mesh_quotient(&ctx, &candidates, 2, &[[0, 1], [2, 3]])
            .expect("service resource budget")
            .expect("initial quotient");
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("service resource budget")
    .expect("unknown exact direction is deferred");

    assert_ne!(quotient.find(0), quotient.find(3));
}

#[test]
fn deferred_gap_search_propagates_quotient_forced_edge_order() {
    catia_test_context!(ctx);
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
    let mut quotient = MeshQuotient::new(
        (0..8)
            .map(|node| Arc::new(HashSet::from([[0, 1, 2, 3, 1, 2, 3, 0][node]])))
            .collect(),
    );
    let budget = WorkBudget::new(10_000);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("service resource budget")
    .expect("deferred gap quotient");

    assert_eq!(quotient.find(1), quotient.find(4));
    assert_eq!(quotient.find(5), quotient.find(2));
    assert_eq!(quotient.find(3), quotient.find(6));
    assert_eq!(quotient.find(7), quotient.find(0));
}

#[test]
fn ordered_structural_equations_propagate_without_direction_enumeration() {
    catia_test_context!(ctx);
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
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 4));
    quotient.merge(0, 1).expect("first closed edge");
    quotient.merge(2, 3).expect("second closed edge");
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("service resource budget")
    .expect("structural quotient");

    assert_eq!(quotient.find(0), quotient.find(2));
}

#[test]
fn ordered_face_options_preflight_exact_signature_work() {
    catia_test_context!(ctx);
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
    let mut quotient = MeshQuotient::new(vec![broad; 4]);
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("service resource budget")
    .expect("bounded common quotient propagation");

    assert_eq!(quotient.root_count(), 4);
    assert!(!budget.exhausted());
}

#[test]
fn ordered_cycle_support_propagates_domain_forced_directions() {
    catia_test_context!(ctx);
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
    let mut quotient = MeshQuotient::new(vec![
        Arc::new(HashSet::from([0])),
        Arc::new(HashSet::from([1])),
        Arc::new(HashSet::from([1])),
        Arc::new(HashSet::from([0])),
    ]);
    let budget = WorkBudget::new(100);

    crate::solve::mesh_quotient::propagate_common_ordered_face_quotients(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
        &budget,
    )
    .expect("service resource budget")
    .expect("supported cycle quotient");

    assert_eq!(quotient.find(0), quotient.find(3));
    assert_eq!(quotient.find(1), quotient.find(2));
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
    let mut quotient = MeshQuotient::new(repeated_domain(HashSet::from([0, 1]), 4));
    catia_test_context!(ctx);

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("service resource budget")
    .expect("ordered component quotient");

    assert_eq!(quotient.find(0), quotient.find(3));
    assert_eq!(quotient.find(1), quotient.find(2));
}

#[test]
fn unordered_components_close_cycles_in_the_abstract_quotient() {
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![2, 0, 1])];
    let candidates = vec![Vec::new(); 3];
    let mut quotient = MeshQuotient::new(
        [0, 1, 1, 2, 2, 0]
            .into_iter()
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
    );
    catia_test_context!(ctx);

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("service resource budget")
    .expect("unordered component quotient");

    assert_eq!(quotient.find(1), quotient.find(2));
    assert_eq!(quotient.find(3), quotient.find(4));
    assert_eq!(quotient.find(5), quotient.find(0));
}

#[test]
fn compact_unordered_boundary_rejects_partial_subtours() {
    catia_test_context!(ctx);
    let domain = MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3, 4]);

    assert!(!compact_boundary_domain_viable(
        &ctx,
        &domain,
        &[Some([0, 1]), Some([1, 2]), Some([2, 0]), None, None],
        None,
    )
    .expect("service resource budget"));
    assert!(compact_boundary_domain_viable(
        &ctx,
        &domain,
        &[Some([0, 1]), Some([1, 2]), Some([2, 3]), None, None],
        None,
    )
    .expect("service resource budget"));
    assert!(compact_boundary_domain_viable(
        &ctx,
        &domain,
        &[
            Some([0, 1]),
            Some([1, 2]),
            Some([2, 3]),
            Some([3, 4]),
            Some([4, 0]),
        ],
        None,
    )
    .expect("service resource budget"));
}

#[test]
fn compact_partial_subtour_refuses_new_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let domain = MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2, 3, 4]);
    let assignment = [Some([0, 1]), Some([1, 2]), Some([2, 0]), Some([3, 4]), None];
    catia_test_context!(service_ctx);
    assert!(
        !compact_boundary_domain_viable(&service_ctx, &domain, &assignment, None)
            .expect("service resource budget")
    );

    let mut operations = HashSet::new();
    for limit in 0..=128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match compact_boundary_domain_viable(&ctx, &domain, &assignment, None) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(false) => {}
            Ok(true) => panic!("partial subtour must remain invalid"),
            Err(error) => panic!("unexpected compact boundary refusal: {error}"),
        }
    }
    for operation in [
        "catia_compact_boundary_nodes",
        "catia_compact_boundary_degrees",
        "catia_compact_boundary_points",
        "catia_compact_boundary_open_components",
    ] {
        assert!(operations.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn compact_unordered_boundary_refuses_degree_overflow() {
    catia_test_context!(ctx);
    let domain = MeshFaceBoundaryDomain::UnorderedFullCycle((0..129).collect());
    let mut assignment = vec![Some([0, 0]); 128];
    assignment.push(None);
    assert!(
        !compact_boundary_domain_viable(&ctx, &domain, &assignment, None)
            .expect("service resource budget")
    );
}

#[test]
fn compact_boundary_viability_refuses_labeled_edge_point_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let domain = MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2]);
    let assignment = [Some([0, 1]), Some([1, 2]), Some([2, 0])];
    catia_test_context!(service_ctx);
    assert!(
        compact_boundary_domain_viable(&service_ctx, &domain, &assignment, None)
            .expect("service resource budget")
    );

    let mut refused = HashSet::new();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match compact_boundary_domain_viable(&ctx, &domain, &assignment, None) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(true) => break,
            Ok(false) => panic!("closed cycle must remain viable"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    assert!(refused.contains("catia labeled edge points"));
    assert!(refused.contains("catia_compact_boundary_complete_pairs"));
}

#[test]
fn component_face_viability_refuses_edge_point_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let faces = HashSet::from([0]);
    let assignment = [Some([0, 1]), Some([1, 2]), Some([2, 0])];
    let choices = vec![vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]];
    let face_edges = vec![vec![0, 1, 2]];
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
    catia_test_context!(service_ctx);
    assert!(crate::solve::incidence::component_incidence_faces_viable(
        &service_ctx,
        &faces,
        &assignment,
        &choices,
        &face_edges,
        Some(&domains),
        3,
    )
    .expect("service resource budget"));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = crate::solve::incidence::component_incidence_faces_viable(
        &ctx,
        &faces,
        &assignment,
        &choices,
        &face_edges,
        Some(&domains),
        3,
    )
    .expect_err("edge point array exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia component incidence edge points"));
}

#[test]
fn unordered_component_enumeration_is_atomic_at_its_state_limit() {
    let quotient = MeshQuotient::new(repeated_domain(HashSet::from([0]), 16));
    let budget = WorkBudget::new(10_000);
    catia_test_context!(ctx);

    assert!(
        crate::solve::mesh_quotient::bounded_unordered_cycle_assignments(
            &ctx,
            &(0..8).collect::<Vec<_>>(),
            &quotient,
            16,
            &budget,
        )
        .expect("service resource budget")
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
    let mut quotient = MeshQuotient::new(
        (0..8)
            .map(|node| Arc::new(HashSet::from([[0, 1, 2, 3, 1, 2, 3, 0][node]])))
            .collect(),
    );
    catia_test_context!(ctx);

    crate::solve::mesh_quotient::propagate_common_boundary_components(
        &ctx,
        &domains,
        &candidates,
        &mut quotient,
    )
    .expect("service resource budget")
    .expect("deferred component quotient");

    assert_eq!(quotient.find(1), quotient.find(4));
    assert_eq!(quotient.find(5), quotient.find(2));
    assert_eq!(quotient.find(3), quotient.find(6));
    assert_eq!(quotient.find(7), quotient.find(0));
}

#[test]
fn deferred_faces_share_one_endpoint_quotient() {
    catia_test_context!(ctx);
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
        crate::solve::mesh_quotient::initial_mesh_quotient(&ctx, &choices, 2, &[[0, 1], [2, 3]])
            .expect("service resource budget")
            .expect("initial quotient");
    let budget = WorkBudget::new(10_000);

    assert!(
        crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &ctx,
            &[domain(false), domain(false)],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            &quotient,
            &budget,
        )
        .expect("service resource budget")
    );
    assert!(
        !crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &ctx,
            &[domain(false), domain(true)],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            &quotient,
            &budget,
        )
        .expect("service resource budget")
    );
}

#[test]
fn compact_faces_share_one_physical_edge_direction_gauge() {
    catia_test_context!(ctx);
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
        &ctx,
        &choices,
        4,
        &[[0, 1], [2, 3], [4, 5], [6, 7], [8, 9]],
    )
    .expect("service resource budget")
    .expect("initial quotient");
    let budget = WorkBudget::new(10_000);

    assert!(
        crate::solve::incidence::compact_boundary_domains_jointly_viable(
            &ctx,
            &domains,
            &choices,
            &assignment,
            None,
            &quotient,
            &budget,
        )
        .expect("service resource budget")
    );
}

#[test]
fn compact_face_quotient_states_accumulate_across_calls() {
    catia_test_context!(ctx);
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
        crate::solve::mesh_quotient::initial_mesh_quotient(&ctx, &choices, 2, &[[0, 1], [2, 3]])
            .expect("service resource budget")
            .expect("initial quotient");
    let budget = WorkBudget::new(10_000);
    let first = domain(false);
    let conflicting = domain(true);
    let initial = vec![(quotient.clone(), HashSet::new())];

    let crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(first_states) =
        crate::solve::incidence::advance_compact_boundary_domains(
            &ctx,
            [&first],
            &choices,
            &assignment,
            None,
            initial.clone(),
            &budget,
        )
        .expect("service resource budget")
    else {
        panic!("first face quotient");
    };
    assert!(matches!(
        crate::solve::incidence::advance_compact_boundary_domains(
            &ctx,
            [&conflicting],
            &choices,
            &assignment,
            None,
            initial,
            &budget,
        )
        .expect("service resource budget"),
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)
    ));
    assert!(matches!(
        crate::solve::incidence::advance_compact_boundary_domains(
            &ctx,
            [&conflicting],
            &choices,
            &assignment,
            None,
            first_states,
            &budget,
        )
        .expect("service resource budget"),
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Rejected
    ));
}

#[test]
fn compact_face_quotient_state_cap_is_exhausted() {
    const EDGE_COUNT: usize = 14;
    catia_test_context!(ctx);
    let choices = vec![Vec::new(); EDGE_COUNT];
    let quotient = MeshQuotient::new(
        (0..EDGE_COUNT * 2)
            .map(|node| {
                Arc::new(if node % 2 == 0 {
                    HashSet::from([0, 1])
                } else {
                    HashSet::from([1, 2])
                })
            })
            .collect(),
    );
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
        &ctx,
        [&domain],
        &choices,
        &assignment,
        None,
        vec![(quotient, HashSet::new())],
        &budget,
    )
    .expect("service resource budget");
    assert!(matches!(
        outcome,
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Exhausted
    ));
    assert!(!budget.exhausted());
}

#[test]
fn compact_boundary_advance_refuses_edge_point_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: (edge + 1) % 2,
        reversed: Some(false),
    };
    let domain = MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 2,
                exact_uses: vec![(use_(0), 1), (use_(1), 1)],
            }],
            missing_edges: Vec::new(),
        },
    );
    catia_test_context!(service_ctx);
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &service_ctx,
        &choices,
        2,
        &[[0, 1], [2, 3]],
    )
    .expect("service resource budget")
    .expect("initial quotient");
    let budget = WorkBudget::new(10_000);
    let service = crate::solve::incidence::advance_compact_boundary_domains(
        &service_ctx,
        [&domain],
        &choices,
        &[Some([0, 1]), Some([0, 1])],
        None,
        vec![(quotient.clone(), HashSet::new())],
        &budget,
    )
    .expect("service resource budget");
    assert!(matches!(
        service,
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)
    ));

    let mut refused = HashSet::new();
    for limit in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let budget = WorkBudget::new(10_000);
        match crate::solve::incidence::advance_compact_boundary_domains(
            &ctx,
            [&domain],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            vec![(quotient.clone(), HashSet::new())],
            &budget,
        ) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)) => break,
            Ok(_) => panic!("closed compact boundary must advance"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    for operation in [
        "catia compact boundary edges",
        "catia compact boundary selected edges",
        "catia compact boundary edge points",
        "catia_compact_boundary_candidate_pair",
        "catia_compact_boundary_candidate_rows",
        "catia_compact_boundary_oriented_edges",
        "catia_compact_boundary_oriented_signature",
        "catia_compact_boundary_signatures",
        "catia_compact_boundary_next_states",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn compact_boundary_advance_refuses_nested_ordered_alternative_copies() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: (edge + 1) % 2,
        reversed: Some(false),
    };
    let domain = MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![use_(0), use_(1)]],
    }]);
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    catia_test_context!(service_ctx);
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &service_ctx,
        &choices,
        2,
        &[[0, 1], [2, 3]],
    )
    .expect("service resource budget")
    .expect("initial quotient");
    let run = |ctx: &DecodeContext<'_>| {
        let budget = WorkBudget::new(10_000);
        crate::solve::incidence::advance_compact_boundary_domains(
            ctx,
            [&domain],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            vec![(quotient.clone(), HashSet::new())],
            &budget,
        )
    };
    assert!(matches!(
        run(&service_ctx).expect("service resource budget"),
        crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)
    ));

    let mut refused = HashSet::new();
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation);
            }
            Ok(crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)) => break,
            Ok(_) => panic!("ordered compact boundary must advance"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    for operation in [
        "catia compact boundary alternatives",
        "catia compact boundary alternative cycles",
        "catia compact boundary alternative uses",
        "catia compact boundary domain rows",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn compact_boundary_advance_charges_existing_oriented_edges() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: edge,
        end: (edge + 1) % 2,
        reversed: Some(false),
    };
    let domain = MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 2,
                exact_uses: vec![(use_(0), 1), (use_(1), 1)],
            }],
            missing_edges: Vec::new(),
        },
    );
    catia_test_context!(service_ctx);
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let quotient = crate::solve::mesh_quotient::initial_mesh_quotient(
        &service_ctx,
        &choices,
        2,
        &[[0, 1], [2, 3]],
    )
    .expect("service resource budget")
    .expect("initial quotient");
    let initial = vec![(quotient, HashSet::from([0]))];
    let mut refused = HashSet::new();
    for limit in 0..256 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let budget = WorkBudget::new(10_000);
        match crate::solve::incidence::advance_compact_boundary_domains(
            &ctx,
            [&domain],
            &choices,
            &[Some([0, 1]), Some([0, 1])],
            None,
            initial.clone(),
            &budget,
        ) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)) => break,
            Ok(_) => panic!("closed compact boundary must advance"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    assert!(refused.contains("catia_compact_boundary_oriented_copy"));
}

#[test]
fn incidence_components_filter_complete_solutions_during_search() {
    catia_test_context!(ctx);
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let edge_faces = [[0, 0]; 4];
    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        1,
        4,
        None,
        None,
        None,
        &|pairs| Ok(pairs[1] == [2, 3]),
    )
    .expect("service resource budget")
    .expect("filtered component solution");

    assert_eq!(solutions, vec![vec![[0, 1], [2, 3], [1, 2], [3, 0]]]);
}

#[test]
fn incidence_components_apply_monotone_partial_constraints_before_solution_limits() {
    catia_test_context!(ctx);
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
        &ctx,
        &choices,
        &edge_faces,
        2,
        600,
        None,
        None,
        Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_order: None,
            valid: &partial,
        }),
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("partially constrained component solutions");

    assert_eq!(solutions, vec![vec![[0, 0], [300, 300]]]);
}

#[test]
fn incidence_components_reuse_independent_solution_domains() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::ops::ControlFlow;

    const COMPONENT_COUNT: usize = 15;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 10_000_000;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
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
        &ctx,
        &choices,
        &edge_faces,
        COMPONENT_COUNT,
        COMPONENT_COUNT * 2,
        None,
        None,
        None,
        &|_| Ok(true),
        &mut |_| {
            visited += 1;
            Ok(ControlFlow::Continue(()))
        },
    )
    .expect("collection budget admits all independent solutions");

    assert_eq!(
        outcome,
        crate::solve::incidence::IncidenceSolve::Solved(1 << COMPONENT_COUNT)
    );

    catia_test_context!(service_ctx);
    let error = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &service_ctx,
        &choices,
        &edge_faces,
        COMPONENT_COUNT,
        COMPONENT_COUNT * 2,
        None,
        None,
        None,
        &|_| Ok(true),
        &mut |_| Ok(ControlFlow::Continue(())),
    )
    .expect_err("repeated input-sized collections exceed the service cap");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems));
    assert_eq!(visited, 1 << COMPONENT_COUNT);
}

#[test]
fn incidence_components_include_fixed_incidence_chains() {
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0], [0, 1]], vec![[2, 2], [2, 3]], vec![[0, 2]]];
    let components = crate::solve::incidence::incidence_choice_components(
        &ctx,
        &choices,
        &[[0, 0]; 3],
        None,
        None,
    )
    .expect("service resource budget");

    assert_eq!(components, vec![vec![0, 1]]);
}

#[test]
fn incidence_components_preflight_independent_unsatisfiable_domains() {
    use std::ops::ControlFlow;

    const BROAD_COMPONENT_COUNT: usize = 15;
    catia_test_context!(ctx);
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
        &ctx,
        &choices,
        &edge_faces,
        choices.len(),
        34,
        None,
        None,
        Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_order: None,
            valid: &partial,
        }),
        &|_| Ok(true),
        &mut |_| {
            visited = true;
            Ok(ControlFlow::Continue(()))
        },
    )
    .expect("service resource budget");

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
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0], [1, 1]], vec![[1, 1]]];
    let edge_faces = [[0, 0], [1, 1]];
    let mut quotient = MeshQuotient::new((0..4).map(|_| Arc::new(HashSet::from([0, 1]))).collect());
    quotient.merge(0, 1).expect("first closed edge");
    quotient.merge(2, 3).expect("second closed edge");

    let solutions = crate::solve::incidence::component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        2,
        2,
        None,
        Some(&quotient),
        None,
        &|_| Ok(true),
    )
    .expect("service resource budget")
    .expect("globally assignable component solution");

    assert_eq!(solutions, vec![vec![[0, 0], [1, 1]]]);
}

#[test]
fn incidence_components_preflight_quotient_impossible_domains() {
    use std::ops::ControlFlow;

    const BROAD_COMPONENT_COUNT: usize = 15;
    catia_test_context!(ctx);
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
    let mut domains = Vec::with_capacity(choices.len() * 2);
    for edge in 0..choices.len() {
        let points = if edge < BROAD_COMPONENT_COUNT {
            HashSet::from([edge * 2, edge * 2 + 1])
        } else {
            HashSet::from([30, 31])
        };
        let points = Arc::new(points);
        domains.extend([points.clone(), points]);
    }
    let mut quotient = MeshQuotient::new(domains);
    for edge in 0..choices.len() {
        quotient.merge(edge * 2, edge * 2 + 1).expect("closed edge");
    }
    let mut visited = false;

    let outcome = crate::solve::incidence::visit_component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        choices.len(),
        32,
        None,
        Some(&quotient),
        None,
        &|_| Ok(true),
        &mut |_| {
            visited = true;
            Ok(ControlFlow::Continue(()))
        },
    )
    .expect("service resource budget");

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
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0]], vec![[0, 0]]];
    let edge_faces = [[0, 0], [1, 1]];
    let quotient = MeshQuotient::new((0..4).map(|_| Arc::new(HashSet::from([0]))).collect());

    assert_eq!(
        crate::solve::incidence::component_incidence_pair_solution_outcome(
            &ctx,
            &choices,
            &edge_faces,
            2,
            1,
            None,
            Some(&quotient),
            None,
            &|_| Ok(true),
        )
        .expect("service resource budget"),
        crate::solve::incidence::IncidenceSolve::Rejected(
            crate::solve::incidence::IncidenceRejection::FixedAssignment
        )
    );
}

#[test]
fn incidence_outcome_distinguishes_exhaustion_from_rejection() {
    use crate::solve::incidence::{component_incidence_pair_solution_outcome, IncidenceSolve};

    catia_test_context!(ctx);
    let choices = vec![(0..300).map(|point| [point, point]).collect::<Vec<_>>()];
    assert_eq!(
        component_incidence_pair_solution_outcome(
            &ctx,
            &choices,
            &[[0, 0]],
            1,
            300,
            None,
            None,
            None,
            &|_| Ok(true),
        )
        .expect("service resource budget"),
        IncidenceSolve::Exhausted
    );
    assert_eq!(
        component_incidence_pair_solution_outcome(
            &ctx,
            &[Vec::new()],
            &[[0, 0]],
            1,
            1,
            None,
            None,
            None,
            &|_| Ok(true),
        )
        .expect("service resource budget"),
        IncidenceSolve::Rejected(crate::solve::incidence::IncidenceRejection::FixedAssignment)
    );
}

#[test]
fn incidence_component_products_stream_until_the_consumer_stops() {
    use crate::solve::incidence::{visit_component_incidence_pair_solutions, IncidenceSolve};
    use std::ops::ControlFlow;

    catia_test_context!(ctx);
    let choices = (0..9)
        .map(|edge| vec![[edge * 2, edge * 2], [edge * 2 + 1, edge * 2 + 1]])
        .collect::<Vec<_>>();
    let edge_faces = (0..9).map(|face| [face, face]).collect::<Vec<_>>();
    let mut visited = 0usize;

    let outcome = visit_component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        9,
        18,
        None,
        None,
        None,
        &|_| Ok(true),
        &mut |_| {
            visited += 1;
            if visited == 2 {
                Ok(ControlFlow::Break(()))
            } else {
                Ok(ControlFlow::Continue(()))
            }
        },
    )
    .expect("service resource budget");

    assert_eq!(outcome, IncidenceSolve::Solved(2));
    assert_eq!(visited, 2);
}

#[test]
fn incidence_component_prefix_can_prove_the_consumer_result_before_exhaustion() {
    use crate::solve::incidence::{visit_component_incidence_pair_solutions, IncidenceSolve};
    use std::cell::Cell;
    use std::ops::ControlFlow;

    catia_test_context!(ctx);
    let choices = vec![(0..300).map(|point| [point, point]).collect::<Vec<_>>()];
    let mut visited = 0usize;
    let validated = Cell::new(0usize);
    let outcome = visit_component_incidence_pair_solutions(
        &ctx,
        &choices,
        &[[0, 0]],
        1,
        300,
        None,
        None,
        None,
        &|_| {
            validated.set(validated.get() + 1);
            Ok(true)
        },
        &mut |_| {
            visited += 1;
            if visited == 2 {
                Ok(ControlFlow::Break(()))
            } else {
                Ok(ControlFlow::Continue(()))
            }
        },
    )
    .expect("service resource budget");

    assert_eq!(outcome, IncidenceSolve::Solved(2));
    assert_eq!(visited, 2);
    assert_eq!(validated.get(), 2);
}
