// SPDX-License-Identifier: Apache-2.0

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::collections::HashSet;

#[test]
fn linear_growth_rejects_full_capacity_above_the_address_limit() {
    use crate::decode::collect::LinearGrowth;
    for growth in [LinearGrowth::Exact, LinearGrowth::Amortized, LinearGrowth::PrechargedBytes] {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
            .expect("context");
        let capacity = usize::try_from(isize::MAX).expect("address limit") / 2;
        let error = ctx.linear_growth::<u16>(capacity, capacity, 1, growth, "addressable growth")
            .err().expect("full capacity exceeds the address limit");
        assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(error.reason, crate::decode::ResourceFailure::AllocationFailed);
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
    assert!(ctx
        .copy_retained_set(&HashSet::<u64>::new(), "empty set copy")
        .expect("admitted test operation")
        .is_empty());
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
            let CodecError::ResourceLimit(first) = growth.expect_err("old buffer refuses") else { panic!("refusal") };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.used, first.additional), (0, 32));
            assert_eq!(values.capacity(), 4);
            assert_eq!(values, [1, 2, 3, 4]);
            let CodecError::ResourceLimit(second) = ctx.charge_work(1, "later").expect_err("fused") else { panic!("refusal") };
            assert_eq!(first, second);
        } else {
            growth.expect("both allocations admitted");
            assert_eq!(values.capacity(), 8);
            let _probe = ctx.reserve_scoped(32, "released overlap").expect("old reservation released");
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
            let CodecError::ResourceLimit(first) = growth.expect_err("overlap refuses") else { panic!("refusal") };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.used, first.additional), (64, 32));
            assert_eq!(values.capacity(), 4);
            assert_eq!(values, [1, 2, 3, 4]);
        } else {
            growth.expect("both allocations admitted");
            assert_eq!(values.capacity(), 8);
            let probe = ctx.reserve_scoped(32, "released overlap").expect("only new buffer remains");
            drop(probe);
            drop(values);
            drop(storage);
            let _probe = ctx.reserve_scoped(96, "all buffers released").expect("storage released");
        }
    }
}

#[test]
fn hash_reallocation_admits_both_bucket_allocations_before_growth() {
    let old = 4 * std::mem::size_of::<u64>() + 15 + 4 + 16;
    let grown = 8 * std::mem::size_of::<u64>() + 15 + 8 + 16;
    for fits in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Both bucket arrays include alignment padding and control bytes.
        policy.limits.max_materialized_bytes = u64::try_from(old + grown - usize::from(!fits)).expect("bound");
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let (mut values, mut storage) = ctx.temporary_set::<u64>(3, "initial").expect("initial");
        values.extend([1, 2, 3]);
        let capacity = values.capacity();
        let growth = storage.with_storage(|| ctx.reserve_set(&mut values, 1, "grow"));
        if fits {
            growth.expect("both tables admitted");
            assert!(values.capacity() > capacity);
            let _probe = ctx.reserve_scoped(u64::try_from(old).expect("bound"), "released overlap").expect("old table released");
        } else {
            let CodecError::ResourceLimit(first) = growth.expect_err("overlap refuses") else { panic!("refusal") };
            assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!((first.used, first.additional), (u64::try_from(grown).expect("bound"), u64::try_from(old).expect("bound")));
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
    let CodecError::ResourceLimit(first) = ctx.try_reserve_retained_text(&mut text, 2, "grow").expect_err("overlap refuses") else { panic!("refusal") };
    assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((first.used, first.additional), (0, 4));
    assert_eq!(text.capacity(), 4);
    assert_eq!(text, "abcd");
}
