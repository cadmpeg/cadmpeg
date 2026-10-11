// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::ops::ControlFlow;

fn assert_empty_saved_route(present: bool) {
    let mut definition = super::definition(None);
    if present {
        definition.saved_section = Some(crate::feature::definitions::FeatureSavedSection {
            entities: Vec::new(), offset: 0,
        });
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let result = super::super::visit_semantic_saved_section_entities::<()>(&ctx, &definition, |_| {
        panic!("absent or empty source must not call the visitor");
    });
    assert_eq!(result.expect("no saved rows"), ControlFlow::Continue(()));
    ctx.finish_session().expect("active free route");

}

#[test]
fn absent_saved_visitor_is_free() {
    assert_empty_saved_route(false);
}

#[test]
fn empty_saved_visitor_is_free() {
    assert_empty_saved_route(true);
}
