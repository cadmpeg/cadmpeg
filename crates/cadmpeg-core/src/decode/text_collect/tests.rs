// SPDX-License-Identifier: Apache-2.0
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::borrow::Cow;

#[test]
fn string_collection_preserves_character_and_fragment_order() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert_eq!(
        ctx.collect_text(['A', 'é', '🧪'], "chars").expect("text"),
        "Aé🧪"
    );
    assert_eq!(
        ctx.collect_text(["é", "", "λ"], "fragments").expect("text"),
        "éλ"
    );
    assert_eq!(
        ctx.collect_text([String::from("a"), String::from("b")], "owned")
            .expect("text"),
        "ab"
    );
    assert_eq!(
        ctx.collect_text([Cow::Borrowed("x"), Cow::Owned(String::from("y"))], "cow")
            .expect("text"),
        "xy"
    );
    assert_eq!(
        ctx.collect_text([Box::<str>::from("x")], "boxed")
            .expect("text"),
        "x"
    );
    assert_eq!(
        ctx.collect_text(['é'].iter(), "borrowed chars")
            .expect("text"),
        "é"
    );
    assert_eq!(
        ctx.collect_text(std::iter::empty::<char>(), "empty")
            .expect("text"),
        ""
    );
}

#[test]
fn string_collection_refuses_before_source_step_and_preserves_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let calls = std::cell::Cell::new(0);
    let input = std::iter::from_fn(|| {
        calls.set(calls.get() + 1);
        Some('a')
    });
    let CodecError::ResourceLimit(first) = ctx.collect_text(input, "collect").expect_err("refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(calls.get(), 0);
    let CodecError::ResourceLimit(repeated) =
        ctx.collect_text(['b'], "later").expect_err("refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first, repeated);
}

#[test]
fn scoped_string_collection_releases_only_output_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2;
    policy.limits.max_materialized_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let input = std::iter::once_with(|| {
        ctx.copy_retained_text("é", "retained child")
            .expect("child")
    });
    let (text, storage) = ctx
        .collect_scoped_text(input, "scoped result")
        .expect("result");
    assert_eq!(text, "é");
    drop((text, storage));
    ctx.reserve_scoped(2, "released result").expect("released");
    let CodecError::ResourceLimit(limit) = ctx
        .copy_retained_text("x", "child remains")
        .expect_err("retained child stays charged")
    else {
        panic!("resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.used, 2);
}

#[test]
fn string_appends_refuse_before_mutation_in_each_storage_dimension() {
    let arena = DecodeArena::new();
    for scoped in [false, true] {
        let mut policy = DecodePolicy::service();
        if scoped {
            policy.limits.max_materialized_bytes = 1;
        } else {
            policy.limits.max_retained_bytes = 1;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::new();
        let mut storage = ctx.reserve_scoped(0, "scope").expect("empty scope");
        let result = if scoped {
            ctx.push_scoped_char(&mut storage, &mut text, 'é', "append")
        } else {
            ctx.push_retained_char(&mut text, 'é', "append")
        };
        let CodecError::ResourceLimit(first) = result.expect_err("refusal") else {
            panic!("resource refusal")
        };
        assert!(text.is_empty());
        let CodecError::ResourceLimit(repeated) = ctx
            .append_scoped(&mut storage, &mut text, "a", "later")
            .expect_err("sticky refusal")
        else {
            panic!("resource refusal")
        };
        assert_eq!(first, repeated);
        assert_eq!(
            first.dimension,
            if scoped {
                ResourceDimension::MaterializedBytes
            } else {
                ResourceDimension::RetainedBytes
            }
        );
        assert!(text.is_empty());
    }
}

#[test]
fn string_capacity_operations_admit_only_capacity_and_refuse_overflow() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 3;
    policy.limits.max_materialized_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let retained = ctx
        .retained_string(3, "retained capacity")
        .expect("capacity");
    assert!(retained.is_empty());
    assert!(retained.capacity() >= 3);
    let (scoped, reservation) = ctx.scoped_string(3, "scoped capacity").expect("capacity");
    assert!(scoped.is_empty());
    assert!(scoped.capacity() >= 3);
    drop((scoped, reservation));
    ctx.reserve_scoped(3, "released capacity")
        .expect("released");
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert!(matches!(
        ctx.retained_string(usize::MAX, "overflow"),
        Err(CodecError::ResourceLimit(_))
    ));
}
