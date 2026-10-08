// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::AdjacencyRow;

#[test]
fn brep_ordered_graph_callbacks_admit_actual_bytes_and_keep_refusals() {
    for (stored, query, work, found) in [
        ("alpha", "z-long-unread-tail", 2, false),
        ("alpha", "al", 3, false),
        ("alpha", "alpha", 6, true),
        ("é", "ê", 3, false),
        ("", "", 1, true),
        ("", "unread", 1, false),
    ] {
        for allowance in 0..=work {
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
            if allowance < work {
                let CodecError::ResourceLimit(original) = result.unwrap_err() else {
                    panic!("comparison must refuse");
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert_eq!(original.operation, "compare F3D BREP graph ID");
                assert_eq!(original.additional, 1);
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
                );
            } else {
                assert_eq!(result.unwrap(), found);
                ctx.finish_session().unwrap();
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
        matches!(super::super::insert_id(&ctx, &mut Vec::new(), "candidate".to_owned(), "empty graph insert"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(super::super::insert_brep_adjacency(&ctx, &mut Vec::new(), "source", "target"), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn an_existing_dependency_pair_requires_no_new_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut adjacency = vec![AdjacencyRow {
        source: "source".into(),
        targets: vec!["target".into()],
    }];
    super::super::insert_brep_adjacency(&ctx, &mut adjacency, "source", "target").unwrap();
    assert_eq!(adjacency[0].targets, ["target"]);
    ctx.finish_session().unwrap();
}
