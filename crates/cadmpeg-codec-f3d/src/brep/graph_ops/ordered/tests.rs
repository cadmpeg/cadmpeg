// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::AdjacencyRow;

#[test]
fn brep_ordered_graph_callbacks_admit_actual_bytes_and_keep_refusals() {
    for (stored, query, found) in [
        ("alpha", "z-long-unread-tail", false),
        ("alpha", "al", false),
        ("alpha", "alpha", true),
        ("é", "ê", false),
        ("", "", true),
        ("", "unread", false),
    ] {
        let refusal = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            "compare F3D BREP graph ID",
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                super::super::contains(&ctx, &[stored.to_owned()], query, "find graph fixture ID")
            },
        );
        assert!(matches!(refusal, CodecError::ResourceLimit(limit) if limit.additional == 1));
        let mut allowance = 0;
        loop {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = allowance;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let values = vec![stored.to_owned()];
            let result = super::super::contains(&ctx, &values, query, "find graph fixture ID");
            if let Err(CodecError::ResourceLimit(original)) = result {
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.operation, "compare F3D BREP graph ID");
                assert_eq!(original.additional, 1);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                );
                allowance = original.used.checked_add(original.additional).unwrap();
            } else {
                assert_eq!(result.unwrap(), found);
                ctx.finish_session().unwrap();
                break;
            }
        }
    }
}

#[test]
fn brep_graph_searches_keep_original_fused_capsule() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx
        .charge_work_limit(1, "original graph refusal")
        .unwrap_err();
    assert!(
        matches!(super::super::contains(&ctx, &[], "absent", "empty graph lookup"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(super::super::insert_id(&ctx, &mut Vec::new(), "candidate", "copy graph candidate", "empty graph insert"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(super::super::insert_brep_adjacency(&ctx, &mut Vec::new(), "source", "target"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn brep_duplicate_adjacency_needs_no_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut adjacency = vec![AdjacencyRow {
        source: "source".to_owned(),
        targets: vec!["target".to_owned()],
    }];
    super::super::insert_brep_adjacency(&ctx, &mut adjacency, "source", "target").unwrap();
    assert_eq!(adjacency.len(), 1);
    assert_eq!(adjacency.first().unwrap().targets.len(), 1);
    ctx.finish_session().unwrap();
}
