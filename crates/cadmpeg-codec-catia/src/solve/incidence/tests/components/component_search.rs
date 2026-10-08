// SPDX-License-Identifier: Apache-2.0
//! component search tests.

use cadmpeg_core::CodecError;

use super::{
    Arc, HashSet, MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment, MeshFaceBoundaryDomain,
    MeshPartialEndpointConstraint, MeshQuotient, WorkBudget,
};

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
        };
    }
    for operation in [
        "catia compact boundary edges",
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
        .map(|outcome| {
            matches!(
                outcome,
                crate::solve::incidence::CompactBoundaryAdvanceOutcome::Complete(_)
            )
        })
    };
    assert!(run(&service_ctx).expect("service resource budget"));

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
            Ok(true) => break,
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
        };
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
        Ok(assignment[0].is_none_or(|pair| pair == [0, 0])
            && assignment[1].is_none_or(|pair| pair == [300, 300]))
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
fn partial_constraint_refusal_propagates_from_component_search() {
    catia_test_context!(ctx);
    let choices = vec![
        vec![[0, 1]],
        vec![[1, 2], [2, 3]],
        vec![[2, 3], [1, 2]],
        vec![[3, 0]],
    ];
    let edge_faces = [[0, 0]; 4];
    let active_edges = [true; 4];
    let refusal = |_: &[Option<[usize; 2]>]| {
        Err(ctx.refuse_codec_limit("catia test partial constraint", 0, 1))
    };
    let result = crate::solve::incidence::component_incidence_pair_solutions(
        &ctx,
        &choices,
        &edge_faces,
        1,
        4,
        None,
        None,
        Some(MeshPartialEndpointConstraint {
            active_edges: &active_edges,
            coupled_edges: &active_edges,
            assignment_order: None,
            valid: &refusal,
        }),
        &|_| Ok(true),
    );

    let Err(CodecError::ResourceLimit(returned)) = result else {
        panic!("partial constraint refusal must reach the component search caller");
    };
    assert_eq!(returned.operation, "catia test partial constraint");
    assert_eq!(ctx.resource_refusal(), Some(returned));
}

#[test]
fn incidence_components_reuse_independent_solution_domains() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::ops::ControlFlow;

    const COMPONENT_COUNT: usize = 15;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Global byte work admits all states; the local search budget and collection ceiling own this assertion.
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_collection_items = 10_000_000;

    let choices = (0..COMPONENT_COUNT)
        .map(|component| {
            let first = component * 2;
            vec![[first, first], [first + 1, first + 1]]
        })
        .collect::<Vec<_>>();
    let edge_faces = (0..COMPONENT_COUNT)
        .map(|face| [face, face])
        .collect::<Vec<_>>();
    let expected_solutions = (0..1_usize << COMPONENT_COUNT)
        .map(|mask| {
            (0..COMPONENT_COUNT)
                .map(|component| choices[component][(mask >> component) & 1])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    // The root contains every expected solution row; retained admission covers the complete enumeration.
    let source = serde_json::to_vec(&expected_solutions).expect("solution rows serialize");
    let (ctx, _) =
        DecodeContext::from_root_bytes(&source, &arena, &policy).expect("solution input");
    let mut remaining = expected_solutions
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<_>>();
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
        &mut |solution| {
            assert!(
                remaining.remove(solution),
                "each expected solution occurs once"
            );
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
    assert_eq!(visited, expected_solutions.len());
    assert!(remaining.is_empty());
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
    let partial = |assignment: &[Option<[usize; 2]>]| Ok(assignment[constrained_edge].is_none());
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
    crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 0, 1))
        .expect("service merge")
        .expect("first closed edge");
    crate::test_support::with_service_context(|ctx| quotient.merge_charged(ctx, 2, 3))
        .expect("service merge")
        .expect("second closed edge");

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
        crate::test_support::with_service_context(|ctx| {
            quotient.merge_charged(ctx, edge * 2, edge * 2 + 1)
        })
        .expect("service merge")
        .expect("closed edge");
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

#[test]
fn rejected_complete_branch_retains_no_solution() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty fixture fits the input limit");
    let budget = ctx.work_budget(10_000);
    let reject = |_: &[(usize, [usize; 2])]| Ok(false);
    let mut search = super::super::super::IncidenceComponentSearch {
        ctx: &ctx,
        choices: &[vec![[0, 1]]],
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: std::cell::RefCell::new(std::collections::HashMap::new()),
        edge_faces: &[[0, 0]],
        face_edges: &[vec![0]],
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true],
        edges: &[0],
        constraints: Vec::new(),
        assignment: vec![Some([0, 1])],
        degrees: Vec::new(),
        solutions: Vec::new(),
        solution_filter: Some(&reject),
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: std::collections::HashMap::new(),
        budget: &budget,
        degree_support_budget: &budget,
        coordinate_propagation_budget: &budget,
        boundary_propagation_budget: &budget,
        state: super::super::super::IncidenceSearchState::Open,
        search_storage: std::cell::RefCell::new(
            ctx.reserve_scoped(0, "catia rejected branch")
                .expect("empty storage"),
        ),
    };
    search
        .search_edge_state(&[], None, &[])
        .expect("rejected solution stays temporary");
    assert!(search.solutions.is_empty());
    assert!(matches!(
        search.state,
        super::super::super::IncidenceSearchState::Open
    ));
}

#[test]
fn unordered_boundary_queries_retain_no_cycle_outputs() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("fixture input");
    let domains = [MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0, 1, 2])];
    let points = [[0, 1], [1, 2], [0, 2]];
    assert!(
        super::super::super::boundary_domains_close(&ctx, Some(&domains), &points)
            .expect("temporary cycles")
    );
    assert!(super::super::super::component_incidence_faces_viable(
        &ctx,
        &[0],
        &points.map(Some),
        &points.map(|pair| vec![pair]),
        &[vec![0, 1, 2]],
        Some(&domains),
        3,
    )
    .expect("temporary component cycles"));
}
