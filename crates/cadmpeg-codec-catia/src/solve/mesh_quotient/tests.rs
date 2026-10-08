// SPDX-License-Identifier: Apache-2.0
//! Work-estimate tests for the mesh quotient search.

use super::selection_search::direction_work_estimate;
use super::{MeshQuotient, MeshSelectionSearch, SearchOutcome};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::WorkBudget;
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn direction_work_estimate_states_no_figure_the_work_counter_cannot_hold() {
    let estimate = |unknown_uses: &[usize]| {
        crate::test_support::with_service_context(|ctx| direction_work_estimate(ctx, unknown_uses))
            .expect("service resource budget")
    };
    assert_eq!(estimate(&[0usize, 1, 2]), Some(7));
    let widest = cadmpeg_core::decode::index_from_u32(usize::BITS) - 1;
    assert_eq!(estimate(&[widest]), Some(1usize << widest));
    for unknown in [
        cadmpeg_core::decode::index_from_u32(usize::BITS),
        usize::MAX,
    ] {
        assert_eq!(estimate(&[unknown]), None);
    }
    assert_eq!(estimate(&[widest, widest]), None);
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
    catia_test_context!(ctx);
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
            ctx: &ctx,
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
            visited_states: std::collections::HashMap::new(),
            memo_storage: RefCell::new(
                (&ctx)
                    .reserve_scoped(0, "catia_selection_memo_storage")
                    .expect("memo storage"),
            ),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        let quotient = MeshQuotient::new(domains);
        let budget = WorkBudget::new(10_000);
        let propagation_budget = WorkBudget::new(0);
        search
            .search_from_state(&quotient, true, &budget, &propagation_budget)
            .expect("service resource budget");
        matches!(search.outcome, SearchOutcome::Exhausted)
    };

    assert!(outcome_for(cadmpeg_core::decode::index_from_u32(
        usize::BITS
    )));
    assert!(!outcome_for(2));
}

mod face_equation_cache;
mod orientation_limits;
mod quotient_search;
mod selection_limits;

#[test]
fn quotient_change_detection_ignores_member_insertion_order() {
    catia_test_context!(ctx);
    let mut left = MeshQuotient::new((0..4).map(|_| Arc::new(HashSet::from([0, 1]))).collect());
    let mut right = left.clone();
    for node in [1, 2] {
        left.merge_charged(&ctx, 0, node)
            .expect("merge budget")
            .expect("shared domain");
    }
    for node in [2, 1] {
        right
            .merge_charged(&ctx, 0, node)
            .expect("merge budget")
            .expect("shared domain");
    }
    assert_ne!(left.members(0), right.members(0));
    assert!(super::changed_quotient_edges(&ctx, &left, &right)
        .expect("comparison budget")
        .is_empty());
    right.domains[0] = super::unscoped_point_domain([1]);
    assert_eq!(
        super::changed_quotient_edges(&ctx, &left, &right).expect("comparison budget"),
        HashSet::from([0, 1])
    );
}

#[test]
fn quotient_change_detection_visits_a_shared_class_linearly() {
    const NODE_COUNT: usize = 512;
    // Two root walks, two index visits and flag initialization fit this ceiling.
    const WORK_PER_NODE: u64 = 128;
    let mut left = MeshQuotient::new(
        (0..NODE_COUNT)
            .map(|_| Arc::new(HashSet::from([0])))
            .collect(),
    );
    crate::test_support::with_service_context(|ctx| {
        for node in 1..NODE_COUNT {
            left.merge_charged(ctx, 0, node)
                .expect("merge budget")
                .expect("shared domain");
        }
    });
    let right = left.clone();
    let changed = crate::test_support::with_work_limit(
        WORK_PER_NODE * cadmpeg_core::decode::u64_from_index(NODE_COUNT),
        |ctx| super::changed_quotient_edges(ctx, &left, &right),
    )
    .expect("linear comparison budget");
    assert!(changed.is_empty());
}

#[test]
fn quotient_merge_releases_child_members_and_unused_domain() {
    catia_test_context!(ctx);
    let mut quotient = MeshQuotient::new_charged(&ctx, 2, |_| {
        crate::solve::mesh_quotient::point_domain(&ctx, [0], "test singleton domain")
    })
    .expect("quotient storage");
    let child_domain = std::rc::Rc::downgrade(&quotient.domains[1]);
    let root = quotient
        .merge_charged(&ctx, 0, 1)
        .expect("merge budget")
        .expect("joined class");
    let child = usize::from(root == 0);
    assert_eq!(quotient.members(root).len(), 2);
    assert!(quotient.members[child].is_empty());
    assert!(quotient.members[child].storage.is_none());
    assert!(quotient.domains[child].is_empty());
    assert!(child_domain.upgrade().is_none());
}
