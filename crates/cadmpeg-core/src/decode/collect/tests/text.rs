// SPDX-License-Identifier: Apache-2.0

use super::context;
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::{HashMap, HashSet};

const UTF8_CHAR_WIDTHS: [(char, usize); 4] = [('A', 1), ('é', 2), ('€', 3), ('🧪', 4)];

admitted_case!(
    reserve_admitted_vec_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.reserve_capacity(&mut Vec::<u8>::new(), 2, "test admitted")
);
admitted_case!(
    reserve_admitted_map_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.reserve_map(&mut HashMap::<u8, u8>::new(), 2, "test admitted")
);
admitted_case!(
    reserve_admitted_set_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.reserve_set(&mut HashSet::<u8>::new(), 2, "test admitted")
);
admitted_case!(
    copy_admitted_slice_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.copy_slice(&[1_u8, 2], "test admitted").map(|_| ())
);
admitted_case!(
    copy_admitted_rows_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx
        .copy_rows(&[1_u8, 2], 1, "test admitted", "test admitted")
        .map(|_| ())
);
admitted_case!(
    admitted_vec_follows_prior_admission,
    |ctx: &DecodeContext<'_>| ctx.vector_storage::<u8>(2, "test admitted").map(|_| ())
);

#[test]
fn formatted_text_admits_growth_when_display_length_changes() {
    struct ChangingText(std::cell::Cell<bool>);
    impl std::fmt::Display for ChangingText {
        fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            output.write_str(if self.0.replace(true) { "abcd" } else { "a" })
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let changing = ChangingText(std::cell::Cell::new(false));
    let error = ctx
        .format_retained(format_args!("{changing}"), "changing text")
        .expect_err("test operation refuses");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == 1 && limit.additional == 3));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("admitted test operation");
    let changing = ChangingText(std::cell::Cell::new(false));
    assert_eq!(
        ctx.format_retained(format_args!("{changing}"), "changing text")
            .expect("admitted test operation"),
        "abcd"
    );
}

retained_case!(
    copy_retained_slice_charges_before_allocation,
    2,
    |ctx: &DecodeContext<'_>| ctx
        .copy_slice(&[1_u8, 2], "test retained slice")
        .map(|_| ())
);
retained_case!(
    copy_retained_rows_charges_before_allocation,
    crate::decode::u64_from_index(std::mem::size_of::<Vec<u8>>()),
    |ctx: &DecodeContext<'_>| ctx
        .copy_retained_rows(
            &[vec![1_u8]],
            "test retained rows",
            "test retained row items"
        )
        .map(|_| ())
);
retained_case!(
    format_retained_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .format_retained(format_args!("abc"), "test format retained")
        .map(|_| ())
);
retained_case!(
    append_retained_charges_before_growth,
    3,
    |ctx: &DecodeContext<'_>| ctx.append_retained(
        &mut String::new(),
        "abc",
        "test append retained"
    )
);
retained_case!(
    retained_suffix_charges_before_growth,
    2,
    |ctx: &DecodeContext<'_>| ctx.retained_suffix("a", "b", "test suffix").map(|_| ())
);
retained_case!(
    join_retained_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .join_retained(&["a", "b"], "-", "test join retained")
        .map(|_| ())
);
retained_case!(
    join_display_retained_charges_before_growth,
    7,
    |ctx: &DecodeContext<'_>| ctx
        .join_display_retained(["one", "two"], ",", "test display join")
        .map(|_| ())
);
retained_case!(
    retained_string_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx.retained_string(3, "test retained string").map(|_| ())
);
retained_case!(
    copy_retained_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .copy_retained(b"abc", "test optional retained")
        .map(|_| ())
);

materialized_case!(
    temporary_set_reserves_scoped_storage,
    33,
    |ctx: &DecodeContext<'_>| ctx.temporary_set::<u8>(1, "test temporary set").map(|_| ())
);
materialized_case!(
    temporary_queue_reserves_scoped_storage,
    1,
    |ctx: &DecodeContext<'_>| ctx
        .temporary_queue::<u8>(1, "test temporary queue")
        .map(|_| ())
);
materialized_case!(
    format_scoped_charges_before_allocation,
    3,
    |ctx: &DecodeContext<'_>| ctx
        .format_scoped(format_args!("abc"), "test format scoped")
        .map(|_| ())
);

#[test]
fn copy_scoped_text_refuses_before_allocation_and_succeeds_under_service_profile() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test context");
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped text")
        .expect("empty reserve");
    let result = ctx.copy_scoped_text("abc", &mut reservation, "test scoped text");
    assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("test context");
    let mut reservation = ctx
        .reserve_scoped(0, "test scoped text")
        .expect("empty reserve");
    assert_eq!(
        ctx.copy_scoped_text("abc", &mut reservation, "test scoped text")
            .expect("copy"),
        "abc"
    );
}
#[test]
fn charged_join_refuses_input_sized_text_before_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 5;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    assert!(matches!(
        ctx.join_display_retained(["one", "two"], ",", "step_test_join"),
        Err(CodecError::ResourceLimit(refusal))
            if refusal.dimension == ResourceDimension::RetainedBytes
                && refusal.operation == "step_test_join"
    ));
}

#[test]
fn charged_format_refuses_retained_text_before_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 3;
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root fits policy");
    let error = ctx
        .format_retained(
            format_args!("prefix {suffix}", suffix = "input"),
            "step_test_format",
        )
        .expect_err("formatted text exceeds three bytes");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "step_test_format"
    ));
}

#[test]
fn scan_join_measurement_refuses_on_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation is admitted");
    assert!(
        matches!(ctx.join_retained(&[""], "", "join"), Err(CodecError::ResourceLimit(limit)) if limit.dimension == ResourceDimension::WorkUnits)
    );
}
#[test]
fn retained_join_uses_owned_and_borrowed_text_views_without_copying_views() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two length visits, two copy visits, three text bytes and one separator byte.
    policy.limits.max_work_units = 8;
    policy.limits.max_retained_bytes = 4;
    let parts = [String::from("A"), String::from("λ")];
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    assert_eq!(
        ctx.join_retained(&parts, "/", "join").expect("admission"),
        "A/λ"
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let borrowed = [&parts[0], &parts[1]];
    assert_eq!(
        ctx.join_retained(&borrowed, "/", "join")
            .expect("admission"),
        "A/λ"
    );
}

#[test]
fn exact_text_growth_admits_only_added_bytes_and_old_capacity_moves() {
    let arena = DecodeArena::new();
    for (work, retained) in [(3, 2), (4, 1)] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::with_capacity(4);
        text.push_str("abcd");
        let CodecError::ResourceLimit(first) = ctx
            .try_reserve_retained_text(&mut text, 2, "growth")
            .expect_err("refusal")
        else {
            panic!("refusal")
        };
        assert_eq!(text, "abcd");
        assert_eq!(text.capacity(), 4);
        let CodecError::ResourceLimit(second) = ctx.charge_work(1, "later").expect_err("fused")
        else {
            panic!("refusal")
        };
        assert_eq!(first, second);
    }
    let mut policy = DecodePolicy::service();
    // Two added capacity bytes and four old capacity bytes moved by reallocation.
    policy.limits.max_retained_bytes = 2;
    policy.limits.max_work_units = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut text = String::with_capacity(4);
    text.push_str("abcd");
    ctx.try_reserve_retained_text(&mut text, 2, "growth")
        .expect("admission");
    assert!(text.capacity() >= 6);
    assert_eq!(text, "abcd");
}

#[test]
fn push_retained_char_charges_utf8_work_exactly_before_mutation() {
    for (value, length) in UTF8_CHAR_WIDTHS {
        let work = crate::decode::u64_from_index(length);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::with_capacity(length);
        let capacity = text.capacity();
        ctx.push_retained_char(&mut text, value, "retained char")
            .expect("exact UTF-8 work fits");
        assert_eq!(text, value.to_string());
        assert_eq!(text.capacity(), capacity);
        let CodecError::ResourceLimit(limit) = ctx
            .charge_work(1, "work probe")
            .expect_err("the character used its exact UTF-8 work bound")
        else {
            panic!("work refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!((limit.used, limit.additional), (work, 1));

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work - 1;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::with_capacity(length);
        let capacity = text.capacity();
        let CodecError::ResourceLimit(first) = ctx
            .push_retained_char(&mut text, value, "retained char")
            .expect_err("one below UTF-8 work refuses before mutation")
        else {
            panic!("work refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!((first.used, first.additional), (0, work));
        assert!(text.is_empty());
        assert_eq!(text.capacity(), capacity);
        assert_eq!(ctx.resource_refusal(), Some(first));
        let CodecError::ResourceLimit(repeated) = ctx
            .charge_work(0, "later work")
            .expect_err("work refusal is sticky")
        else {
            panic!("work refusal");
        };
        assert_eq!(repeated, first);
    }
}

#[test]
fn push_retained_char_charges_exact_retained_utf8_growth() {
    for (value, length) in UTF8_CHAR_WIDTHS {
        let bytes = crate::decode::u64_from_index(length);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = bytes;
        policy.limits.max_retained_bytes = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::new();
        ctx.push_retained_char(&mut text, value, "retained char")
            .expect("exact retained UTF-8 growth fits");
        assert_eq!(text, value.to_string());
        assert!(text.capacity() >= length);
        let CodecError::ResourceLimit(limit) = ctx
            .charge_retained(1, "retained probe")
            .expect_err("the character used its exact retained byte bound")
        else {
            panic!("retained refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((limit.used, limit.additional), (bytes, 1));

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = bytes;
        policy.limits.max_retained_bytes = bytes - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut text = String::new();
        let capacity = text.capacity();
        let CodecError::ResourceLimit(first) = ctx
            .push_retained_char(&mut text, value, "retained char")
            .expect_err("one below retained UTF-8 growth refuses before allocation")
        else {
            panic!("retained refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((first.used, first.additional), (0, bytes));
        assert!(text.is_empty());
        assert_eq!(text.capacity(), capacity);
        assert_eq!(ctx.resource_refusal(), Some(first));
        let CodecError::ResourceLimit(repeated) = ctx
            .charge_work(0, "later work")
            .expect_err("retained refusal is sticky")
        else {
            panic!("retained refusal");
        };
        assert_eq!(repeated, first);
    }
}

#[test]
fn push_retained_char_charges_and_releases_scoped_utf8_storage() {
    for (value, length) in UTF8_CHAR_WIDTHS {
        let bytes = crate::decode::u64_from_index(length);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = bytes;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = bytes;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut reservation = ctx.reserve_scoped(0, "scoped char").expect("scope");
        let mut text = String::new();
        reservation
            .with_storage(|| ctx.push_retained_char(&mut text, value, "retained char"))
            .expect("exact scoped UTF-8 storage fits");
        assert_eq!(text, value.to_string());
        assert!(text.capacity() >= length);
        drop(text);
        drop(reservation);
        let _released = ctx
            .reserve_scoped(bytes, "released scoped char")
            .expect("scoped character storage is released");

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = bytes;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = bytes - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut reservation = ctx.reserve_scoped(0, "scoped char").expect("scope");
        let mut text = String::new();
        let capacity = text.capacity();
        let CodecError::ResourceLimit(first) = reservation
            .with_storage(|| ctx.push_retained_char(&mut text, value, "retained char"))
            .expect_err("one below scoped UTF-8 storage refuses before allocation")
        else {
            panic!("scoped storage refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!((first.used, first.additional), (0, bytes));
        assert!(text.is_empty());
        assert_eq!(text.capacity(), capacity);
        assert_eq!(ctx.resource_refusal(), Some(first));
        let CodecError::ResourceLimit(repeated) = ctx
            .charge_work(0, "later work")
            .expect_err("scoped refusal is sticky")
        else {
            panic!("scoped storage refusal");
        };
        assert_eq!(repeated, first);
    }
}

#[test]
fn push_retained_char_scoped_reallocation_admits_exact_peak_and_refuses_one_below() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 12;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 12;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut reservation = ctx.reserve_scoped(0, "scoped char").expect("scope");
    let mut text = reservation
        .with_storage(|| ctx.copy_retained_text("abcd", "char prefix"))
        .expect("four-byte scoped prefix");
    assert_eq!(text.capacity(), 4);
    reservation
        .with_storage(|| ctx.push_retained_char(&mut text, '🧪', "retained char"))
        .expect("four bytes plus the old and new buffers fit");
    assert_eq!(text, "abcd🧪");
    assert!(text.capacity() >= 8);
    let overlap_probe = ctx
        .reserve_scoped(4, "verify old-buffer release")
        .expect("the four-byte old-buffer overlap was released");
    drop(overlap_probe);
    drop(text);
    drop(reservation);
    let released = ctx
        .reserve_scoped(12, "verify scoped reallocation release")
        .expect("scoped output storage was released");
    drop(released);
    let CodecError::ResourceLimit(work) = ctx
        .charge_work(1, "verify char and move work")
        .expect_err("four char bytes, four copied bytes and four move bytes were charged")
    else {
        panic!("work refusal");
    };
    assert_eq!(work.dimension, ResourceDimension::WorkUnits);
    assert_eq!((work.used, work.additional), (12, 1));

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 12;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 11;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut reservation = ctx.reserve_scoped(0, "scoped char").expect("scope");
    let mut text = reservation
        .with_storage(|| ctx.copy_retained_text("abcd", "char prefix"))
        .expect("four-byte scoped prefix");
    let capacity = text.capacity();
    let CodecError::ResourceLimit(first) = reservation
        .with_storage(|| ctx.push_retained_char(&mut text, '🧪', "retained char"))
        .expect_err("one below the old and new buffer peak refuses")
    else {
        panic!("scoped storage refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((first.used, first.additional), (8, 4));
    assert_eq!(text, "abcd");
    assert_eq!(text.capacity(), capacity);
    assert_eq!(ctx.resource_refusal(), Some(first));
    let CodecError::ResourceLimit(repeated) = ctx
        .charge_work(0, "later work")
        .expect_err("scoped growth refusal is sticky")
    else {
        panic!("scoped storage refusal");
    };
    assert_eq!(repeated, first);
}
