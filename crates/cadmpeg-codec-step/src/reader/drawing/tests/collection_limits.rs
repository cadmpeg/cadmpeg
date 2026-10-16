// SPDX-License-Identifier: Apache-2.0
//! Nesting and retention limits for drawing discovery.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

#[test]
fn drawing_target_records_are_charged_as_retained_bytes() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM('shape');ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid item record");
    let add = |limit: u64| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) =
            DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
        let mut ir = CadIr::empty();
        let result = super::super::add_source_typed_targets(
            &mut ir,
            &exchange,
            &HashSet::from([1_u64]),
            &BTreeSet::from([1]),
            &mut BTreeMap::new(),
            &ctx,
        );
        (result, ir)
    };

    let (result, ir) = add(u64::MAX);
    result.expect("an unbounded ceiling admits the target");
    let targets = &ir
        .native
        .namespace("step")
        .expect("STEP native namespace")
        .arenas()["drawing_targets"];
    assert_eq!(targets.len(), 1);
    let text = serde_json::to_string(&serde_json::json!({
        "id": targets[0].id(),
        "source_id": "#1",
        "source_type": "ITEM",
    }))
    .expect("record text");
    let needed = cadmpeg_core::decode::u64_from_index(text.len());

    let (result, _) = add(needed);
    result.expect("the record text fits the retained ceiling");

    let (result, ir) = add(needed - 1);
    assert!(matches!(
        result,
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.additional == needed
    ));
    assert!(ir.native.namespace("step").is_none());
}

#[test]
fn drawing_reference_walk_refuses_depth_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits depth policy");
    let value = crate::parse::Value::List(vec![crate::parse::Value::Reference(1)]);
    assert!(matches!(
        super::super::visit_drawing_references(&value, &ctx, &mut |_| Ok(())),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RecursionDepth
                && refusal.operation == "step_drawing_reference_walk"
    ));
}
