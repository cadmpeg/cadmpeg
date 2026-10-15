// SPDX-License-Identifier: Apache-2.0
//! component search tests.

use super::{Arc, HashSet, MeshPartialEndpointConstraint, MeshQuotient};
use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;
use crate::solve::missing_edge::MeshFaceBoundaryDomain;

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
    .expect("compute the component incidence pair solutions")
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
    .expect("compute the component incidence pair solutions")
    .expect("partially constrained component solutions");

    assert_eq!(solutions, vec![vec![[0, 0], [300, 300]]]);
}

#[test]
fn incidence_components_visit_every_independent_combination() {
    use std::ops::ControlFlow;

    const COMPONENT_COUNT: usize = 15;
    catia_test_context!(ctx);
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
    .expect("service decode");

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
        crate::solve::incidence::incidence_choice_components(&choices, &[[0, 0]; 3], None, None)
            .expect("compute the incidence choice components");

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
    .expect("visit component incidence pair solutions");

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
    quotient
        .merge(0, 1)
        .expect("service merge")
        .expect("first closed edge");
    quotient
        .merge(2, 3)
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
    .expect("compute the component incidence pair solutions")
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
        quotient
            .merge(edge * 2, edge * 2 + 1)
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
    .expect("visit component incidence pair solutions");

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
        .expect("compute the component incidence pair solution outcome"),
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
        .expect("compute the component incidence pair solution outcome"),
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
        .expect("compute the component incidence pair solution outcome"),
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
    .expect("visit component incidence pair solutions");

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
    .expect("visit component incidence pair solutions");

    assert_eq!(outcome, IncidenceSolve::Solved(2));
    assert_eq!(visited, 2);
    assert_eq!(validated.get(), 2);
}

#[test]
fn compact_boundary_advance_completes_a_two_edge_cycle() {
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::WorkBudget;
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
    let quotient =
        crate::solve::mesh_quotient::initial_mesh_quotient(&choices, 2, &[[0, 1], [2, 3]])
            .expect("compute the initial mesh quotient")
            .expect("initial quotient");
    let advance = |ctx: &DecodeContext<'_>| {
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
    assert!(advance(&service_ctx).expect("advance compact boundary domains"));
}
