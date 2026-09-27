use crate::solve::mesh_quotient::{
    possible_face_choices, possible_face_choices_with_limit, possible_face_equations, MeshQuotient, MeshSelectionSearch,
    SearchOutcome, MAX_FACE_EQUATION_CACHE_ENTRIES,
};
use crate::solve::missing_edge::{MeshBoundaryEdgeCandidate, MeshFaceBoundaryAssignment};
use std::cell::RefCell;
use std::collections::HashSet;
use std::sync::Arc;

#[test]
fn face_equation_projection_and_cache_refuse_before_growth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use cadmpeg_core::CodecError;
    use std::collections::BTreeSet;

    let assignments = vec![vec![MeshFaceBoundaryAssignment {
        boundaries: vec![vec![
            MeshBoundaryEdgeCandidate { edge: 0, start: 0, end: 1, reversed: None },
            MeshBoundaryEdgeCandidate { edge: 1, start: 1, end: 0, reversed: None },
        ]],
    }]];
    let candidates = vec![vec![]; 3];
    let run = |ctx: &DecodeContext<'_>| {
        let equations = possible_face_equations(ctx, &assignments)?;
        let mut choices = Vec::new();
        if !possible_face_choices_with_limit(ctx, &assignments, &equations, usize::MAX, &mut choices)? {
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
        let mut quotient = MeshQuotient::new(
            (0..6).map(|_| Arc::new(HashSet::from([0, 1, 2]))).collect(),
        );
        search.propagate_forced_face_equations(&mut quotient)
    };
    crate::test_support::with_service_context(|ctx| assert!(run(ctx).expect("service budget")));
    let mut refusals = BTreeSet::new();
    let mut completed = false;
    for cap in 0..=1_024 {
        match crate::test_support::with_collection_limit(cap, run) {
            Err(CodecError::ResourceLimit(limit)) => {
                refusals.insert(limit.operation);
            }
            Ok(true) => {
                completed = true;
                break;
            }
            _ => panic!("unexpected face projection result"),
        }
    }
    assert!(completed, "fixture must fit the final collection cap");
    for operation in [
        "catia_forced_equation_queue",
        "catia_forced_equation_queued",
        "catia_face_projection_roots",
        "catia_face_projection_root_order",
        "catia_face_projection_signature_rows",
        "catia_face_projection_domain_points",
        "catia_face_projection_member_nodes",
        "catia_forced_equation_cache",
    ] {
        assert!(refusals.contains(operation), "no refusal at {operation}");
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits input budget");
    assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(limit))
        if limit.operation == "catia_forced_equation_cache_key"));
}

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
            &ctx,
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
