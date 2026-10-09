// SPDX-License-Identifier: Apache-2.0
//! A representation context propagates each reachable source member once.

use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

use super::super::resolve_unit_scales;
use crate::parse::Exchange;
use crate::test_support::with_service_context;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";
const MM_CONTEXT: &str = "#1=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#2=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('',''));";
const CM_CONTEXT: &str = "#3=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.CENTI.,.METRE.));#4=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#3)) REPRESENTATION_CONTEXT('',''));";

fn exchange(records: &str) -> Exchange {
    let source = format!("{HEADER}{records}{TAIL}");
    with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("valid unit-scope exchange").0
}

fn with_context(policy: &DecodePolicy, run: impl FnOnce(DecodeContext<'_>)) {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, policy).expect("empty root");
    run(ctx);
}

fn default_length() -> PositiveReal {
    PositiveReal::new(2.0).expect("positive default")
}

#[test]
fn unit_scope_repeated_contexts_need_only_distinct_member_slots() {
    const MEMBERS: u64 = 32;
    const REPRESENTATIONS: u64 = 200;
    let mut records = MM_CONTEXT.to_owned();
    for offset in 0..MEMBERS {
        let id = 10 + offset;
        if offset + 1 < MEMBERS {
            write!(records, "#{id}=ITEM(#{});", id + 1).expect("synthetic chain");
        } else {
            write!(records, "#{id}=ITEM();").expect("synthetic leaf");
        }
    }
    for offset in 0..REPRESENTATIONS {
        write!(records, "#{}=SHAPE_REPRESENTATION('',(#10),#2);", 1000 + offset)
            .expect("synthetic representation");
    }
    let exchange = exchange(&records);
    let mut policy = DecodePolicy::service();
    // Unit resolution: three slots. Context index: one. Each representation:
    // candidate group + value + selected slot. Each distinct member: active,
    // member, candidate group + value + selected slot; chain pending: N - 1.
    let slots = 3 + 3 * REPRESENTATIONS + 6 * MEMBERS;
    policy.limits.max_collection_items = slots;
    policy.limits.max_retained_bytes = 0;
    with_context(&policy, |ctx_owner| {
        let ctx = &ctx_owner;
        let mut storage = ctx.reserve_scoped(0, "test selected units").expect("empty owner");
        let mut losses = Vec::new();
        let units = resolve_unit_scales(&exchange, default_length(), PositiveReal::ONE,
            &mut losses, &mut storage, ctx).expect("only distinct context/member slots");
        for id in 10..10 + MEMBERS {
            assert_eq!(units.length([id], ctx).expect("member scale"), PositiveReal::ONE);
        }
        assert_eq!(units.length([1199], ctx).expect("last representation scale"), PositiveReal::ONE);
        assert!(losses.is_empty());
        let CodecError::ResourceLimit(first) = ctx.charge_collection_items(1, "test next slot")
            .expect_err("exact count exhausted") else { panic!("resource refusal") };
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "test next slot");
        assert_eq!((first.used, first.additional, first.limit), (slots, 1, slots));
        assert!(matches!(ctx.charge_work(0, "later operation"), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
        drop(units);
        drop(losses);
        drop(storage);
        assert!(matches!(ctx_owner.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    });
}

#[test]
fn unit_scope_context_index_refuses_before_member_propagation() {
    let exchange = exchange(&format!("{MM_CONTEXT}#10=ITEM();#20=SHAPE_REPRESENTATION('',(#10),#2);"));
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    with_context(&policy, |ctx_owner| {
        let ctx = &ctx_owner;
        let mut storage = ctx.reserve_scoped(0, "test selected units").expect("empty owner");
        let mut losses = Vec::new();
        let CodecError::ResourceLimit(first) = resolve_unit_scales(&exchange, default_length(),
            PositiveReal::ONE, &mut losses, &mut storage, ctx).err().expect("context slot refuses")
            else { panic!("resource refusal") };
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "step unit context index");
        assert_eq!((first.used, first.additional, first.limit), (3, 1, 3));
        assert!(losses.is_empty());
        assert!(matches!(ctx.charge_work(0, "later operation"), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
        drop(losses);
        drop(storage);
        assert!(matches!(ctx_owner.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    });
}

#[test]
fn unit_scope_distinct_contexts_preserve_conflicts() {
    let exchange = exchange(&format!("{MM_CONTEXT}{CM_CONTEXT}#10=ITEM();#20=SHAPE_REPRESENTATION('',(#10),#2);#21=SHAPE_REPRESENTATION('',(#10),#4);"));
    with_context(&DecodePolicy::service(), |ctx_owner| {
        let ctx = &ctx_owner;
        let mut storage = ctx.reserve_scoped(0, "test selected units").expect("empty owner");
        let mut losses = Vec::new();
        let units = resolve_unit_scales(&exchange, default_length(), PositiveReal::ONE,
            &mut losses, &mut storage, ctx).expect("both contexts propagate");
        assert_eq!(units.length([10], ctx).expect("conflicting member"), default_length());
        assert_eq!(units.length([20], ctx).expect("millimetre representation"), PositiveReal::ONE);
        assert_eq!(units.length([21], ctx).expect("centimetre representation").get(), 10.0);
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].code, crate::loss::StepLossCode::ConflictingRepresentationUnits.kind());
        assert_eq!(losses[0].message, "1 geometry record(s) belong to representations with conflicting length units; source-order unit selection was not applied");
        drop(units);
        drop(losses);
        drop(storage);
        ctx_owner.finish_session().expect("unrefused session");
    });
}

#[test]
fn unit_scope_equal_units_in_distinct_contexts_remain_unambiguous() {
    let exchange = exchange(&format!("{MM_CONTEXT}#4=(GLOBAL_UNIT_ASSIGNED_CONTEXT((#1)) REPRESENTATION_CONTEXT('',''));#10=ITEM();#20=SHAPE_REPRESENTATION('',(#10),#2);#21=SHAPE_REPRESENTATION('',(#10),#4);"));
    with_context(&DecodePolicy::service(), |ctx_owner| {
        let ctx = &ctx_owner;
        let mut storage = ctx.reserve_scoped(0, "test selected units").expect("empty owner");
        let mut losses = Vec::new();
        let units = resolve_unit_scales(&exchange, default_length(), PositiveReal::ONE,
            &mut losses, &mut storage, ctx).expect("equal contexts propagate");
        assert_eq!(units.length([10], ctx).expect("member units"), PositiveReal::ONE);
        assert!(losses.is_empty());
        drop(units);
        drop(losses);
        drop(storage);
        ctx_owner.finish_session().expect("unrefused session");
    });
}

#[test]
fn unit_scope_cycles_keep_every_reachable_member() {
    let exchange = exchange(&format!("{MM_CONTEXT}#10=ITEM(#11);#11=ITEM(#10);#20=SHAPE_REPRESENTATION('',(#10),#2);#21=SHAPE_REPRESENTATION('',(#11),#2);"));
    with_context(&DecodePolicy::service(), |ctx_owner| {
        let ctx = &ctx_owner;
        let mut storage = ctx.reserve_scoped(0, "test selected units").expect("empty owner");
        let mut losses = Vec::new();
        let units = resolve_unit_scales(&exchange, default_length(), PositiveReal::ONE,
            &mut losses, &mut storage, ctx).expect("cycle terminates");
        assert_eq!(units.length([10], ctx).expect("first member"), PositiveReal::ONE);
        assert_eq!(units.length([11], ctx).expect("second member"), PositiveReal::ONE);
        assert!(losses.is_empty());
        drop(units);
        drop(losses);
        drop(storage);
        ctx_owner.finish_session().expect("unrefused session");
    });
}

#[test]
fn unit_scope_mapped_items_propagate_only_target_context() {
    let exchange = exchange(&format!("{MM_CONTEXT}{CM_CONTEXT}#10=ITEM();#11=ITEM();#12=REPRESENTATION_MAP(#11,#21);#13=MAPPED_ITEM('',#12,#10);#20=SHAPE_REPRESENTATION('',(#13),#2);#21=SHAPE_REPRESENTATION('',(#11),#4);"));
    with_context(&DecodePolicy::service(), |ctx_owner| {
        let ctx = &ctx_owner;
        let mut storage = ctx.reserve_scoped(0, "test selected units").expect("empty owner");
        let mut losses = Vec::new();
        let units = resolve_unit_scales(&exchange, default_length(), PositiveReal::ONE,
            &mut losses, &mut storage, ctx).expect("mapped contexts remain separate");
        assert_eq!(units.length([10], ctx).expect("target units"), PositiveReal::ONE);
        assert_eq!(units.length([11], ctx).expect("source units").get(), 10.0);
        assert_eq!(units.length([12], ctx).expect("unpropagated mapping source"), default_length());
        assert!(losses.is_empty());
        drop(units);
        drop(losses);
        drop(storage);
        ctx_owner.finish_session().expect("unrefused session");
    });
}

#[test]
fn unit_scope_pcurve_does_not_propagate_into_its_definition() {
    let exchange = exchange(&format!("{MM_CONTEXT}#10=ITEM();#11=DEFINITIONAL_REPRESENTATION('',(#10),#30);#12=PCURVE('',#10,#11);#20=SHAPE_REPRESENTATION('',(#12),#2);#30=REPRESENTATION_CONTEXT('','');"));
    with_context(&DecodePolicy::service(), |ctx_owner| {
        let ctx = &ctx_owner;
        let mut storage = ctx.reserve_scoped(0, "test selected units").expect("empty owner");
        let mut losses = Vec::new();
        let units = resolve_unit_scales(&exchange, default_length(), PositiveReal::ONE,
            &mut losses, &mut storage, ctx).expect("pcurve scope is bounded");
        assert_eq!(units.length([12], ctx).expect("pcurve units"), PositiveReal::ONE);
        assert_eq!(units.length([10], ctx).expect("unpropagated definition item"), default_length());
        assert_eq!(units.length([11], ctx).expect("unpropagated definition representation"), default_length());
        assert!(losses.is_empty());
        drop(units);
        drop(losses);
        drop(storage);
        ctx_owner.finish_session().expect("unrefused session");
    });
}
