// SPDX-License-Identifier: Apache-2.0
//! Collection-limit tests for mesh selection and edge-class constraints.

use crate::solve::missing_edge::MeshFaceBoundaryAssignment;

use crate::solve::mesh_quotient::edge_class_search_constraint;

use crate::solve::missing_edge::MeshBoundaryEdgeCandidate;

use crate::solve::mesh_quotient::MeshSelectionSearch;

use crate::solve::mesh_quotient::SearchOutcome;

use crate::solve::mesh_quotient::possible_face_equations;

use crate::solve::mesh_quotient::possible_face_choices;

#[test]
fn single_edge_class_choice_yields_a_search_constraint() {
    catia_test_context!(service_ctx);
    let choices = [vec![[0usize, 1usize]]];
    assert!(edge_class_search_constraint(&service_ctx, &[0], &choices)
        .expect("service decode")
        .is_some());
}

#[test]
fn selected_face_with_open_edge_direction_is_orientable() {
    use cadmpeg_core::decode::DecodeContext;

    use std::cell::RefCell;
    use std::collections::HashSet;
    catia_test_context!(service_ctx);
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    }]];
    let equations = possible_face_equations(&assignments);
    let face_choices = possible_face_choices(&assignments, &equations);
    let run = |ctx: &DecodeContext<'_>| {
        let search = MeshSelectionSearch {
            ctx,
            assignments: &assignments,
            possible_face_equations: equations.clone(),
            possible_face_choices: face_choices.clone(),
            face_work: vec![Some(1)],
            edge_candidates: &[],
            edge_rows: &[],
            vertex_points: &[],
            candidate_gauge: None,
            port_identities: None,
            fixed_face_directions: Vec::new(),
            fixed_edge_orientations: Vec::new(),
            edge_has_fixed_direction: Vec::new(),
            selected: vec![Some((0, vec![vec![false]]))],
            visited_states: HashSet::new(),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        search.selected_orientable()
    };
    assert!(run(&service_ctx).expect("service decode"));
}

#[test]
fn remaining_face_with_fixed_edge_direction_is_orientable() {
    use cadmpeg_core::decode::DecodeArena;
    use cadmpeg_core::decode::DecodeContext;
    use cadmpeg_core::decode::DecodePolicy;
    use std::cell::RefCell;
    use std::collections::HashSet;
    catia_test_context!(service_ctx);
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    }]];
    let equations = possible_face_equations(&assignments);
    let mut search = MeshSelectionSearch {
        ctx: &service_ctx,
        assignments: &assignments,
        possible_face_equations: equations.clone(),
        possible_face_choices: possible_face_choices(&assignments, &equations),
        face_work: vec![Some(1)],
        edge_candidates: &[],
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
    assert!(search
        .fixed_remaining_faces_are_orientable()
        .expect("service decode"));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    search.ctx = &ctx;
}

#[test]
fn selected_face_reports_its_boundary_edge() {
    use std::cell::RefCell;
    use std::collections::HashSet;
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 7,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    }]];
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
            fixed_edge_orientations: Vec::new(),
            edge_has_fixed_direction: Vec::new(),
            selected: vec![Some((0, vec![vec![false]]))],
            visited_states: HashSet::new(),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        search.selected_edges()
    };
    crate::test_support::with_service_context(|ctx| {
        assert_eq!(run(ctx), HashSet::from([7]));
    });
}
