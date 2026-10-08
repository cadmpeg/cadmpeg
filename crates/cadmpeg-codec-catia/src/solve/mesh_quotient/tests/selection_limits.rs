// SPDX-License-Identifier: Apache-2.0
//! Collection-limit tests for mesh selection and edge-class constraints.

use crate::solve::mesh_quotient::{
    edge_class_search_constraint, possible_face_choices, possible_face_equations,
    MeshSelectionSearch, SearchOutcome,
};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};

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
            visited_states: std::collections::HashMap::new(),
            memo_storage: RefCell::new(
                (ctx)
                    .reserve_scoped(0, "catia_selection_memo_storage")
                    .expect("memo storage"),
            ),
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits the input limit");
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
        visited_states: std::collections::HashMap::new(),
        memo_storage: RefCell::new(
            (&service_ctx)
                .reserve_scoped(0, "catia_selection_memo_storage")
                .expect("memo storage"),
        ),
        outcome: SearchOutcome::Open,
        face_equation_cache: RefCell::default(),
    };
    assert!(search
        .fixed_remaining_faces_are_orientable()
        .expect("service decode"));

    search.ctx = &ctx;
    let error = search
        .fixed_remaining_faces_are_orientable()
        .expect_err("completion exceeds the collection limit");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "catia_selection_completion"));
}

#[test]
fn mesh_selection_completion_refuses_existing_nested_direction_copies() {
    use cadmpeg_core::CodecError;

    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 1,
            reversed: Some(false),
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
            visited_states: std::collections::HashMap::new(),
            memo_storage: RefCell::new(
                (ctx)
                    .reserve_scoped(0, "catia_selection_memo_storage")
                    .expect("memo storage"),
            ),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        search.fixed_remaining_faces_are_orientable()
    };
    crate::test_support::with_service_context(|ctx| assert!(run(ctx).expect("service budget")));
    let mut refusals = HashSet::new();
    for cap in 0..=32 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refusals.insert(limit.operation);
            }
            Ok(true) => break,
            _ => panic!("unexpected selected completion result"),
        }
    }
    assert!(refusals.contains("catia_direction_copy_rows"));
    assert!(refusals.contains("catia_direction_copy_values"));
}

#[test]
fn mesh_selection_selected_edges_refuse_before_set_growth() {
    use cadmpeg_core::CodecError;

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
            visited_states: std::collections::HashMap::new(),
            memo_storage: RefCell::new(
                (ctx)
                    .reserve_scoped(0, "catia_selection_memo_storage")
                    .expect("memo storage"),
            ),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        search.selected_edges()
    };
    crate::test_support::with_service_context(|ctx| {
        assert_eq!(run(ctx).expect("service budget"), BTreeSet::from([7]));
    });
    assert!(matches!(crate::test_support::with_collection_limit(0, run),
        Err(CodecError::ResourceLimit(limit)) if limit.operation == "catia_selection_selected_edges"));
}

#[test]
fn singleton_duplicate_assignments_release_candidate_storage() {
    use crate::families::standard::topology::{EdgeBoundaryLayout, EdgeRow};
    use crate::solve::mesh_quotient::{
        resolve_singleton_mesh_endpoint_candidates, MeshSolve,
        ResolveSingletonMeshEndpointCandidatesInputs,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 16 * 1024;
    policy.limits.max_materialized_bytes = 16 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let edges = (0..3)
        .map(|_| {
            EdgeRow::new(1, vec![0, 0], EdgeBoundaryLayout::CompleteBoundaryRun).expect("edge row")
        })
        .collect::<Vec<_>>();
    let vertices = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    let candidates = [vec![[0, 1]], vec![[1, 2]], vec![[2, 0]]];
    let assignment = MeshFaceBoundaryAssignment {
        boundaries: vec![(0..3)
            .map(|edge| MeshBoundaryEdgeCandidate {
                edge,
                start: 0,
                end: 1,
                reversed: None,
            })
            .collect()],
    };
    let assignments = [vec![assignment; 1000]];
    let budget = WorkBudget::new(usize::MAX);
    let MeshSolve::Solved((topology, points)) = resolve_singleton_mesh_endpoint_candidates(
        &ctx,
        ResolveSingletonMeshEndpointCandidatesInputs {
            edge_rows: &edges,
            vertex_points: &vertices,
            edge_candidates: &candidates,
            assignments: &assignments,
            port_identities: &[[0, 1], [2, 3], [4, 5]],
            edge_direction_evidence: None,
            budget: &budget,
            candidate_gauge: None,
        },
    )
    .expect("duplicate scratch is released")
    .expect("singleton applies") else {
        panic!("duplicate assignments must solve");
    };
    assert_eq!(topology.faces.len(), 1);
    assert_eq!(topology.edge_rows.len(), 3);
    assert_eq!(points, vec![0, 1, 2]);
    let _released = ctx
        .reserve_scoped(16 * 1024, "released singleton scratch")
        .expect("all temporary bytes released");
}

#[test]
fn endpoint_relation_constraints_retain_arcs_without_join_indexes() {
    use crate::solve::mesh_quotient::{
        build_endpoint_relation_constraints, MeshEndpointRelationChoice,
        MeshEndpointRelationSelection,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, WorkBudget};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4096;
    policy.limits.max_materialized_bytes = 128 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    let choice = || MeshEndpointRelationChoice {
        id: 0,
        selection: MeshEndpointRelationSelection::Enumerated {
            assignments: vec![0],
            edge_pairs: (0..200).map(|edge| (edge, [0, 1])).collect(),
        },
    };
    let domains = [vec![choice()], vec![choice()]];
    let budget = WorkBudget::new(usize::MAX);
    let constraints = build_endpoint_relation_constraints(&ctx, &domains, &budget)
        .expect("only arcs retained")
        .expect("compatible keys");
    assert_eq!(constraints.choice_counts, vec![1, 1]);
    assert_eq!(constraints.incoming, vec![vec![(1, 0)], vec![(0, 0)]]);
    assert_eq!(constraints.arcs.len(), 2);
    for (face, arcs) in constraints.arcs.iter().enumerate() {
        assert_eq!(arcs.len(), 1);
        assert_eq!(arcs[0].neighbor, 1 - face);
        assert_eq!(arcs[0].supports, vec![vec![1]]);
    }
    let _released = ctx
        .reserve_scoped(128 * 1024, "released relation join scratch")
        .expect("all temporary bytes released");
}
