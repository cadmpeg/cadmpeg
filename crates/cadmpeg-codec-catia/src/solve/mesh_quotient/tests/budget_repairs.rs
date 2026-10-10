//! Regressions for quotient work and recursion admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn fixed_assignment_search_admits_recursive_frames() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let domains = vec![
        vec![super::super::MeshFaceBoundaryAssignment {
            boundaries: Vec::new()
        }];
        3
    ];
    let budget = ctx.work_budget(1_000);
    let Err(CodecError::ResourceLimit(limit)) =
        super::super::resolve_fixed_mesh_endpoint_assignment_domains(
            &ctx,
            super::super::MeshEndpointGeometry {
                edge_rows: &[],
                vertex_points: &[],
            },
            &[],
            &domains,
            &[],
            &budget,
            None,
        )
    else {
        panic!("recursive selection must refuse before materialization")
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.operation, "catia_fixed_assignment_search");
    assert_eq!(limit.used, 2);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn equivalent_edge_classes_store_adjacent_predecessors() {
    const EDGES: usize = 128;
    let choices = vec![vec![[0, 1], [1, 2]]; EDGES];
    crate::test_support::with_service_context(|ctx| {
        let constraint = super::super::edge_class_search_constraint(ctx, &vec![0; EDGES], &choices)
            .expect("class construction")
            .expect("class constraint");
        assert_eq!(
            constraint.ordered,
            (1..EDGES).map(|edge| (edge - 1, edge)).collect::<Vec<_>>()
        );
        assert!(constraint.active.iter().all(|active| *active));
    });
}

#[test]
fn affected_nodes_walk_each_merged_class_once() {
    use std::{collections::HashSet, sync::Arc};
    // One class needs linear collection growth; 128 repeated rows need 16,384 items.
    crate::test_support::with_collection_limit(32 * 128, |ctx| {
        let mut quotient = super::super::MeshQuotient::new(vec![Arc::new(HashSet::from([0])); 128]);
        for node in 1..128 {
            quotient
                .merge_charged(ctx, node, node - 1)
                .expect("merge")
                .expect("compatible domains");
        }
        let candidates = vec![vec![[0, 0]]; 64];
        let edges = quotient
            .affected_edges_for_nodes(ctx, &(0..128).collect::<Vec<_>>(), &candidates)
            .expect("one component walk fits");
        assert_eq!(edges, (0..64).collect::<Vec<_>>());
        assert_eq!(quotient.root(ctx, 0).expect("representative"), 127);
    });
}

#[test]
fn component_edge_propagation_skips_global_queue_buffers() {
    use std::{collections::HashSet, sync::Arc};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 100;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut quotient = super::super::MeshQuotient::new(
        (0..1024)
            .map(|point| Arc::new(HashSet::from([point])))
            .collect(),
    );
    let candidates = (0..512)
        .map(|edge| vec![[edge * 2, edge * 2 + 1]])
        .collect::<Vec<_>>();
    assert!(quotient
        .propagate_edge_domains(&ctx, &[0], &candidates, None)
        .expect("local queue fits"));
    assert_eq!(quotient.domains[0].as_slice(), [0]);
    assert_eq!(quotient.domains[1].as_slice(), [1]);
    assert_eq!(quotient.domains[1023].as_slice(), [1023]);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn selection_search_routes_admit_active_depth() {
    use super::super::{MeshQuotient, MeshSelectionSearch, SearchOutcome};
    use std::cell::RefCell;
    for (route, operation) in [
        (0, "catia_fixed_direction_selection"),
        (1, "catia_face_selection"),
        (2, "catia_selection_state"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let _ancestor = ctx.enter_nested("test ancestor").expect("one frame fits");
        let mut search = MeshSelectionSearch {
            ctx: &ctx,
            assignments: &[],
            possible_face_choices: Vec::new(),
            face_work: Vec::new(),
            edge_candidates: &[],
            edge_rows: &[],
            vertex_points: &[],
            candidate_gauge: None,
            port_identities: None,
            fixed_face_directions: Vec::new(),
            fixed_edge_orientations: Vec::new(),
            edge_has_fixed_direction: Vec::new(),
            selected: Vec::new(),
            visited_states: std::collections::HashMap::new(),
            memo_storage: RefCell::new(ctx.reserve_scoped(0, "selection memo").expect("scope")),
            outcome: SearchOutcome::Open,
            face_equation_cache: RefCell::default(),
        };
        let quotient = MeshQuotient::new(Vec::new());
        let budget = ctx.work_budget(1_000);
        let result = match route {
            0 => search.search_fixed_direction_with_budget(&quotient, &budget),
            1 => search.search_from_state(&quotient, true, &budget, &budget),
            _ => search.search_state(&quotient, true, &budget, &budget),
        };
        let Err(CodecError::ResourceLimit(limit)) = result else {
            panic!("route must admit its frame")
        };
        assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.used, 1);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }
}

#[test]
fn descending_merges_keep_the_larger_member_buffer() {
    use std::{collections::HashSet, sync::Arc};
    // Moving each growing child row costs 8 * 128 * 127 / 2 = 65,024 units
    // before destination growth. Moving only the smaller row fits this cap.
    crate::test_support::with_work_limit(32_000, |ctx| {
        let mut quotient = super::super::MeshQuotient::new(vec![Arc::new(HashSet::from([0])); 128]);
        for node in 1..128 {
            quotient
                .merge_charged(ctx, node, node - 1)
                .expect("linear member movement")
                .expect("compatible domains");
        }
        assert_eq!(quotient.root(ctx, 0).expect("representative"), 127);
        let mut members = quotient.members(127).to_vec();
        members.sort_unstable();
        assert_eq!(members, (0..128).collect::<Vec<_>>());
        assert_eq!(quotient.domains[127].as_slice(), [0]);
    });
}

#[test]
fn fixed_direction_options_skip_unvisited_local_work() {
    use std::{collections::HashSet, sync::Arc};
    crate::test_support::with_service_context(|ctx| {
        let quotient = super::super::MeshQuotient::new(vec![Arc::new(HashSet::from([0])); 2]);
        let use_ = super::super::MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        };
        let mut boundaries = vec![Vec::new()];
        boundaries.extend(vec![vec![use_; 64]; 16]);
        let directions = boundaries
            .iter()
            .map(|boundary| vec![false; boundary.len()])
            .collect();
        let assignment = super::super::MeshFaceBoundaryAssignment { boundaries };
        let budget = super::super::WorkBudget::new(0);
        let options = quotient
            .assignment_options_for_directions(
                ctx,
                &assignment,
                &vec![directions],
                1,
                Some(&budget),
            )
            .expect("empty first boundary rejects without corner work");
        assert!(options.is_empty());
        assert_eq!(budget.consumed(), 0);
        assert!(!budget.exhausted());
    });
}

#[test]
fn label_direction_scan_debits_only_the_visited_prefix() {
    use std::{collections::HashSet, sync::Arc};
    crate::test_support::with_service_context(|ctx| {
        let mut quotient = super::super::MeshQuotient::new(vec![Arc::new(HashSet::from([0])); 2]);
        let use_ = super::super::MeshBoundaryEdgeCandidate {
            edge: 0,
            start: 0,
            end: 0,
            reversed: None,
        };
        let assignment = super::super::MeshFaceBoundaryAssignment {
            boundaries: vec![vec![use_; 1024]],
        };
        let budget = super::super::WorkBudget::new(1);
        assert!(quotient
            .merge_label_directions_in_place(
                ctx,
                &assignment,
                &[vec![false; 1024]],
                &[None],
                Some(&budget),
            )
            .expect("missing first orientation rejects")
            .is_none());
        assert_eq!(budget.consumed(), 1);
        assert!(!budget.exhausted());
    });
}
