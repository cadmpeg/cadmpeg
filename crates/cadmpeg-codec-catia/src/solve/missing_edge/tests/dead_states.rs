//! Dead-state insertion keeps the point key's lease through refusal.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn gap_dead_state_insertion_refuses_with_live_endpoint_constraints() {
    let mut bytes = crate::test_support::test_topology::standard_quad_topology_stream();
    let header = bytes.windows(3).position(|part| part == [0x01, 0x01, 0x04])
        .expect("synthetic edge table");
    let first_row = header + 3;
    bytes[first_row + 1] = 4;
    bytes.splice(first_row + 6..first_row + 6, 0x7ffe_u16.to_be_bytes());
    let candidates = [vec![[8, 9], [10, 11]], vec![[1, 2]], vec![[2, 3]], vec![[3, 0]]];
    let run = |ctx: &DecodeContext<'_>| {
        super::super::standard_mesh_missing_edge_assignments(
            ctx, &bytes, &[[0, 0]; 4], Some(&candidates), false,
        )
    };
    let assignments = crate::test_support::with_service_context(run)
        .expect("service budget").expect("unconstrained fallback preserves the gap");
    assert_eq!(assignments[0].len(), 1);
    assert_eq!(assignments[0][0][0].edge, 0);
    assert_eq!(assignments[0][0][0].segment_count, 2);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::MAX;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("synthetic input admitted");
    let probe = RefusalProbe::arm(ResourceDimension::CollectionItems, "catia_gap_dead_states", Some(1));
    let CodecError::ResourceLimit(limit) = run(&ctx)
        .expect_err("the contradictory endpoint branch inserts a dead state")
    else {
        panic!("resource refusal");
    };
    drop(probe);
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "catia_gap_dead_states");
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(later)) if later == limit));
}
