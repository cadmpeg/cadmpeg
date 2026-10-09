//! Rejected incidence completions retain actual dead-state keys.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn incidence_dead_state_insertion_refuses_after_rejected_completion() {
    let run = |ctx: &DecodeContext<'_>| {
        let budget = ctx.work_budget(10_000);
        let reject = |_: &[(usize, [usize; 2])]| Ok(false);
        let mut search = super::super::IncidenceComponentSearch {
            ctx,
            choices: &[vec![[0, 1]]],
            explicit_point_supports: Vec::new(),
            point_support_edges: Vec::new(),
            degree_support_witnesses: std::cell::RefCell::new(std::collections::HashMap::new()),
            edge_faces: &[[0, 0]],
            face_edges: &[vec![0]],
            mesh_assignments: None,
            face_configuration_domains: None,
            coordinate_domains: None,
            active: vec![true],
            edges: &[0],
            constraints: Vec::new(),
            assignment: vec![Some([0, 1])],
            degrees: Vec::new(),
            solutions: Vec::new(),
            solution_filter: Some(&reject),
            solution_visitor: None,
            partial_solution_filter: None,
            dead_states: std::collections::HashMap::new(),
            budget: &budget,
            degree_support_budget: &budget,
            coordinate_propagation_budget: &budget,
            boundary_propagation_budget: &budget,
            state: super::super::IncidenceSearchState::Open,
            search_storage: std::cell::RefCell::new(
                ctx.reserve_scoped(0, "incidence memo test")?,
            ),
        };
        search.search_with_quotient(&[], None, &[])?;
        assert!(search.solutions.is_empty());
        assert_eq!(search.dead_states.len(), 1);
        assert!(search.dead_states.contains_key(&vec![Some([0, 1])]));
        search.search_with_quotient(&[], None, &[])?;
        assert_eq!(search.dead_states.len(), 1);
        Ok::<_, CodecError>(())
    };
    crate::test_support::with_service_context(run).expect("one retained rejected state");

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let probe = RefusalProbe::arm(ResourceDimension::CollectionItems,
        "catia incidence dead states", Some(1));
    let CodecError::ResourceLimit(limit) = run(&ctx).expect_err("dead-state entry admission")
    else {
        panic!("resource refusal");
    };
    drop(probe);
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "catia incidence dead states");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.charge_work(0, "after rejected memo insertion"),
        Err(CodecError::ResourceLimit(later)) if later == limit));
}
