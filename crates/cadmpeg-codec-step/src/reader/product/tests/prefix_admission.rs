// SPDX-License-Identifier: Apache-2.0
//! Product traversal visits and refusal prefixes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parse::Value;

#[test]
fn first_reference_collection_refusal_leaves_the_list_suffix_unvisited() {
    let value = Value::List(vec![Value::Reference(1); 8193]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut references = Vec::new();
    let CodecError::ResourceLimit(limit) = super::super::collect_references(
        &value,
        &mut references,
        &ctx,
    ).expect_err("the first output slot refuses") else {
        panic!("collection refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "step_drawing_owned_pending");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert!(references.is_empty());
    assert!(matches!(value, Value::List(ref values) if values.len() == 8193));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn reference_list_accepts_exact_source_visits_and_keeps_order() {
    let value = Value::List((1..=8).map(Value::Reference).collect());
    // The caller owns an existing eight-slot output buffer.
    let mut references = Vec::with_capacity(8);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 8;
    policy.limits.max_collection_items = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    super::super::collect_references(&value, &mut references, &ctx).expect("eight visits fit");
    assert_eq!(references, [1, 2, 3, 4, 5, 6, 7, 8]);
    ctx.finish_session().expect("no exhausted list visit");
}

#[test]
fn second_reference_visit_preserves_the_original_work_refusal() {
    let value = Value::List(vec![Value::Omitted; 8]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let CodecError::ResourceLimit(limit) = super::super::collect_references(
        &value,
        &mut Vec::new(),
        &ctx,
    ).expect_err("the second visit refuses") else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "STEP collect references value traversal");
    assert_eq!((limit.used, limit.additional), (1, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn empty_reference_list_is_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut references = Vec::new();
    super::super::collect_references(&Value::List(Vec::new()), &mut references, &ctx)
        .expect("empty list has no visit");
    assert!(references.is_empty());
    ctx.finish_session().expect("empty source stays within zero work");
}

#[test]
fn empty_reference_list_preserves_an_existing_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let original = ctx.charge_work(1, "product original refusal").expect_err("original fuse");
    let error = super::super::collect_references(&Value::List(Vec::new()), &mut Vec::new(), &ctx)
        .expect_err("empty source observes original fuse");
    assert_eq!(error.to_string(), original.to_string());
    assert_eq!(ctx.finish_session().expect_err("fused session").to_string(), original.to_string());
}

#[test]
fn joined_product_references_accept_exact_actual_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two source visits, two formatting passes over four bytes, two
    // two-item join traversals, and six output bytes: 2 + 8 + 4 + 6.
    policy.limits.max_work_units = 20;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let text = super::super::join_product_references([1, 2].into_iter(), &ctx, "product reference fixture")
        .expect("only actual source visits are billed");
    assert_eq!(text, "#1, #2");
    ctx.finish_session().expect("no exhausted reference visit");
}

#[test]
fn empty_product_reference_join_is_free() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let text = super::super::join_product_references([].into_iter(), &ctx, "empty reference fixture")
        .expect("empty source has no visit");
    assert!(text.is_empty());
    ctx.finish_session().expect("no exhausted empty source visit");
}

#[test]
fn reference_list_growth_accepts_exact_visits_and_backing_copy_work() {
    let value = Value::List((1..=8).map(Value::Reference).collect());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Eight visits plus the four existing u64 lanes copied when capacity
    // grows from four to eight at the fifth insertion: 8 + 4 * 8.
    policy.limits.max_work_units = 40;
    policy.limits.max_collection_items = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut references = Vec::new();
    super::super::collect_references(&value, &mut references, &ctx).expect("actual growth fits");
    assert_eq!(references, [1, 2, 3, 4, 5, 6, 7, 8]);
    ctx.finish_session().expect("no exhausted growing list visit");
}

#[test]
fn growing_reference_list_preserves_the_final_visit_refusal() {
    let value = Value::List((1..=8).map(Value::Reference).collect());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 39;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut references = Vec::new();
    let CodecError::ResourceLimit(limit) = super::super::collect_references(&value, &mut references, &ctx)
        .expect_err("the eighth actual visit does not fit") else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "STEP collect references value traversal");
    assert_eq!((limit.used, limit.additional), (39, 1));
    assert_eq!(references, [1, 2, 3, 4, 5, 6, 7]);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}
