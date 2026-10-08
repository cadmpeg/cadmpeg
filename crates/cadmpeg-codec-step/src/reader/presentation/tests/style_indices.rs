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
