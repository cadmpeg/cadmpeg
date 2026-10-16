// SPDX-License-Identifier: Apache-2.0
//! Recursion-depth refusals in the STEP PMI reader.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const HEADER: &str = "ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;";
const TAIL: &str = "ENDSEC;END-ISO-10303-21;";

#[test]
fn pmi_modifier_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Enumeration("ABC".into())]);
    assert!(matches!(
        super::super::modifier_values(&value, &mut Vec::new(), &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_modifier_walk"
    ));
}

#[test]
fn pmi_datum_id_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    assert!(matches!(
        super::super::visit_datum_ids(&value, &ctx, &mut |_| Ok(())),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_datum_id_walk"
    ));
}

#[test]
fn pmi_placement_walk_refuses_depth_limit() {
    let source = format!("{HEADER}#1=ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid exchange");
    let arena = DecodeArena::new();
    let (setup_ctx, _) =
        DecodeContext::from_root_bytes(source.as_bytes(), &arena, &DecodePolicy::default())
            .expect("setup root fits policy");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let (geometry, _) =
        crate::reader::geometry::decode(&exchange, &mut ir, &setup_ctx).expect("geometry setup");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    assert!(matches!(
        super::super::collect_placement_candidates(1, &exchange, &geometry.value, &mut BTreeMap::new(), &mut BTreeMap::new(), 0, &ctx),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_placement_walk"
    ));
}

fn measure_id_refusal(depth_limit: u64) -> CodecError {
    let source = format!("{HEADER}#1=MEASURE_REPRESENTATION_ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid measure exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    super::super::collect_measure_ids(
        &crate::parse::Value::Reference(1),
        &exchange,
        &mut std::collections::BTreeSet::new(),
        &mut std::collections::BTreeSet::new(),
        &ctx,
    )
    .expect_err("measure ID walk exceeds the limit")
}

#[test]
fn pmi_measure_id_walk_follows_the_session_depth_past_256_records() {
    let chain = (1..300).fold(String::new(), |mut chain, id| {
        write!(chain, "#{id}=ITEM(#{});", id + 1).expect("write chain fixture");
        chain
    });
    let source = format!("{HEADER}{chain}#300=MEASURE_REPRESENTATION_ITEM();{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid measure chain");
    let walk = |depth_limit: u64| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = depth_limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
        let mut measure_ids = BTreeSet::new();
        super::super::collect_measure_ids(
            &crate::parse::Value::Reference(1),
            &exchange,
            &mut BTreeSet::new(),
            &mut measure_ids,
            &ctx,
        )
        .map(|()| measure_ids)
    };

    assert_eq!(
        walk(1024).expect("the session admits the chain"),
        BTreeSet::from([300])
    );
    assert!(matches!(
        walk(200),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_measure_id_walk"
    ));
}

#[test]
fn pmi_measure_id_walk_refuses_depth_limit() {
    assert!(matches!(
        measure_id_refusal(0),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_measure_id_walk"
    ));
}

fn measure_eval_refusal(depth_limit: u64, record: &str) -> CodecError {
    let source = format!("{HEADER}{record}{TAIL}");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("valid measure exchange");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = depth_limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let mut losses = Vec::new();
    let mut measurements = super::super::MeasureContext {
        length_scale: 1.0,
        angle_scale: 1.0,
        losses: &mut losses,
    };
    super::super::measure(
        &crate::parse::Value::Reference(1),
        &exchange,
        &mut measurements,
        &ctx,
    )
    .expect_err("measure evaluation exceeds the limit")
}

#[test]
fn pmi_measure_eval_walk_refuses_depth_limit() {
    assert!(matches!(
        measure_eval_refusal(0, "#1=MEASURE_REPRESENTATION_ITEM();"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_measure_eval_walk"
    ));
}

#[test]
fn pmi_measure_quantity_walk_refuses_depth_limit() {
    assert!(matches!(
        measure_eval_refusal(1, "#1=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),$);"),
        CodecError::ResourceLimit(refusal)
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_pmi_measure_quantity_walk"
    ));
}

#[test]
fn pmi_target_reference_filter_propagates_depth_refusal() {
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 1;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let ids = crate::reader::reference::references(&value, ctx).filter(|id| match id {
            Ok(id) => *id == 1,
            Err(_) => true,
        });
        assert!(
            matches!(super::super::targets(ids), Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_reference_value_walk")
        );
    });
}
