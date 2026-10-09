// SPDX-License-Identifier: Apache-2.0

use super::super::{style_application_order, style_is_hidden};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};

const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'4;2');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;REFERENCE;#99=<urn:style:missing>;ENDSEC;DATA;#1=STYLED_ITEM('',(),$);#2=OVER_RIDING_STYLED_ITEM('',(),$,#1);#3=OVER_RIDING_STYLED_ITEM('',(),$,#2);#4=OVER_RIDING_STYLED_ITEM('',(),$,#4);#5=OVER_RIDING_STYLED_ITEM('',(),$,#99);ENDSEC;END-ISO-10303-21;";

#[test]
fn style_depth_index_preserves_cached_zero_chain_cycle_and_graph_limit() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    crate::test_support::with_service_context(SOURCE, |_, ctx| {
        let mut cache = BTreeMap::new();
        let mut storage = ctx.reserve_scoped(0, "style index fixture").expect("scope");
        // External references resolve to resource values; #99 has no local record.
        for (id, expected) in [
            (1, Some(0)),
            (3, Some(2)),
            (2, Some(1)),
            (4, None),
            (5, Some(0)),
            (99, None),
        ] {
            assert_eq!(
                style_application_order(id, &exchange, 64, &mut cache, &mut storage, ctx)
                    .expect("depth"),
                (expected.is_none(), expected)
            );
        }
        assert_eq!(
            style_application_order(3, &exchange, 2, &mut cache, &mut storage, ctx)
                .expect("bounded depth"),
            (true, None)
        );
        assert_eq!(
            style_application_order(3, &exchange, 3, &mut cache, &mut storage, ctx)
                .expect("admitted depth"),
            (false, Some(2))
        );
        // A cache hit needs no collection slot and does not revisit the source.
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (hit_ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root");
        assert_eq!(
            style_application_order(1, &exchange, 64, &mut cache, &mut storage, &hit_ctx)
                .expect("cached zero"),
            (false, Some(0))
        );
    });
}

#[test]
fn hidden_style_index_preserves_ancestor_cycle_and_missing_record() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    crate::test_support::with_service_context(SOURCE, |_, ctx| {
        let mut cache = BTreeMap::new();
        let mut storage = ctx
            .reserve_scoped(0, "hidden style fixture")
            .expect("scope");
        let hidden = BTreeSet::from([1]);
        for (id, expected) in [(3, true), (2, true), (1, true), (4, false), (5, false)] {
            assert_eq!(
                style_is_hidden(id, &hidden, &exchange, &mut cache, &mut storage, ctx)
                    .expect("visibility"),
                expected
            );
        }
    });
}

#[test]
fn hidden_style_index_preserves_recursion_depth_refusal() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    let hidden = BTreeSet::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    crate::test_support::with_policy_context(SOURCE, &policy, |_, ctx| {
        let mut cache = BTreeMap::new();
        let mut storage = ctx
            .reserve_scoped(0, "hidden style fixture")
            .expect("scope");
        let error = style_is_hidden(3, &hidden, &exchange, &mut cache, &mut storage, ctx)
            .expect_err("second style exceeds the depth limit");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("style walk must refuse its depth limit");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(refusal.operation, "step_presentation_hidden_style_walk");
        assert_eq!(refusal.used, 1);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn style_depth_index_preserves_keyed_lookup_refusal() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "STEP style depth record lookup",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root");
            let mut storage = ctx.reserve_scoped(0, "style fixture").expect("scope");
            let result =
                style_application_order(3, &exchange, 64, &mut BTreeMap::new(), &mut storage, &ctx);
            if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                assert_eq!(ctx.resource_refusal(), Some(*refusal));
            }
            result
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(refusal) if refusal.operation == "STEP style depth record lookup")
    );
}

#[test]
fn zero_style_graph_limit_performs_no_source_work() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &policy).expect("root");
    let mut cache = BTreeMap::new();
    let mut storage = ctx.reserve_scoped(0, "style fixture").expect("scope");
    for id in [1, 3, 4, 99] {
        assert_eq!(
            style_application_order(id, &exchange, 0, &mut cache, &mut storage, &ctx)
                .expect("zero-depth gate"),
            (true, None)
        );
    }
    assert!(cache.is_empty());
}

#[test]
fn truncated_style_depth_does_not_cache_unvisited_suffixes() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    crate::test_support::with_service_context(SOURCE, |_, ctx| {
        let mut cache = BTreeMap::new();
        let mut storage = ctx.reserve_scoped(0, "style fixture").expect("scope");
        assert_eq!(
            style_application_order(3, &exchange, 1, &mut cache, &mut storage, ctx)
                .expect("bounded path"),
            (true, None)
        );
        assert!(cache.is_empty());
        assert_eq!(
            style_application_order(1, &exchange, 1, &mut cache, &mut storage, ctx)
                .expect("base depth"),
            (false, Some(0))
        );
        assert_eq!(cache, BTreeMap::from([(1, Some(0))]));
        assert_eq!(
            style_application_order(2, &exchange, 2, &mut cache, &mut storage, ctx)
                .expect("cached base"),
            (false, Some(1))
        );
    });
}

#[test]
fn style_depth_path_cache_visits_prefix_before_work_refusal() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    // This three-style path needs at most three visited-set and three cache
    // insertions. The largest node pass is 320 work units, and each insertion
    // admits at most three passes, for at most 5,760 units. The remaining
    // budget covers keyed lookups and path steps. Advance one unit at a time
    // and accept only the refusal after the first cache insertion.
    let found_prefix_refusal = (0..=8_192).any(|work_limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
        let mut cache = BTreeMap::new();
        let mut storage = ctx.reserve_scoped(0, "style fixture").expect("scope");
        let result = style_application_order(
            3,
            &exchange,
            64,
            &mut cache,
            &mut storage,
            &ctx,
        );
        let Err(CodecError::ResourceLimit(refusal)) = result else {
            return false;
        };
        assert_eq!(ctx.resource_refusal().as_ref(), Some(&refusal));
        if refusal.operation != "STEP style depth path result traversal" {
            return false;
        }
        if refusal.dimension != ResourceDimension::WorkUnits
            || cache != BTreeMap::from([(1, Some(0))])
        {
            return false;
        }
        true
    });
    assert!(found_prefix_refusal, "no partial style-depth cache boundary");
}

#[test]
fn hidden_style_path_cache_visits_prefix_before_work_refusal() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    let hidden = BTreeSet::new();
    // The same three-entry bound applies: six insertions need at most 5,760
    // work units at three 320-unit node passes each. The remaining budget
    // covers keyed lookups and path steps before the prefix refusal.
    let found_prefix_refusal = (0..=8_192).any(|work_limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
        let mut cache = BTreeMap::new();
        let mut storage = ctx.reserve_scoped(0, "hidden style fixture").expect("scope");
        let result = style_is_hidden(3, &hidden, &exchange, &mut cache, &mut storage, &ctx);
        let Err(CodecError::ResourceLimit(refusal)) = result else {
            return false;
        };
        assert_eq!(ctx.resource_refusal().as_ref(), Some(&refusal));
        if refusal.operation != "STEP hidden style path result traversal" {
            return false;
        }
        if refusal.dimension != ResourceDimension::WorkUnits
            || cache != BTreeMap::from([(3, false)])
        {
            return false;
        }
        true
    });
    assert!(found_prefix_refusal, "no partial hidden-style cache boundary");
}

#[test]
fn empty_presentation_identity_index_visits_no_terminal_step() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let source: [&str; 0] = [];
    let result = super::super::collect_identity_indices(source, &ctx, "test empty identity index")
        .expect("empty index has no source work or allocation");
    assert!(result.is_empty());
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().expect("no terminal-step refusal");
}

#[test]
fn presentation_identity_index_refuses_only_first_visit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let source = ["step:model:item#1"; 1024];
    let CodecError::ResourceLimit(refusal) = super::super::collect_identity_indices(
        source, &ctx, "test first identity visit",
    ).expect_err("first visit refuses before copying its identity") else {
        panic!("identity visit resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "test first identity visit");
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}

#[test]
fn empty_presentation_identity_index_preserves_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "test original identity refusal")
        .expect_err("original refusal") else {
        panic!("identity resource refusal");
    };
    let source: [&str; 0] = [];
    assert!(matches!(super::super::collect_identity_indices(source, &ctx, "test empty identity index"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}
