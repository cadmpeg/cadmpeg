// SPDX-License-Identifier: Apache-2.0
//! face configuration tests.

use super::{
    prepare_face_configuration_domains, prune_face_configuration_singleton_support,
    prune_face_configuration_support, prune_ordered_face_endpoint_support, sparse_degrees,
    BTreeMap, HashMap, HashSet, IncidenceSearchState, MeshBoundaryEdgeCandidate,
    MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain, RefCell, WorkBudget,
    MAX_MESH_CONSTRAINT_OPERATIONS,
};

#[test]
fn incidence_face_factor_allocations_refuse_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let edge = MeshBoundaryEdgeCandidate {
        edge: 0,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let assignments = vec![
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![edge]],
        }]),
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![edge]],
        }]),
    ];
    let choices = vec![vec![[0, 0], [1, 1]]];
    catia_test_context!(service_ctx);
    assert!(prepare_face_configuration_domains(
        &service_ctx,
        Some(&assignments),
        &choices,
        &[None],
        &[true],
    )
    .expect("service resource budget")
    .is_some());

    let mut refused = HashSet::new();
    for limit in 0..512 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match prepare_face_configuration_domains(
            &ctx,
            Some(&assignments),
            &choices,
            &[None],
            &[true],
        ) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                refused.insert(error.operation.to_owned());
            }
            Ok(Some(_)) => break,
            Ok(None) => panic!("ordered face factors must be available"),
            Err(error) => panic!("unexpected refusal: {error}"),
        }
    }
    for operation in [
        "catia_face_factor_domains",
        "catia face configuration neighbors",
        "catia_face_config_present",
        "catia_face_config_matching",
        "catia_face_config_viable",
        "catia_face_factor_incoming",
        "catia_face_config_full_mask",
        "catia_face_factor_by_face",
        "catia_face_factors_by_edge",
    ] {
        assert!(refused.contains(operation), "no refusal at {operation}");
    }
}

#[test]
fn incidence_face_configuration_support_retains_shared_edge_correlations() {
    catia_test_context!(ctx);
    let correlated = vec![
        vec![(0, [0, 1]), (1, [2, 3])],
        vec![(0, [4, 5]), (1, [6, 7])],
    ];
    let mut domains = vec![correlated.clone(), vec![vec![(0, [0, 1]), (1, [2, 3])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(
        prune_face_configuration_support(&ctx, &mut domains, &budget)
            .expect("service resource budget")
    );
    assert_eq!(domains[0], vec![correlated[0].clone()]);

    let mut incompatible = vec![correlated, vec![vec![(0, [0, 1]), (1, [6, 7])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(
        !prune_face_configuration_support(&ctx, &mut incompatible, &budget)
            .expect("service resource budget")
    );

    let preserved = incompatible.clone();
    let budget = WorkBudget::new(0);
    assert!(
        prune_face_configuration_support(&ctx, &mut incompatible, &budget)
            .expect("service resource budget")
    );
    assert_eq!(incompatible, preserved);
    assert!(budget.exhausted());
}

#[test]
fn incidence_face_configuration_support_propagates_across_a_factor_chain() {
    catia_test_context!(ctx);
    let mut domains = vec![
        vec![vec![(0, [0, 1])], vec![(0, [0, 2])]],
        vec![
            vec![(0, [0, 1]), (1, [1, 2])],
            vec![(0, [0, 2]), (1, [2, 3])],
        ],
        vec![vec![(1, [1, 2])]],
    ];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(
        prune_face_configuration_support(&ctx, &mut domains, &budget)
            .expect("service resource budget")
    );
    assert_eq!(
        domains,
        vec![
            vec![vec![(0, [0, 1])]],
            vec![vec![(0, [0, 1]), (1, [1, 2])]],
            vec![vec![(1, [1, 2])]],
        ]
    );

    let mut optional = vec![vec![vec![(0, [0, 1])]], vec![vec![], vec![(0, [0, 2])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(
        prune_face_configuration_support(&ctx, &mut optional, &budget)
            .expect("service resource budget")
    );
    assert_eq!(optional[0], vec![vec![(0, [0, 1])]]);
}

#[test]
fn incidence_face_singleton_support_rejects_an_inconsistent_factor_cycle() {
    catia_test_context!(ctx);
    let equal = |left, right| {
        vec![
            vec![(left, [0, 0]), (right, [0, 0])],
            vec![(left, [1, 1]), (right, [1, 1])],
        ]
    };
    let different = vec![
        vec![(0, [0, 0]), (2, [1, 1])],
        vec![(0, [1, 1]), (2, [0, 0])],
    ];
    let mut inconsistent = vec![equal(0, 1), equal(1, 2), different];
    let arc_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(
        prune_face_configuration_support(&ctx, &mut inconsistent, &arc_budget)
            .expect("service resource budget")
    );
    assert!(inconsistent.iter().all(|domain| domain.len() == 2));

    let singleton_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(!prune_face_configuration_singleton_support(
        &ctx,
        &mut inconsistent,
        &singleton_budget,
    )
    .expect("service resource budget"));

    let mut consistent = vec![equal(0, 1), equal(1, 2), equal(2, 0)];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    assert!(
        prune_face_configuration_singleton_support(&ctx, &mut consistent, &budget,)
            .expect("service resource budget")
    );
    assert!(consistent.iter().all(|domain| domain.len() == 2));

    let preserved = consistent.clone();
    let exhausted = WorkBudget::new(0);
    assert!(
        prune_face_configuration_singleton_support(&ctx, &mut consistent, &exhausted,)
            .expect("service resource budget")
    );
    assert_eq!(consistent, preserved);
    assert!(exhausted.exhausted());
}

#[test]
fn incidence_face_singleton_support_tracks_multiword_configuration_masks() {
    catia_test_context!(ctx);
    let wide = (0..130)
        .map(|point| vec![(0, [point, point])])
        .collect::<Vec<_>>();
    let mut domains = vec![wide, vec![vec![(0, [129, 129])]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(
        prune_face_configuration_singleton_support(&ctx, &mut domains, &budget,)
            .expect("service resource budget")
    );
    assert_eq!(domains[0], vec![vec![(0, [129, 129])]]);
    assert_eq!(domains[1], vec![vec![(0, [129, 129])]]);
}

#[test]
fn ordered_face_support_prunes_edge_pairs_to_complete_configurations() {
    catia_test_context!(ctx);
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: None,
    };
    let domains = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let mut choices = vec![vec![[0, 1], [0, 2], [3, 4]], vec![[0, 1], [0, 2]]];
    let budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);

    assert!(
        prune_ordered_face_endpoint_support(&ctx, &domains, &mut choices, &budget,)
            .expect("service resource budget")
    );
    assert_eq!(choices, vec![vec![[0, 1], [0, 2]], vec![[0, 1], [0, 2]]]);

    let mut unpruned = vec![vec![[0, 1], [0, 2], [3, 4]], vec![[0, 1], [0, 2]]];
    let exhausted = WorkBudget::new(0);
    assert!(
        prune_ordered_face_endpoint_support(&ctx, &domains, &mut unpruned, &exhausted,)
            .expect("service resource budget")
    );
    assert_eq!(
        unpruned,
        vec![vec![[0, 1], [0, 2], [3, 4]], vec![[0, 1], [0, 2]]]
    );
    assert!(exhausted.exhausted());
}

#[test]
fn ordered_face_support_refuses_selection_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: None,
    };
    let domains = [MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    catia_test_context!(service_ctx);
    let mut service_choices = choices.clone();
    assert!(prune_ordered_face_endpoint_support(
        &service_ctx,
        &domains,
        &mut service_choices,
        &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
    )
    .expect("service budget"));

    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "catia_ordered_face_selection",
        |cap| {
            crate::test_support::with_collection_limit(cap, |ctx| {
                let mut limited_choices = choices.clone();
                prune_ordered_face_endpoint_support(
                    ctx,
                    &domains,
                    &mut limited_choices,
                    &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
                )
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_ordered_face_selection"));
}

#[test]
fn incidence_forced_face_chain_does_not_consume_branch_budget() {
    catia_test_context!(ctx);
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![vec![[0, 0]], vec![[1, 1]]];
    let edge_faces = [[0, 0], [1, 1]];
    let face_edges = vec![vec![0], vec![1]];
    let assignments = vec![
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0)]],
        }]),
        MeshFaceBoundaryDomain::Ordered(vec![MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(1)]],
        }]),
    ];
    let budget = WorkBudget::new(1);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        ctx: &ctx,
        search_storage: RefCell::new(
            ctx.reserve_scoped(0, "search test storage")
                .expect("storage"),
        ),
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None, None],
        degrees: vec![BTreeMap::new(), BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashMap::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        state: IncidenceSearchState::Open,
    };

    search.search().expect("service resource budget");

    assert_ne!(search.state, IncidenceSearchState::Exhausted);
    assert_eq!(search.solutions, vec![vec![(0, [0, 0]), (1, [1, 1])]]);
}

#[test]
fn incidence_forced_face_configuration_closes_its_frontier_atomically() {
    catia_test_context!(ctx);
    let use_ = |edge| MeshBoundaryEdgeCandidate {
        edge,
        start: 0,
        end: 0,
        reversed: Some(false),
    };
    let choices = vec![vec![[0, 1]], vec![[0, 1]]];
    let edge_faces = [[0, 0], [0, 0]];
    let face_edges = vec![vec![0, 1]];
    let assignments = vec![MeshFaceBoundaryDomain::Ordered(vec![
        MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_(0), use_(1)]],
        },
    ])];
    let budget = WorkBudget::new(1);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        ctx: &ctx,
        search_storage: RefCell::new(
            ctx.reserve_scoped(0, "search test storage")
                .expect("storage"),
        ),
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, true],
        edges: &[0, 1],
        constraints: vec![(0, 0), (0, 1)],
        assignment: vec![None, None],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashMap::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        state: IncidenceSearchState::Open,
    };

    search.search().expect("service resource budget");

    assert_ne!(search.state, IncidenceSearchState::Exhausted);
    assert_eq!(search.solutions, vec![vec![(0, [0, 1]), (1, [0, 1])]]);
}

#[test]
fn incidence_candidate_uses_a_separate_global_quotient_validation_budget() {
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0]]];
    let edge_faces = [[0, 0]];
    let face_edges = vec![vec![0]];
    let assignments = vec![MeshFaceBoundaryDomain::DeferredValidation(
        crate::solve::missing_edge::MeshDeferredFaceBoundary {
            cycles: vec![crate::solve::missing_edge::MeshDeferredBoundaryCycle {
                length: 1,
                exact_uses: vec![(
                    MeshBoundaryEdgeCandidate {
                        edge: 0,
                        start: 0,
                        end: 0,
                        reversed: Some(false),
                    },
                    1,
                )],
            }],
            missing_edges: Vec::new(),
        },
    )];
    let budget = WorkBudget::new(0);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        ctx: &ctx,
        search_storage: RefCell::new(
            ctx.reserve_scoped(0, "search test storage")
                .expect("storage"),
        ),
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true],
        edges: &[0],
        constraints: vec![(0, 0)],
        assignment: vec![None],
        degrees: vec![BTreeMap::new()],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashMap::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        state: IncidenceSearchState::Open,
    };

    assert!(search
        .candidate_fits(0, [0, 0])
        .expect("service resource budget"));
    assert!(!budget.exhausted());
    search.adjust(0, [0, 0]).expect("service resource budget");
    search.assignment[0] = Some([0, 0]);
    assert!(search
        .ordered_faces_feasible([0])
        .expect("service resource budget"));
    assert!(!budget.exhausted());
}

#[test]
fn incidence_selection_validates_only_its_affected_faces() {
    catia_test_context!(ctx);
    let choices = vec![vec![[0, 0]], vec![[0, 1]]];
    let edge_faces = [[0, 0], [1, 1]];
    let face_edges = vec![vec![0], vec![1]];
    let assignments = vec![
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0]),
        MeshFaceBoundaryDomain::UnorderedFullCycle(vec![1]),
    ];
    let budget = WorkBudget::new(1_000);
    let propagation_budget = WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS);
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        ctx: &ctx,
        search_storage: RefCell::new(
            ctx.reserve_scoped(0, "search test storage")
                .expect("storage"),
        ),
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &edge_faces,
        face_edges: &face_edges,
        mesh_assignments: Some(&assignments),
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true, false],
        edges: &[0],
        constraints: vec![(0, 0)],
        assignment: vec![Some([0, 0]), Some([0, 1])],
        degrees: sparse_degrees(&[&[2, 0], &[1, 1]]),
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashMap::new(),
        budget: &budget,
        degree_support_budget: &propagation_budget,
        coordinate_propagation_budget: &propagation_budget,
        boundary_propagation_budget: &propagation_budget,
        state: IncidenceSearchState::Open,
    };

    assert!(search
        .ordered_faces_feasible([0])
        .expect("service resource budget"));
    assert!(!search
        .ordered_faces_feasible([1])
        .expect("service resource budget"));
}
