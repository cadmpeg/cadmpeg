//! Admission and lifetime regressions for incidence preparation.

use super::*;
use crate::solve::incidence::{
    advance_compact_boundary_domains, prune_incidence_choices, CompactBoundaryAdvanceOutcome,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn incidence_unused_selection_and_point_tables_do_not_allocate() {
    let mut choices = vec![Vec::new(); 128];
    crate::test_support::with_collection_limit(0, |ctx| {
        assert!(prune_ordered_face_endpoint_support(
            ctx,
            &[MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0])],
            &mut choices,
            &WorkBudget::new(100),
        )
        .expect("unordered faces do not use selection"));
    });
    crate::test_support::with_collection_limit(4, |ctx| {
        let outcome = advance_compact_boundary_domains(
            ctx,
            [&MeshFaceBoundaryDomain::UnorderedFullCycle(vec![0])],
            &choices,
            &[None; 128],
            None,
            Vec::new(),
            &WorkBudget::new(100),
        )
        .expect("partial domain only collects its one edge");
        assert!(
            matches!(outcome, CompactBoundaryAdvanceOutcome::Complete(states) if states.is_empty())
        );
    });
}

#[test]
fn isolated_face_configurations_need_no_masks_or_local_work() {
    let input = vec![
        vec![vec![(0, [0, 0])], vec![(0, [1, 1])]],
        vec![vec![(1, [2, 2])], vec![(1, [3, 3])]],
    ];
    crate::test_support::with_retained_limit(0, |ctx| {
        let budget = ctx.work_budget(0);
        let mut domains = input.clone();
        assert!(
            prune_face_configuration_singleton_support(ctx, &mut domains, &budget)
                .expect("isolated domains never build queried masks")
        );
        assert_eq!(domains, input);
        assert!(!budget.exhausted());
    });
}

#[test]
fn incidence_support_index_forces_disjoint_cycles_on_one_face() {
    let mut choices = Vec::new();
    for base in (0..128).step_by(4) {
        choices.extend([
            vec![[base, base + 1]],
            vec![[base + 2, base + 3]],
            vec![[base, base + 2], [base + 2, base + 3]],
            vec![[base + 1, base + 3], [base + 2, base + 3]],
        ]);
    }
    let faces = vec![[0, 0]; choices.len()];
    crate::test_support::with_service_context(|ctx| {
        assert!(prune_incidence_choices(ctx, &mut choices, &faces, 1, 128)
            .expect("indexed point supports")
            .is_some());
    });
    for (group, rows) in choices.chunks(4).enumerate() {
        let base = group * 4;
        assert_eq!(
            rows,
            [
                vec![[base, base + 1]],
                vec![[base + 2, base + 3]],
                vec![[base, base + 2]],
                vec![[base + 1, base + 3]]
            ]
        );
    }
}

#[test]
fn incidence_search_refuses_nested_frames() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let budget = ctx.work_budget(10_000);
    let choices = [vec![[0, 0], [1, 1]], vec![[2, 2], [3, 3]]];
    let mut search = crate::solve::incidence::IncidenceComponentSearch {
        ctx: &ctx,
        choices: &choices,
        explicit_point_supports: Vec::new(),
        point_support_edges: Vec::new(),
        degree_support_witnesses: RefCell::new(HashMap::new()),
        edge_faces: &[[0, 0], [1, 1]],
        face_edges: &[vec![0], vec![1]],
        mesh_assignments: None,
        face_configuration_domains: None,
        coordinate_domains: None,
        active: vec![true; 2],
        edges: &[0, 1],
        constraints: Vec::new(),
        assignment: vec![None; 2],
        degrees: vec![BTreeMap::new(); 2],
        solutions: Vec::new(),
        solution_filter: None,
        solution_visitor: None,
        partial_solution_filter: None,
        dead_states: HashMap::new(),
        budget: &budget,
        degree_support_budget: &budget,
        coordinate_propagation_budget: &budget,
        boundary_propagation_budget: &budget,
        state: IncidenceSearchState::Open,
        search_storage: RefCell::new(
            ctx.reserve_scoped(0, "test incidence storage")
                .expect("storage"),
        ),
    };
    let CodecError::ResourceLimit(limit) = search.search().expect_err("recursive branch refuses")
    else {
        panic!("depth refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.used, 1);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
