// SPDX-License-Identifier: Apache-2.0
//! Collection-limit tests for mesh selection and edge-class constraints.

use crate::solve::mesh_quotient::{
    edge_class_search_constraint, possible_face_choices, possible_face_equations,
    MeshSelectionSearch, SearchOutcome,
};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use std::cell::RefCell;
use std::collections::HashSet;

#[test]
fn edge_class_constraint_refuses_normalized_row_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    catia_test_context!(service_ctx);
    let choices = [vec![[0usize, 1usize]]];
    assert!(edge_class_search_constraint(&service_ctx, &[0], &choices)
        .expect("service decode")
        .is_some());

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let Err(error) = edge_class_search_constraint(&ctx, &[0], &choices) else {
        panic!("the normalized row exceeds the collection limit");
    };
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_edge_class_normalized_rows"));

    let mut operations = HashSet::new();
    for limit in 0..=16 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match edge_class_search_constraint(&ctx, &[0], &choices) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(Some(_)) => break,
            Ok(None) => panic!("single edge class must remain viable"),
            Err(error) => panic!("unexpected edge-class refusal: {error}"),
        }
    }
    assert!(operations.contains("catia_edge_class_active"));
}

#[test]
fn mesh_selection_orientation_refuses_constraint_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    catia_test_context!(service_ctx);
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: None,
        }]],
    }]];
    let equations =
        possible_face_equations(&service_ctx, &assignments).expect("service resource budget");
    let face_choices = possible_face_choices(&service_ctx, &assignments, &equations);
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

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    let error = run(&ctx).expect_err("the selected boundary exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_selection_constraint_nodes"));

    let mut operations = HashSet::new();
    for limit in 0..=32 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        match run(&ctx) {
            Err(CodecError::ResourceLimit(error)) => {
                assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                operations.insert(error.operation);
            }
            Ok(true) => break,
            Ok(false) => panic!("single boundary remains orientable"),
            Err(error) => panic!("unexpected orientation refusal: {error}"),
        }
    }
    assert!(operations.contains("catia_selection_flips"));
}

#[test]
fn mesh_selection_completion_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    catia_test_context!(service_ctx);
    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
        }]],
    }]];
    let equations =
        possible_face_equations(&service_ctx, &assignments).expect("service resource budget");
    let mut search = MeshSelectionSearch {
        ctx: &service_ctx,
        assignments: &assignments,
        possible_face_equations: equations.clone(),
        possible_face_choices: possible_face_choices(&service_ctx, &assignments, &equations),
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
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
    search.ctx = &ctx;
    let error = search
        .fixed_remaining_faces_are_orientable()
        .expect_err("completion exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_selection_completion"));
}
