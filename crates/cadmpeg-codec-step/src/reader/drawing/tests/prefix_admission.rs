// SPDX-License-Identifier: Apache-2.0
//! Drawing traversal prefixes and wrapper result ownership.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::Value;

#[test]
fn drawing_reference_first_output_refusal_leaves_suffix_unvisited() {
    let value = Value::List(vec![Value::Reference(1); 8193]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut references = Vec::new();
    let CodecError::ResourceLimit(limit) = super::super::visit_drawing_references(
        &value,
        &ctx,
        &mut |id| ctx.push_vec(&mut references, id, "drawing fixture reference slots"),
    ).expect_err("first output slot refuses") else {
        panic!("collection refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "drawing fixture reference slots");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert!(references.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn drawing_reference_visitor_error_preserves_the_semantic_prefix() {
    let value = Value::List(vec![Value::Reference(1); 8193]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut visited = 0;
    let error = super::super::visit_drawing_references(&value, &ctx, &mut |_| {
        visited += 1;
        Err(CodecError::malformed("drawing target rejection"))
    }).expect_err("first visitor rejects");
    assert!(matches!(error, CodecError::Malformed(ref message) if message == "drawing target rejection"));
    assert_eq!(visited, 1);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().expect("unvisited suffix was not billed");
}

#[test]
fn drawing_reference_visits_accept_exact_count_and_keep_order() {
    let value = Value::List((1..=8).map(Value::Reference).collect());
    let mut references = Vec::with_capacity(8);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    policy.limits.max_collection_items = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    super::super::visit_drawing_references(
        &value,
        &ctx,
        &mut |id| ctx.push_vec(&mut references, id, "drawing fixture reference slots"),
    ).expect("eight actual visits fit");
    assert_eq!(references, [1, 2, 3, 4, 5, 6, 7, 8]);
    ctx.finish_session().expect("no exhausted source visit");
}

#[test]
fn empty_drawing_reference_list_is_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut visited = 0;
    super::super::visit_drawing_references(&Value::List(Vec::new()), &ctx, &mut |_| {
        visited += 1;
        Ok(())
    }).expect("empty source has no visit");
    assert_eq!(visited, 0);
    ctx.finish_session().expect("empty source fits zero work");
}

#[test]
fn empty_drawing_reference_list_keeps_an_existing_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let original = ctx.charge_work(1, "drawing original refusal").expect_err("original fuse");
    let mut visited = 0;
    let error = super::super::visit_drawing_references(&Value::List(Vec::new()), &ctx, &mut |_| {
        visited += 1;
        Ok(())
    }).expect_err("empty source observes original refusal");
    assert_eq!(error.to_string(), original.to_string());
    assert_eq!(visited, 0);
    assert_eq!(ctx.finish_session().expect_err("original session fuse").to_string(), original.to_string());
}

#[test]
fn ambiguous_wrapper_result_retains_only_its_live_identity_owner() {
    const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=MAPPED_ITEM('',#2,$);#2=REPRESENTATION_MAP($,#3);#3=REPRESENTATION('',(#4,#5),$);#4=ITEM();#5=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
        .expect("wrapper exchange");
    let targets = BTreeMap::from([
        (4, BTreeSet::from([String::from("first")])),
        (5, BTreeSet::from([String::from("second")])),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 64 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let Some(super::super::WrapperTargetResolution::Ambiguous((identity_buffer, identity_storage))) =
        super::super::wrapper_target_resolution(1, &targets, &exchange, &ctx).expect("ambiguous wrapper")
    else {
        panic!("two distinct target identities");
    };
    let identities = identity_buffer;
    assert_eq!(identities, BTreeSet::from([String::from("first"), String::from("second")]));
    // Two entries need one admitted String-key node. The node bound is
    // eleven key lanes, sixteen pointer lanes, and two alignment paddings.
    // Only that node and the two copied identity strings survive the query.
    let alignment = std::mem::align_of::<String>().max(std::mem::align_of::<usize>());
    let node_bytes = 11 * std::mem::size_of::<String>()
        + 16 * std::mem::size_of::<usize>() + 2 * alignment;
    let held_bytes = u64::try_from(node_bytes + "first".len() + "second".len()).expect("fixture bytes");
    let probe = ctx.reserve_scoped(policy.limits.max_materialized_bytes - held_bytes,
        "drawing released wrapper traversal").expect("obsolete workspace is released");
    drop(probe);
    drop(identities);
    drop(identity_storage);
    let probe = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
        "drawing released wrapper result").expect("result owner is released after its values");
    drop(probe);
    ctx.finish_session().expect("all actual storage fits");
}
