use crate::solve::mesh_quotient::{
    possible_face_choices, possible_face_equations, MeshQuotient, MeshSelectionSearch,
    SearchOutcome, MAX_FACE_EQUATION_CACHE_ENTRIES,
};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn face_equation_cache_ignores_unrelated_quotient_components() {
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
    let candidates = vec![vec![]; 3];
    let search = MeshSelectionSearch {
        ctx: &ctx,
        assignments: &assignments,
        possible_face_equations: possible_face_equations(&ctx, &assignments)
            .expect("service resource budget"),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&ctx, &assignments).expect("service resource budget"),
        ),
        face_work: vec![Some(1)],
        edge_candidates: &candidates,
        edge_rows: &[],
        vertex_points: &[],
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
    let mut quotient =
        MeshQuotient::new((0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect());

    assert!(search
        .propagate_forced_face_equations(&mut quotient)
        .expect("service resource budget"));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient.merge(4, 5).expect("unrelated component merge");
    assert!(search
        .propagate_forced_face_equations(&mut quotient)
        .expect("service resource budget"));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient
        .merge(0, 4)
        .expect("component joined to a face port");
    assert!(search
        .propagate_forced_face_equations(&mut quotient)
        .expect("service resource budget"));
    assert_eq!(search.face_equation_cache.borrow().len(), 2);
    {
        let mut cache = search.face_equation_cache.borrow_mut();
        for key in 1..=MAX_FACE_EQUATION_CACHE_ENTRIES {
            cache.insert((key, Vec::new()), Vec::new());
        }
    }
    quotient.merge(1, 2).expect("new face-component merge");
    assert!(search
        .propagate_forced_face_equations(&mut quotient)
        .expect("service resource budget"));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
}
