// SPDX-License-Identifier: Apache-2.0
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;

#[test]
fn vector_extension_accepts_owned_optional_array_and_borrowed_sources() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut values = vec![0_u8];
    ctx.extend_vec(&mut values, vec![1], "owned")
        .expect("owned");
    ctx.extend_vec(&mut values, Some(2), "optional")
        .expect("optional");
    ctx.extend_vec(&mut values, None::<u8>, "none")
        .expect("none");
    ctx.extend_vec(&mut values, [3, 4], "array").expect("array");
    ctx.extend_vec(&mut values, &[5, 6][..], "slice")
        .expect("slice");
    ctx.extend_vec(&mut values, &[7], "borrowed array")
        .expect("array");
    ctx.extend_vec(&mut values, &vec![8], "borrowed vector")
        .expect("vector");
    assert_eq!(values, [0, 1, 2, 3, 4, 5, 6, 7, 8]);
    let child = String::from("owned child");
    let pointer = child.as_ptr();
    let mut owned = Vec::new();
    ctx.extend_vec(&mut owned, Some(child), "child move")
        .expect("move");
    assert_eq!(owned[0].as_ptr(), pointer);
}

#[test]
fn vector_extension_refuses_before_mutation_and_preserves_refusal() {
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::CollectionItems,
        ResourceDimension::RetainedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut target = vec![1_u8];
        let CodecError::ResourceLimit(first) = ctx
            .extend_vec(&mut target, Some(2), "extend")
            .expect_err("refusal")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first.dimension, dimension);
        assert_eq!(target, [1]);
        let CodecError::ResourceLimit(repeated) = ctx
            .extend_vec(&mut target, [3], "later")
            .expect_err("original refusal")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
        assert_eq!(target, [1]);
    }
}

#[test]
fn fallible_scoped_vector_keeps_children_retained_and_releases_slots() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    let slot_bytes = u64::try_from(4 * std::mem::size_of::<String>()).expect("slot bytes");
    policy.limits.max_materialized_bytes = slot_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let input = std::iter::once_with(|| ctx.copy_retained_text("é", "child"));
    let (values, storage) = ctx
        .try_collect_scoped_vec(input, "temporary slots")
        .expect("temporary collection");
    assert_eq!(values, ["é"]);
    drop((values, storage));
    ctx.reserve_scoped(slot_bytes, "released slots")
        .expect("released");
    let CodecError::ResourceLimit(limit) = ctx
        .copy_retained_text("x", "retained child")
        .expect_err("child stays charged")
    else {
        panic!("resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, 2);
}

#[test]
fn fallible_scoped_vector_preserves_child_refusal_and_stops_source() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let calls = std::cell::Cell::new(0);
    let input = std::iter::from_fn(|| {
        calls.set(calls.get() + 1);
        Some(Err::<String, _>(ctx.refuse_codec_limit("child", 0, 1)))
    });
    let CodecError::ResourceLimit(first) = ctx
        .try_collect_scoped_vec(input, "slots")
        .expect_err("child refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(calls.get(), 1);
    assert_eq!(first.operation, "child");
    assert_eq!(ctx.resource_refusal(), Some(first));
}

#[test]
fn fallible_scoped_vector_refuses_before_source_step() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let calls = std::cell::Cell::new(0);
    let input = std::iter::from_fn(|| {
        calls.set(calls.get() + 1);
        Some(Ok::<u8, CodecError>(1))
    });
    assert!(matches!(
        ctx.try_collect_scoped_vec(input, "slots"),
        Err(CodecError::ResourceLimit(_))
    ));
    assert_eq!(calls.get(), 0);
}
