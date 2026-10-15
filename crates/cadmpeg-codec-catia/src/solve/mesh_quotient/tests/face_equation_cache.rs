use crate::solve::mesh_quotient::possible_face_choices_with_limit;
use crate::solve::mesh_quotient::{
    possible_face_choices, possible_face_equations, MeshQuotient, MeshSelectionSearch,
    SearchOutcome, MAX_FACE_EQUATION_CACHE_ENTRIES, MAX_MESH_CONSTRAINT_OPERATIONS,
};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use cadmpeg_core::decode::WorkBudget;
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
        possible_face_equations: possible_face_equations(&assignments),
        possible_face_choices: possible_face_choices(
            &assignments,
            &possible_face_equations(&assignments),
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
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient
        .merge(4, 5)
        .expect("service merge")
        .expect("unrelated component merge");
    assert!(search
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
    quotient
        .merge(0, 4)
        .expect("service merge")
        .expect("component joined to a face port");
    assert!(search
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(search.face_equation_cache.borrow().len(), 2);
    {
        let mut cache = search.face_equation_cache.borrow_mut();
        for key in 1..=MAX_FACE_EQUATION_CACHE_ENTRIES {
            cache.insert((key, Vec::new()), Vec::new());
        }
    }
    quotient
        .merge(1, 2)
        .expect("service merge")
        .expect("new face-component merge");
    assert!(search
        .propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
        .expect("propagate the forced face equations"));
    assert_eq!(search.face_equation_cache.borrow().len(), 1);
}

#[test]
fn forced_face_equations_propagate_for_a_two_edge_loop() {
    use cadmpeg_core::decode::DecodeContext;

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
    let run = |ctx: &DecodeContext<'_>| {
        let equations = possible_face_equations(&assignments);
        let mut choices = Vec::new();
        if !possible_face_choices_with_limit(&assignments, &equations, usize::MAX, &mut choices) {
            return Ok(false);
        }
        let search = MeshSelectionSearch {
            ctx,
            assignments: &assignments,
            possible_face_choices: choices,
            possible_face_equations: equations,
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
        search.propagate_forced_face_equations_from(
            &mut quotient,
            None,
            &WorkBudget::new(MAX_MESH_CONSTRAINT_OPERATIONS),
        )
    };
    crate::test_support::with_service_context(|ctx| {
        assert!(run(ctx).expect("propagate the forced face equations"));
    });
}
