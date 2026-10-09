// SPDX-License-Identifier: Apache-2.0
//! Inherited line rules preserve requester units and original diagnostics.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::PositiveReal;

use super::super::LineParameterScaleIndex;

fn source(magnitude: f64, ancestors: usize) -> String {
    let mut source = format!(
        "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=LINE('',#1002,#1000);#1000=VECTOR('',#1001,{magnitude});#1001=DIRECTION('',(1.,0.,0.));#1002=CARTESIAN_POINT('',(0.,0.,0.));"
    );
    for id in 2..=ancestors + 1 {
        source.push_str(&format!(
            "#{id}=TRIMMED_CURVE('',#{},(0.),(1.),.T.,.PARAMETER.);", id - 1
        ));
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    source
}

#[test]
fn inherited_line_rules_reuse_completed_ancestors_at_depth_two() {
    let source = source(2.0, 64);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The new requester and its already resolved parent are the only frames.
    policy.limits.max_recursion_depth = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    let mut losses = Vec::new();
    for id in 1..=65 {
        assert_eq!(index.resolve(id, PositiveReal::ONE, &mut losses)
            .expect("each ancestor is resolved once").get(), 2.0);
    }
    assert!(losses.is_empty());
    drop(index);
    ctx.finish_session().expect("successful memoized walk");
}

#[test]
fn inherited_line_rules_preserve_cold_walk_depth_refusal() {
    let source = source(2.0, 64);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    let CodecError::ResourceLimit(first) = index.resolve(65, PositiveReal::ONE, &mut Vec::new())
        .expect_err("third actual frame refuses") else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(first.operation, "step_line_parameter_scale_walk");
    assert_eq!(first.used, 2);
    assert_eq!(first.additional, 1);
    assert_eq!(first.limit, 2);
    drop(index);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn inherited_line_rules_apply_each_requesters_units_after_cache_lookup() {
    let source = source(2.0, 2);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    let mut losses = Vec::new();
    for (root, unit, expected) in [(3, 1.0, 2.0), (2, 10.0, 20.0), (3, 3.0, 6.0)] {
        assert_eq!(index.resolve(root, PositiveReal::new(unit).expect("positive unit"), &mut losses)
            .expect("resolved rule").get(), expected);
    }
    assert!(losses.is_empty());
}

#[test]
fn inherited_line_rules_preserve_each_unresolved_line_diagnostic() {
    let source = source(-1.0, 2);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    let mut losses = Vec::new();
    for root in [3, 2, 3] {
        let unit = PositiveReal::new(10.0).expect("positive unit");
        assert_eq!(index.resolve(root, unit, &mut losses).expect("fallback rule"), unit);
    }
    assert_eq!(losses.len(), 3);
    for loss in losses {
        assert_eq!(loss.message, "LINE #1 parameter scale did not resolve; the document length scale was used");
    }
}

#[test]
fn inherited_line_rules_do_not_cache_a_requesters_numeric_overflow() {
    let source = source(2.0, 1);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    let mut losses = Vec::new();
    let large = PositiveReal::new(f64::MAX).expect("finite unit");
    assert_eq!(index.resolve(2, large, &mut losses).expect("overflow fallback"), large);
    assert_eq!(losses.len(), 1);
    assert_eq!(index.resolve(2, PositiveReal::new(3.0).expect("positive unit"), &mut losses)
        .expect("cached magnitude remains valid").get(), 6.0);
    assert_eq!(losses.len(), 1);
}

#[test]
fn inherited_line_rules_preserve_sticky_refusal_on_a_cache_hit() {
    let source = source(2.0, 0);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    index.resolve(1, PositiveReal::ONE, &mut Vec::new()).expect("populate memo");
    let CodecError::ResourceLimit(first) = ctx.charge_work(u64::MAX, "test original refusal")
        .expect_err("original work refusal") else { panic!("resource refusal") };
    assert!(matches!(index.resolve(1, PositiveReal::ONE, &mut Vec::new()),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    drop(index);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn inherited_line_rules_preserve_cycle_and_missing_record_fallbacks() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=TRIMMED_CURVE('',#2,(0.),(1.),.T.,.PARAMETER.);#2=TRIMMED_CURVE('',#1,(0.),(1.),.T.,.PARAMETER.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(
        source, crate::parse::parse_inner,
    ).expect("valid cyclic source");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(source, &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    let mut losses = Vec::new();
    for (root, unit) in [(1, 2.0), (2, 3.0), (42, 4.0), (42, 5.0)] {
        let unit = PositiveReal::new(unit).expect("positive unit");
        assert_eq!(index.resolve(root, unit, &mut losses).expect("fallback"), unit);
    }
    assert!(losses.is_empty());
}

#[test]
fn inherited_line_rules_admit_the_first_memo_slot_before_insertion() {
    let source = source(2.0, 0);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The active source key consumes the first slot; the memo needs the second.
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    let CodecError::ResourceLimit(first) = index.resolve(1, PositiveReal::ONE, &mut Vec::new())
        .expect_err("memo slot refuses") else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "step line parameter scale memo");
    assert_eq!(first.used, 1);
    assert_eq!(first.additional, 1);
    assert_eq!(first.limit, 1);
    assert!(matches!(index.resolve(1, PositiveReal::ONE, &mut Vec::new()),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    drop(index);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn inherited_line_rules_keep_memo_storage_scoped_and_reuse_its_slot() {
    let source = source(2.0, 0);
    let (exchange, _) = crate::test_support::with_service_context(
        source.as_bytes(), crate::parse::parse_inner,
    ).expect("valid source");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(source.as_bytes(), &arena, &policy)
        .expect("source fits policy");
    let mut index = LineParameterScaleIndex::new(&exchange, &ctx).expect("empty memo");
    for unit in [PositiveReal::ONE, PositiveReal::new(10.0).expect("positive unit")] {
        assert_eq!(index.resolve(1, unit, &mut Vec::new()).expect("scoped memo").get(),
            2.0 * unit.get());
    }
    drop(index);
    ctx.finish_session().expect("no retained cache or duplicate slot");
}
