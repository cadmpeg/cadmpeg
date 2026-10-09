// SPDX-License-Identifier: Apache-2.0
//! Completed invisibility queries and their publication boundary.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use super::super::{InvisibleIndex, InvisibleSummary, InvisibleWalk};
use crate::{loss::StepLossCode, StepCodec};

fn exchange(records: &str) -> crate::parse::Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("cache exchange").0
}

#[test]
fn invisible_body_refusal_frees_the_failed_active_path() {
    let exchange = exchange("#1=ITEM();");
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(b"", |_, setup| {
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, setup).expect("index");
        let mut topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, setup)
            .expect("topology");
        topology.value.body_by_root.insert(1, vec![cadmpeg_ir::ids::BodyId::from(
            crate::ids::data(crate::ids::kind!("body"), 1),
        )]);
        let indices = BTreeMap::from([("step:data:body#1".to_owned(), 0)]);
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 1;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let complete = BTreeMap::new();
            let mut body_storage = ctx.reserve_scoped(0, "test selected bodies").expect("scope");
            let mut bodies = BTreeMap::new();
            let mut walk = InvisibleWalk {
                active: BTreeSet::new(), complete: BTreeMap::new(), stage_complete: &complete,
                storage: ctx.reserve_scoped(0, "test completion map").expect("scope"),
            };
            let result = body_storage.with_storage(|| super::super::collect_invisible_body_ids(
                1, &exchange, &topology.value, &indices, &mut walk, &mut bodies, ctx,
            ));
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "step_presentation_invisible_body_ids"
                    && limit.used == 1 && limit.additional == 1
                    && ctx.resource_refusal() == Some(limit)));
            assert!(walk.active.is_empty());
            assert!(walk.complete.is_empty());
            assert!(bodies.is_empty());
        });
    });
}

#[test]
fn style_target_claim_refusal_frees_the_failed_active_path() {
    let exchange = exchange("#1=GEOMETRIC_SET('',(#2));#2=ITEM();");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut claim_storage = ctx.reserve_scoped(0, "test target claims").expect("scope");
        let mut claims = BTreeSet::new();
        let mut active = BTreeSet::new();
        let mut visited = 0;
        let result = super::super::expand_style_targets(
            1, &exchange, (&mut claims, &mut claim_storage), &mut active, (0, 128),
            &mut |_| { visited += 1; Ok(()) }, ctx,
        );
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_presentation_typed_claims"
                && limit.used == 1 && limit.additional == 1
                && ctx.resource_refusal() == Some(limit)));
        assert!(active.is_empty());
        assert!(claims.is_empty());
        assert_eq!(visited, 0);
    });
}

#[test]
fn style_domain_child_refusal_frees_the_failed_active_path() {
    let exchange = exchange("#1=GEOMETRIC_SET('',(#2));#2=ITEM();");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut storage = ctx.reserve_scoped(0, "test domain query").expect("scope");
        let mut active = BTreeSet::new();
        let mut pending = BTreeMap::new();
        let complete = BTreeMap::new();
        let result = storage.with_storage(|| super::super::style_domain_at(
            1, &exchange, &mut active, &mut pending, &complete, ctx,
        ));
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_presentation_style_domain_active"
                && limit.used == 1 && limit.additional == 1
                && ctx.resource_refusal() == Some(limit)));
        assert!(active.is_empty());
        assert!(pending.is_empty());
    });
}

#[test]
fn first_invisible_publication_preserves_a_caller_refusal() {
    let exchange = exchange("#1=ITEM();");
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(b"", |_, ctx| {
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("index");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, ctx)
            .expect("topology");
        let indices = BTreeMap::from([("step:data:body#1".to_owned(), 0)]);
        let mut index = InvisibleIndex::new(ctx).expect("index");
        let prepared = index.prepare(1, &exchange, &topology.value, &indices).expect("prepare");
        assert!(!prepared.pending.is_empty());
        let CodecError::ResourceLimit(first) = ctx.charge_work(u64::MAX, "test caller refusal")
            .expect_err("caller refuses before publication") else { panic!("resource refusal"); };
        assert!(matches!(index.publish(prepared), Err(CodecError::ResourceLimit(limit)) if limit == first));
        assert!(index.complete.is_empty());
        assert_eq!(ctx.resource_refusal(), Some(first));
    });
}

#[test]
fn invisible_query_refusal_keeps_completed_descendants_private() {
    let exchange = exchange("#1=REPRESENTATION('',(#2,#3),$);#2=ITEM();#3=ITEM();");
    let mut ir = CadIr::empty();
    let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &setup_arena, &DecodePolicy::service())
        .expect("setup context");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("index");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology").value;
    let indices = BTreeMap::from([("step:data:body#2".to_owned(), 0)]);
    let mut policy = DecodePolicy::service();
    // Root active, #2 active, one body ID and #2 completion use four slots.
    // The next child's active insertion is the fifth slot and must refuse.
    policy.limits.max_collection_items = 4;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let index = InvisibleIndex::new(ctx).expect("index");
        let error = match index.prepare(1, &exchange, &topology, &indices) {
            Err(error) => error,
            Ok(_) => panic!("second child must require another collection slot"),
        };
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_presentation_invisible_body_active"
                && limit.used == 4 && limit.additional == 1
                && ctx.resource_refusal() == Some(limit)));
        assert!(index.complete.is_empty());
    });
}

#[test]
fn unpublished_invisible_effects_are_not_reused() {
    let exchange = exchange("#1=ITEM();");
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(b"", |_, ctx| {
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("index");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, ctx)
            .expect("topology");
        let indices = BTreeMap::from([("step:data:body#1".to_owned(), 0)]);
        let mut index = InvisibleIndex::new(ctx).expect("index");
        let first = index.prepare(1, &exchange, &topology.value, &indices).expect("prepare");
        assert_eq!(first.summary, InvisibleSummary::Supported { hidden: true });
        assert_eq!(first.body_ids.len(), 1);
        drop(first); // Caller effect or warning failure does not publish.
        assert!(index.complete.is_empty());
        let second = index.prepare(1, &exchange, &topology.value, &indices).expect("retry");
        assert_eq!(second.body_ids.len(), 1);
        index.publish(second).expect("publish completed caller effects");
        let third = index.prepare(1, &exchange, &topology.value, &indices).expect("cache hit");
        assert!(third.body_ids.is_empty());
        assert_eq!(third.summary, InvisibleSummary::Supported { hidden: true });
    });
}

#[test]
fn supported_invisible_summary_requires_a_matching_body_index() {
    let exchange = exchange("#1=ITEM();");
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(b"", |_, ctx| {
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("index");
        let mut topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, ctx)
            .expect("topology");
        topology.value.body_by_root.insert(1, vec![cadmpeg_ir::ids::BodyId::from(
            crate::ids::data(crate::ids::kind!("body"), 99),
        )]);
        let mut index = InvisibleIndex::new(ctx).expect("index");
        let indices = BTreeMap::new();
        let first = index.prepare(1, &exchange, &topology.value, &indices).expect("prepare");
        assert_eq!(first.summary, InvisibleSummary::Supported { hidden: false });
        assert_eq!(first.body_ids.len(), 1);
        index.publish(first).expect("publish after unsupported-target warning");
        let second = index.prepare(1, &exchange, &topology.value, &indices).expect("cache hit");
        assert_eq!(second.summary, InvisibleSummary::Supported { hidden: false });
        assert!(second.body_ids.is_empty());
    });
}

#[test]
fn cached_invisible_query_does_not_enter_depth() {
    let exchange = exchange("#1=REPRESENTATION('',(#2),$);#2=ITEM();");
    let mut ir = CadIr::empty();
    let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &setup_arena, &DecodePolicy::service())
        .expect("setup context");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("index");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology").value;
    let indices = BTreeMap::from([("step:data:body#2".to_owned(), 0)]);
    let mut policy = DecodePolicy::service();
    // The first leaf query uses one depth. After publication a real caller
    // depth owns that one slot; the cached query must not enter another.
    policy.limits.max_recursion_depth = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = InvisibleIndex::new(ctx).expect("index");
        let first = index.prepare(2, &exchange, &topology, &indices).expect("leaf");
        index.publish(first).expect("completed leaf effects");
        let _parent_depth = ctx.enter_nested("test active parent query").expect("caller depth");
        let parent = index.prepare(2, &exchange, &topology, &indices).expect("cached query");
        assert_eq!(parent.summary, InvisibleSummary::Supported { hidden: true });
        assert!(parent.body_ids.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn reused_representation_invisibility_keeps_body_effects_and_warning_order() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap242_tessellation.p21").to_vec())
        .expect("UTF-8 fixture")
        .replace("ENDSEC;\nEND-ISO-10303-21;", "#90=REPRESENTATION('',(#39),#2);\n#91=INVISIBILITY((#39));\n#92=INVISIBILITY((#90));\n#93=INVISIBILITY((#999));\n#94=INVISIBILITY((#998));\n#998=ITEM();\n#999=ITEM();\nENDSEC;\nEND-ISO-10303-21;");
    let decoded = StepCodec::default().decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("shared representation invisibility");
    let body = decoded.ir().model.bodies.iter()
        .find(|body| body.id.as_str() == "step:data:body#38").expect("committed surface body");
    assert_eq!(body.visible, Some(false));
    let warnings = decoded.report().losses.iter()
        .filter(|loss| loss.code == StepLossCode::DecodeWarning.kind() && loss.message.starts_with("INVISIBILITY #"))
        .map(|loss| loss.message.as_str()).collect::<Vec<_>>();
    assert_eq!(warnings, [
        "INVISIBILITY #93 targets unsupported item #999",
        "INVISIBILITY #94 targets unsupported item #998",
    ]);
}

#[test]
fn invisible_prepared_body_entry_preserves_its_collection_refusal() {
    let exchange = exchange("#1=ITEM();");
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(b"", |_, setup| {
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, setup).expect("index");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, setup)
            .expect("topology");
        let indices = BTreeMap::from([("step:data:body#1".to_owned(), 0)]);
        let mut policy = DecodePolicy::service();
        // One active node fits. Its first prepared body is the second slot.
        policy.limits.max_collection_items = 1;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let index = InvisibleIndex::new(ctx).expect("index");
            let error = match index.prepare(1, &exchange, &topology.value, &indices) {
                Err(error) => error,
                Ok(_) => panic!("the first body entry must refuse"),
            };
            assert!(matches!(error, CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "step_presentation_invisible_body_ids"
                    && limit.used == 1 && limit.additional == 1
                    && ctx.resource_refusal() == Some(limit)));
            assert!(index.complete.is_empty());
        });
    });
}

#[test]
fn empty_color_frame_visits_no_terminal_step() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let partials: [crate::parse::PartialRecord; 0] = [];
    let parameters: [crate::parse::Value; 0] = [];
    let mut frame = super::super::ColorFrame {
        id: 1,
        depth: 0,
        partials: partials.iter(),
        partial_operation: "test empty color partials",
        transparency: None,
        side_rank: super::super::SurfaceSideRank::NoUsage,
        combine_children: true,
        parameter_operation: "test empty color parameters",
        parameters: Some(parameters.iter()),
        references: None,
        result: None,
        _depth: ctx.enter_nested("test empty color frame").expect("frame depth"),
    };
    assert_eq!(super::super::next_color_reference(&mut frame, &ctx)
        .expect("empty frame has no source work"), None);
    drop(frame);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().expect("no terminal-step refusal");
}
