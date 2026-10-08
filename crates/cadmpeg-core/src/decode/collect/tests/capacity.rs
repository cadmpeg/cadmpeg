// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::HashSet;

#[test]
fn linear_growth_rejects_full_capacity_above_the_address_limit() {
    use crate::decode::collect::LinearGrowth;
    for growth in [
        LinearGrowth::Exact,
        LinearGrowth::Amortized,
        LinearGrowth::PrechargedBytes,
    ] {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let capacity = usize::try_from(isize::MAX).expect("address limit") / 2;
        let error = ctx
            .linear_growth::<u16>(capacity, capacity, 1, growth, "addressable growth")
            .expect_err("full capacity exceeds the address limit");
        assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(
            error.reason,
            crate::decode::ResourceFailure::AllocationFailed
        );
        assert_eq!(error.used, 0);
        assert!(error.additional > u64::try_from(isize::MAX).expect("address limit"));
        assert_eq!(ctx.resource_refusal(), Some(error));
    }
}

#[test]
fn reserve_vec_charges_minimum_capacity_and_added_growth_bytes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 64;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test context");
    let mut values = Vec::<u64>::new();
    ctx.reserve_vec(&mut values, 1, "minimum vector capacity")
        .expect("four slots");
    assert_eq!(values.capacity(), 4);
    ctx.charge_retained(0, "check minimum charge")
        .expect("32 bytes remain");
    values.extend([1, 2, 3, 4]);
    ctx.reserve_vec(&mut values, 1, "grown vector capacity")
        .expect("eight slots");
    assert_eq!(values.capacity(), 8);
    let error = ctx
        .charge_retained(1, "check exact growth charge")
        .expect_err("64 bytes used");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 64 && limit.additional == 1));
}

#[test]
fn reserve_vec_refuses_retained_limit_before_allocation_and_fuses() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 31;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test context");
    let mut values = Vec::<u64>::new();
    let error = ctx
        .reserve_vec(&mut values, 1, "minimum vector capacity")
        .expect_err("four slots need 32 bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes && limit.used == 0 && limit.additional == 32
                && ctx.resource_refusal() == Some(limit)));
    assert_eq!(values.capacity(), 0);
    assert!(values.is_empty());
}

#[test]
fn retained_capacity_reservation_and_push_charge_storage_and_item_once() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        4 * u64::try_from(std::mem::size_of::<u64>()).expect("admitted test operation");
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let mut values = Vec::new();
    ctx.reserve_capacity_limit(&mut values, 1, "reserve retained capacity")
        .expect("admitted test operation");
    assert_eq!(values.capacity(), 4);
    ctx.push_vec(&mut values, 7u64, "admit retained item")
        .expect("admitted test operation");
    assert_eq!(values, [7]);
    let error = ctx
        .push_vec(&mut values, 8u64, "refuse second item")
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::CollectionItems && limit.used == 1 && limit.additional == 1)
    );
    assert_eq!(values, [7]);
}

#[test]
fn flat_copies_do_not_charge_storage_for_empty_or_zero_sized_values() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    assert_eq!(
        ctx.copy_slice(&[(); 3], "zero sized copy")
            .expect("admitted test operation"),
        vec![(); 3]
    );
    assert!(ctx.finish_session().is_ok());
}

#[test]
fn retained_vector_size_overflow_fuses_in_the_storage_dimension() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let mut values = Vec::<u64>::new();
    let error = ctx
        .reserve_vec(&mut values, usize::MAX, "oversized retained vector")
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes)
    );
    assert!(values.is_empty());
    assert!(ctx.finish_session().is_err());
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("admitted test operation");
    let error = ctx
        .with_scoped_storage("oversized scoped vector", || {
            ctx.reserve_vec(&mut values, usize::MAX, "oversized scoped vector")
        })
        .expect_err("test operation refuses");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes)
    );
    assert!(ctx.finish_session().is_err());
}

#[test]
fn retained_reallocation_admits_old_buffer_before_growth_and_releases_it() {
    for limit in [31, 32] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Eight retained u64 slots and four temporary old-buffer slots.
        policy.limits.max_retained_bytes = 64;
        policy.limits.max_materialized_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut values = ctx.collection_vec::<u64>(4, "initial").expect("initial");
        values.extend([1, 2, 3, 4]);
        let growth = ctx.reserve_capacity(&mut values, 1, "grow");
        if limit == 31 {
            let CodecError::ResourceLimit(first) = growth.expect_err("old buffer refuses") else {
                panic!("refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.used, first.additional), (0, 32));
            assert_eq!(values.capacity(), 4);
            assert_eq!(values, [1, 2, 3, 4]);
            let CodecError::ResourceLimit(second) = ctx.charge_work(1, "later").expect_err("fused")
            else {
                panic!("refusal")
            };
            assert_eq!(first, second);
        } else {
            growth.expect("both allocations admitted");
            assert_eq!(values.capacity(), 8);
            let _probe = ctx
                .reserve_scoped(32, "released overlap")
                .expect("old reservation released");
        }
    }
}

#[test]
fn scoped_reallocation_admits_old_and_new_buffers_together() {
    for limit in [95, 96] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Eight new u64 slots overlap four old slots during reallocation.
        policy.limits.max_materialized_bytes = limit;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (mut values, mut storage) = ctx.temporary_vec::<u64>(4, "initial").expect("initial");
        values.extend([1, 2, 3, 4]);
        let growth = storage.with_storage(|| ctx.reserve_capacity(&mut values, 1, "grow"));
        if limit == 95 {
            let CodecError::ResourceLimit(first) = growth.expect_err("overlap refuses") else {
                panic!("refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.used, first.additional), (64, 32));
            assert_eq!(values.capacity(), 4);
            assert_eq!(values, [1, 2, 3, 4]);
        } else {
            growth.expect("both allocations admitted");
            assert_eq!(values.capacity(), 8);
            let probe = ctx
                .reserve_scoped(32, "released overlap")
                .expect("only new buffer remains");
            drop(probe);
            drop(values);
            drop(storage);
            let _probe = ctx
                .reserve_scoped(96, "all buffers released")
                .expect("storage released");
        }
    }
}

#[test]
fn hash_reallocation_admits_both_bucket_allocations_before_growth() {
    let bytes = |buckets: usize| buckets * std::mem::size_of::<u64>() + 15 + buckets + 16;
    // A set sized for three retains four buckets. Growing it to four holds
    // storage for eight, sixteen buckets, beside the old table while the new
    // one is allocated, then retains eight buckets in all.
    let admitted = bytes(4);
    let bound = bytes(16);
    let grown = bytes(8);
    for fits in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes =
            u64::try_from(admitted + bound - usize::from(!fits)).expect("bound");
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (mut values, mut storage) = ctx.temporary_set::<u64>(3, "initial").expect("initial");
        values.extend([1, 2, 3]);
        let capacity = values.capacity();
        let growth = storage.with_storage(|| ctx.reserve_set(&mut values, 1, "grow"));
        if fits {
            growth.expect("both tables admitted");
            assert!(values.capacity() > capacity);
            let _probe = ctx
                .reserve_scoped(
                    u64::try_from(admitted + bound - grown).expect("bound"),
                    "released bound",
                )
                .expect("growth bound released");
        } else {
            let CodecError::ResourceLimit(first) = growth.expect_err("bound refuses") else {
                panic!("refusal")
            };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(
                (first.used, first.additional),
                (
                    u64::try_from(admitted).expect("bound"),
                    u64::try_from(bound).expect("bound")
                )
            );
            assert_eq!(values.capacity(), capacity);
            assert_eq!(values, HashSet::from([1, 2, 3]));
        }
    }
}

#[test]
fn text_reallocation_refuses_before_old_buffer_overlap() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two retained growth bytes require four temporary old capacity bytes.
    policy.limits.max_retained_bytes = 2;
    policy.limits.max_materialized_bytes = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut text = String::with_capacity(4);
    text.push_str("abcd");
    let CodecError::ResourceLimit(first) = ctx
        .try_reserve_retained_text(&mut text, 2, "grow")
        .expect_err("overlap refuses")
    else {
        panic!("refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((first.used, first.additional), (0, 4));
    assert_eq!(text.capacity(), 4);
    assert_eq!(text, "abcd");
}

#[test]
fn hash_growth_after_removals_admits_the_real_allocation() {
    let bytes = |buckets: usize| buckets * std::mem::size_of::<u64>() + 15 + buckets + 16;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut values = HashSet::<u64>::new();
    for value in 0..14_336 {
        ctx.insert_hash_set(&mut values, value, "fill")
            .expect("fill");
    }
    for value in 0..14_300 {
        assert!(ctx
            .remove_hash_set(&mut values, &value, "remove")
            .expect("remove"));
    }
    // Insertions into deleted slots leave capacity() at the entry count while
    // the table keeps its 16384 buckets.
    let mut next = 1_000_000;
    while values.capacity() > values.len() {
        ctx.insert_hash_set(&mut values, next, "refill")
            .expect("refill");
        next += 1;
    }
    assert!(values.len() > 14_336 / 2);
    let before = ctx.budget.retained_used();
    ctx.insert_hash_set(&mut values, next, "grow")
        .expect("grow");
    // Hashbrown resized from the real capacity to 32768 buckets; this growth
    // alone admits the new table's storage beyond the 16384-bucket storage of
    // the old counted capacity, which earlier growth already admitted.
    assert_eq!(values.capacity(), 28_672);
    assert_eq!(
        ctx.budget.retained_used() - before,
        u64::try_from(bytes(32_768) - bytes(16_384)).expect("bound")
    );
    assert!(ctx.budget.retained_used() >= u64::try_from(bytes(32_768)).expect("bound"));
}

#[test]
fn churned_hash_table_retains_at_most_one_bucket_ratio_per_removal() {
    let bytes = |buckets: usize| buckets * std::mem::size_of::<u64>() + 15 + buckets + 16;
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut values = HashSet::<u64>::new();
    let window = 14_000;
    for value in 0..window {
        ctx.insert_hash_set(&mut values, value, "fill")
            .expect("fill");
    }
    let filled = ctx.budget.retained_used();
    assert_eq!(filled, u64::try_from(bytes(16_384)).expect("bound"));
    // A sliding window: every step inserts one new key and removes the oldest.
    let removals = 200_000;
    for step in 0..removals {
        ctx.insert_hash_set(&mut values, window + step, "slide")
            .expect("slide");
        assert!(ctx
            .remove_hash_set(&mut values, &step, "slide")
            .expect("slide"));
    }
    assert_eq!(values.len(), usize::try_from(window).expect("bound"));
    let retained = ctx.budget.retained_used();
    let real = u64::try_from(bytes(values.capacity() * 8 / 7)).expect("bound");
    // Growth after removals cannot tell an in-place rehash from a doubling,
    // so it charges as if the table doubled: more than the table holds, but
    // at most 8/7 of a bucket (entry plus control byte) per removal.
    assert!(retained > real);
    let per_removal =
        (8 * (u64::try_from(std::mem::size_of::<u64>()).expect("bound") + 1)).div_ceil(7);
    assert!(retained <= real + removals * per_removal);
}
