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
    let CodecError::ResourceLimit(refusal) =
        super::super::collect_identity_indices(source, &ctx, "test first identity visit")
            .expect_err("first visit refuses before copying its identity")
    else {
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
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(1, "test original identity refusal")
        .expect_err("original refusal")
    else {
        panic!("identity resource refusal");
    };
    let source: [&str; 0] = [];
    assert!(
        matches!(super::super::collect_identity_indices(source, &ctx, "test empty identity index"),
        Err(CodecError::ResourceLimit(sticky)) if sticky == original)
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn hidden_style_terminals_do_not_enter_depth() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("style graph");
    for (id, limit, hidden, cache, expected) in [
        (1, 0, BTreeSet::from([1]), BTreeMap::new(), true),
        (3, 2, BTreeSet::from([1]), BTreeMap::new(), true),
        (3, 0, BTreeSet::new(), BTreeMap::from([(3, false)]), false),
        (4, 1, BTreeSet::new(), BTreeMap::new(), false),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = limit;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut storage = ctx
                .reserve_scoped(0, "terminal visibility fixture")
                .expect("scope");
            let mut cache = cache;
            assert_eq!(
                style_is_hidden(id, &hidden, &exchange, &mut cache, &mut storage, ctx)
                    .expect("terminal fits depth"),
                expected
            );
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn styles_without_bindings_skip_visibility_walk() {
    for target in ["GEOMETRIC_SET('',())", "MISSING_TARGET()"] {
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('',1.,0.,0.);#10=STYLED_ITEM('',(),#20);#11=OVER_RIDING_STYLED_ITEM('',(),#20,#10);#12=OVER_RIDING_STYLED_ITEM('',(),#20,#11);#13=OVER_RIDING_STYLED_ITEM('',(#1),#20,#12);#20={target};ENDSEC;END-ISO-10303-21;");
        let source = if target.starts_with("MISSING") {
            source
                .replace("#20=MISSING_TARGET();", "")
                .replace("(('test'),'2;1')", "(('test'),'4;2')")
                .replace(
                    "ENDSEC;DATA;",
                    "ENDSEC;REFERENCE;#20=<part.step#target>;ENDSEC;DATA;",
                )
        } else {
            source
        };
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("style exchange");
        let arena = DecodeArena::new();
        let (setup, _) =
            DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("setup");
        let mut ir = cadmpeg_ir::CadIr::empty();
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
            .expect("topology")
            .value;
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = 3;
        // An external target without a local record emits a warning. An empty
        // set emits no callback. Neither route evaluates binding visibility.
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let result = super::super::decode(&exchange, &topology, &mut ir, &BTreeMap::new(), ctx)
                .expect("unused visibility fits depth");
            assert!(ir.model.appearance_bindings.is_empty());
            assert_eq!(
                result.losses.len(),
                usize::from(target.starts_with("MISSING"))
            );
            assert_eq!(ctx.resource_refusal(), None);
        });
    }
}

#[test]
fn binding_visibility_keeps_its_independent_recursion_boundary() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=COLOUR_RGB('',1.,0.,0.);#10=STYLED_ITEM('',(#1),#20);#20=GEOMETRIC_SET('',(#21));#21=GEOMETRIC_SET('',(#22));#22=SOURCE_ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("target graph");
    let mut ir = cadmpeg_ir::CadIr::empty();
    let arena = DecodeArena::new();
    let (setup, _) =
        DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).expect("setup");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
        .expect("topology")
        .value;
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 3;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        super::super::decode(&exchange, &topology, &mut ir, &BTreeMap::new(), ctx)
            .expect("independent visibility depth");
        assert_eq!(ir.model.appearance_bindings.len(), 1);
        assert_eq!(ir.model.appearance_bindings[0].visible, None);
    });
}
