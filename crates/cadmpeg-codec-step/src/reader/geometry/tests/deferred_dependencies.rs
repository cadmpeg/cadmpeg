// SPDX-License-Identifier: Apache-2.0
//! Deferred registration admits the count slot before the group and member.

use std::collections::VecDeque;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{wake_deferred_dependents, DeferredDependencies};

const CURVE_GROUP: &str = "step_deferred_curve_groups";
const CURVE_MEMBER: &str = "step_deferred_curve_members";
const SURFACE_GROUP: &str = "step_deferred_surface_groups";
const SURFACE_MEMBER: &str = "step_deferred_surface_members";

fn assert_registration_refusal(
    cap: u64,
    group: &'static str,
    member: &'static str,
    operation: &'static str,
) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test deferred owner").expect("empty owner");
    let mut waiting = DeferredDependencies::default();
    let CodecError::ResourceLimit(first) = storage.with_storage(|| {
        waiting.register(&ctx, 1, 2, group, member)
    }).expect_err("registration exceeds admitted slots") else { panic!("resource refusal") };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, operation);
    assert_eq!(first.used, cap);
    assert_eq!(first.additional, 1);
    assert_eq!(first.limit, cap);
    assert!(waiting.waiting_on.is_empty());
    assert!(matches!(ctx.charge_work(0, "later operation"), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
    drop(waiting);
    drop(storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first));
}

#[test]
fn deferred_registration_refuses_first_count_slot() {
    assert_registration_refusal(0, CURVE_GROUP, CURVE_MEMBER, "step deferred dependency count");
}

#[test]
fn deferred_curve_registration_refuses_group_after_count_slot() {
    assert_registration_refusal(1, CURVE_GROUP, CURVE_MEMBER, CURVE_GROUP);
}

#[test]
fn deferred_curve_registration_refuses_member_after_count_and_group_slots() {
    assert_registration_refusal(2, CURVE_GROUP, CURVE_MEMBER, CURVE_MEMBER);
}

#[test]
fn deferred_surface_registration_refuses_group_after_count_slot() {
    assert_registration_refusal(1, SURFACE_GROUP, SURFACE_MEMBER, SURFACE_GROUP);
}

#[test]
fn deferred_surface_registration_refuses_member_after_count_and_group_slots() {
    assert_registration_refusal(2, SURFACE_GROUP, SURFACE_MEMBER, SURFACE_MEMBER);
}

#[test]
fn deferred_registration_accepts_exact_count_group_and_member_slots() {
    for (group, member) in [(CURVE_GROUP, CURVE_MEMBER), (SURFACE_GROUP, SURFACE_MEMBER)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 3;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
        let mut storage = ctx.reserve_scoped(0, "test deferred owner").expect("empty owner");
        let mut waiting = DeferredDependencies::default();
        storage.with_storage(|| waiting.register(&ctx, 1, 2, group, member))
            .expect("count, group and member each require one slot");
        assert_eq!(waiting.remaining.get(&2), Some(&1));
        assert_eq!(waiting.waiting_on.get(&1), Some(&vec![2]));
        drop(waiting);
        drop(storage);
        ctx.finish_session().expect("unrefused session");
    }
}

fn assert_final_wake(second_dependency: u64, cap: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = cap;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut storage = ctx.reserve_scoped(0, "test deferred owner").expect("empty owner");
    let mut waiting = DeferredDependencies::default();
    let mut queue = VecDeque::new();
    storage.with_storage(|| -> Result<(), CodecError> {
        waiting.register(&ctx, 1, 3, CURVE_GROUP, CURVE_MEMBER)?;
        waiting.register(&ctx, second_dependency, 3, CURVE_GROUP, CURVE_MEMBER)?;
        assert_eq!(waiting.remaining.get(&3), Some(&2));
        wake_deferred_dependents(1, &mut waiting, &mut queue, &ctx, "step_deferred_curve_queue")?;
        if second_dependency != 1 {
            assert!(queue.is_empty());
            assert_eq!(waiting.remaining.get(&3), Some(&1));
            wake_deferred_dependents(second_dependency, &mut waiting, &mut queue, &ctx, "step_deferred_curve_queue")?;
        }
        assert_eq!(queue, VecDeque::from([3]));
        assert!(waiting.remaining.is_empty());
        assert!(waiting.waiting_on.is_empty());
        Ok(())
    }).expect("exact producer slots and one final queue slot");
    drop(queue);
    drop(waiting);
    drop(storage);
    ctx.finish_session().expect("unrefused session");
}

#[test]
fn deferred_registration_reuses_count_slot_for_distinct_dependencies() {
    // First edge: three slots. Second edge: group + member. Final wake: one slot.
    assert_final_wake(2, 6);
}

#[test]
fn deferred_registration_reuses_count_and_group_slots_for_repeated_dependency() {
    // First edge: three slots. Repeated edge: member. Final wake: one slot.
    assert_final_wake(1, 5);
}
