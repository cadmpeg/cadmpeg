// SPDX-License-Identifier: Apache-2.0
//! Completed invisibility queries, effects, and caller refusals.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{CadIr, Codec, DecodeOptions};

use super::super::{InvisibleIndex, InvisibleSummary};
use crate::{loss::StepLossCode, StepCodec};

fn exchange(records: &str) -> crate::parse::Exchange {
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("cache exchange")
        .0
}

#[test]
fn style_target_claim_refuses_before_visiting_a_target() {
    let exchange = exchange("#1=GEOMETRIC_SET('',(#2));#2=ITEM();");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut active_storage = ctx.reserve_scoped(0, "test active targets").expect("scope");
        let mut claim_storage = ctx.reserve_scoped(0, "test target claims").expect("scope");
        let mut claims = BTreeSet::new();
        let mut active = BTreeSet::new();
        let mut visited = 0;
        let result = super::super::expand_style_targets(
            1,
            &exchange,
            (&mut claims, &mut claim_storage),
            (&mut active, &mut active_storage),
            (0, 128),
            &mut |_| {
                visited += 1;
                Ok(())
            },
            ctx,
        );
        assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_presentation_typed_claims"
                && limit.used == 1 && limit.additional == 1
                && ctx.resource_refusal() == Some(limit)));

        assert!(claims.is_empty());
        assert_eq!(visited, 0);
    });
}

#[test]
fn invisible_query_preserves_collection_refusal() {
    let exchange = exchange("#1=REPRESENTATION('',(#2,#3),$);#2=ITEM();#3=ITEM();");
    let mut ir = CadIr::empty();
    let setup_arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &setup_arena, &DecodePolicy::service())
        .expect("setup context");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("index");
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
        .expect("topology")
        .value;
    let indices = BTreeMap::from([("step:data:body#2".to_owned(), 0)]);
    let mut policy = DecodePolicy::service();
    // Root active, #2 active, one body ID and #2 completion use four slots.
    // The next child's active insertion is the fifth slot and must refuse.
    policy.limits.max_collection_items = 4;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = InvisibleIndex::new(ctx).expect("index");
        let error = match index.resolve(1, &exchange, &topology, &indices) {
            Err(error) => error,
            Ok(_) => panic!("second child must require another collection slot"),
        };
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "step_presentation_invisible_body_active"
                && limit.used == 4 && limit.additional == 1
                && ctx.resource_refusal() == Some(limit)));
    });
}

#[test]
fn supported_invisible_summary_requires_a_matching_body_index() {
    let exchange = exchange("#1=ITEM();");
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(b"", |_, ctx| {
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, ctx).expect("index");
        let mut topology =
            crate::reader::topology::decode(&exchange, &mut ir, &carriers, ctx).expect("topology");
        topology.value.body_by_root.insert(
            1,
            vec![cadmpeg_ir::ids::BodyId::from(crate::ids::data(
                crate::ids::kind!("body"),
                99,
            ))],
        );
        let mut index = InvisibleIndex::new(ctx).expect("index");
        let indices = BTreeMap::new();
        let first = index
            .resolve(1, &exchange, &topology.value, &indices)
            .expect("prepare");
        assert_eq!(first.0, InvisibleSummary::Supported { hidden: false });
        assert_eq!(first.1.len(), 1);
        let second = index
            .resolve(1, &exchange, &topology.value, &indices)
            .expect("cache hit");
        assert_eq!(second.0, InvisibleSummary::Supported { hidden: false });
        assert!(second.1.is_empty());
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
    let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
        .expect("topology")
        .value;
    let indices = BTreeMap::from([("step:data:body#2".to_owned(), 0)]);
    let mut policy = DecodePolicy::service();
    // The first leaf query uses one depth. After completion a real caller
    // depth owns that one slot; the cached query must not enter another.
    policy.limits.max_recursion_depth = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = InvisibleIndex::new(ctx).expect("index");
        let _first = index
            .resolve(2, &exchange, &topology, &indices)
            .expect("leaf");
        let _parent_depth = ctx
            .enter_nested("test active parent query")
            .expect("caller depth");
        let parent = index
            .resolve(2, &exchange, &topology, &indices)
            .expect("cached query");
        assert_eq!(parent.0, InvisibleSummary::Supported { hidden: true });
        assert!(parent.1.is_empty());
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn reused_representation_invisibility_keeps_body_effects_and_warning_order() {
    let source = String::from_utf8(include_bytes!("../../../../tests/fixtures/ap242_tessellation.p21").to_vec())
        .expect("UTF-8 fixture")
        .replace("ENDSEC;\nEND-ISO-10303-21;", "#90=REPRESENTATION('',(#39),#2);\n#91=INVISIBILITY((#39));\n#92=INVISIBILITY((#90));\n#93=INVISIBILITY((#999));\n#94=INVISIBILITY((#998));\n#998=ITEM();\n#999=ITEM();\nENDSEC;\nEND-ISO-10303-21;");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("shared representation invisibility");
    let body = decoded
        .ir()
        .model
        .bodies
        .iter()
        .find(|body| body.id.as_str() == "step:data:body#38")
        .expect("committed surface body");
    assert_eq!(body.visible, Some(false));
    let warnings = decoded
        .report()
        .losses
        .iter()
        .filter(|loss| {
            loss.code == StepLossCode::DecodeWarning.kind()
                && loss.message.starts_with("INVISIBILITY #")
        })
        .map(|loss| loss.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        warnings,
        [
            "INVISIBILITY #93 targets unsupported item #999",
            "INVISIBILITY #94 targets unsupported item #998",
        ]
    );
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
        loss_start: 0,
        partials: partials.iter(),
        partial_operation: "test empty color partials",
        transparency: None,
        side_rank: super::super::SurfaceSideRank::NoUsage,
        combine_children: true,
        parameter_operation: "test empty color parameters",
        parameters: Some(parameters.iter()),
        references: None,
        result: super::super::ColorResult {
            color: None,
            height: Some(0),
        },
        _depth: ctx
            .enter_nested("test empty color frame")
            .expect("frame depth"),
    };
    assert_eq!(
        super::super::next_color_reference(&mut frame, &ctx)
            .expect("empty frame has no source work"),
        None
    );
    drop(frame);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().expect("no terminal-step refusal");
}
#[test]
fn style_domain_completion_keeps_empty_active_tree_admitted() {
    let exchange = exchange("#1=CARTESIAN_POINT('',(0.,0.,0.));");
    let mut policy = DecodePolicy::service();
    // One u64-key active leaf: eleven key lanes, sixteen pointer lanes,
    // and two alignment paddings. Removal leaves its empty root allocated.
    let alignment = std::mem::align_of::<u64>().max(std::mem::align_of::<usize>());
    let active_bytes =
        11 * std::mem::size_of::<u64>() + 16 * std::mem::size_of::<usize>() + 2 * alignment;
    policy.limits.max_materialized_bytes = u64::try_from(active_bytes).expect("node bytes");
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = super::super::StyleDomainIndex::new(ctx).expect("index");
        assert!(
            matches!(index.domain(1, &exchange), Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.operation == "step_style_domain_cache"
                && limit.used == u64::try_from(active_bytes).expect("node bytes")
                && ctx.resource_refusal() == Some(limit))
        );
    });
}

#[test]
fn style_target_active_tree_stays_admitted_until_dropped() {
    let exchange = exchange("#1=GEOMETRIC_SET('',());");
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 64 * 1024;
    let alignment = std::mem::align_of::<u64>().max(std::mem::align_of::<usize>());
    let node_bytes = u64::try_from(
        11 * std::mem::size_of::<u64>() + 16 * std::mem::size_of::<usize>() + 2 * alignment,
    )
    .expect("node bytes");
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut active_storage = ctx.reserve_scoped(0, "active targets").expect("scope");
        let mut active = BTreeSet::new();
        let mut claim_storage = ctx.reserve_scoped(0, "target claims").expect("scope");
        let mut claims = BTreeSet::new();
        super::super::expand_style_targets(
            1,
            &exchange,
            (&mut claims, &mut claim_storage),
            (&mut active, &mut active_storage),
            (0, 128),
            &mut |_| panic!("empty set"),
            ctx,
        )
        .expect("empty expansion");
        assert!(active.is_empty());
        assert_eq!(claims, BTreeSet::from([1]));
        // The claim tree and the empty active tree each keep one admitted node.
        assert!(
            matches!(ctx.reserve_scoped(policy.limits.max_materialized_bytes - node_bytes, "live active root probe"), Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
                && limit.used == 2 * node_bytes
                && ctx.resource_refusal() == Some(limit))
        );
    });
}
