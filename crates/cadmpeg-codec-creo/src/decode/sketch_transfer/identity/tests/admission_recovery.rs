// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use std::ops::ControlFlow;

fn assert_empty_saved_route(present: bool) {
    let mut definition = super::definition(None);
    if present {
        definition.saved_section = Some(crate::feature::definitions::FeatureSavedSection {
            entities: Vec::new(), offset: 0,
        });
    }
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = refused.then(|| {
            ctx.charge_work_limit(1, "before empty saved visitor").expect_err("zero work")
        });
        for _ in 0..2 {
            let result = super::super::visit_semantic_saved_section_entities::<()>(&ctx, &definition, |_| {
                panic!("absent source must not call the visitor");
            });
            if let Some(original) = original {
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else { assert_eq!(result.expect("no saved rows"), ControlFlow::Continue(())); }
        }
        if let Some(original) = original {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else { ctx.finish_session().expect("active free route"); }
    }
}

#[test]
fn absent_saved_visitor_is_free_and_keeps_original_refusal() {
    assert_empty_saved_route(false);
}

#[test]
fn empty_saved_visitor_is_free_and_keeps_original_refusal() {
    assert_empty_saved_route(true);
}
