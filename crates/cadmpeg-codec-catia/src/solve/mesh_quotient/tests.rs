// SPDX-License-Identifier: Apache-2.0
//! Work-estimate tests for the mesh quotient search.

use super::{direction_work_estimate, MeshQuotient, MeshSelectionSearch, SearchOutcome};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::WorkBudget;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn direction_work_estimate_states_no_figure_the_work_counter_cannot_hold() {
    assert_eq!(direction_work_estimate([0usize, 1, 2].into_iter()), Some(7));
    let widest = usize::BITS as usize - 1;
    assert_eq!(
        direction_work_estimate([widest].into_iter()),
        Some(1usize << widest)
    );
    for unknown in [usize::BITS as usize, usize::MAX] {
        assert_eq!(direction_work_estimate([unknown].into_iter()), None);
    }
    assert_eq!(direction_work_estimate([widest, widest].into_iter()), None);
}

/// A face whose direction choices the work counter cannot hold exhausts the
/// search rather than being ordered behind every other face.
///
/// The refusal route: `direction_work_estimate` answers `None`, the scan calls
/// `budget.exhaust()` and drops the face, and the `budget.exhausted()` check
/// after the scan states `SearchOutcome::Exhausted`. The control face states
/// two unknown uses over the same shape and leaves the search open.
#[test]
fn a_face_the_work_counter_cannot_estimate_exhausts_the_search() {
    let outcome_for = |unknown_use_count: usize| {
        let assignments = vec![vec![MeshFaceBoundaryAssignment {
            boundaries: vec![(0..unknown_use_count)
                .map(|edge| MeshBoundaryEdgeCandidate {
                    edge,
                    start: 0,
                    end: 0,
                    reversed: None,
                })
                .collect()],
        }]];
        let edge_candidates = vec![vec![[0usize, 0usize]]; unknown_use_count];
        let mut domains = Vec::with_capacity(unknown_use_count * 2);
        for candidates in &edge_candidates {
            let domain = Arc::new(candidates.iter().flatten().copied().collect::<HashSet<_>>());
            domains.push(domain.clone());
            domains.push(domain);
        }
        let mut search = MeshSelectionSearch {
            assignments: &assignments,
            possible_face_equations: vec![Vec::new()],
            possible_face_choices: vec![Vec::new()],
            face_work: vec![Some(1)],
            edge_candidates: &edge_candidates,
            edge_rows: &[],
            vertex_points: &[[0.0; 3], [1.0, 0.0, 0.0]],
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
        let quotient = MeshQuotient::new(domains);
        let budget = WorkBudget::new(10_000);
        let propagation_budget = WorkBudget::new(0);
        search.search_from_state(&quotient, true, &budget, &propagation_budget);
        matches!(search.outcome, SearchOutcome::Exhausted)
    };

    assert!(outcome_for(usize::BITS as usize));
    assert!(!outcome_for(2));
}
