// SPDX-License-Identifier: Apache-2.0
//! Record insertion keeps the owned payload until both backing leases are stored.

use std::cell::Cell;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ScopedReservation};
use cadmpeg_core::CodecError;

use crate::presentation::push_presentation_record;

#[derive(Debug)]
struct Record<'a> {
    name: String,
    extra: String,
    dropped: &'a Cell<bool>,
}

impl Drop for Record<'_> {
    fn drop(&mut self) {
        self.dropped.set(true);
    }
}

fn record_with_backing<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    dropped: &'ctx Cell<bool>,
    extra: bool,
) -> (Record<'ctx>, ScopedReservation<'ctx>, Option<ScopedReservation<'ctx>>) {
    let (storage, name) = ctx.with_scoped_storage("record fixture backing", || {
        ctx.copy_retained_text("name", "record fixture name")
    }).map(|(value, storage)| (storage, value)).unwrap();
    let (extra_storage, extra) = if extra {
        let (value, storage) = ctx.with_scoped_storage("record fixture extra backing", || {
            ctx.copy_retained_text("extra", "record fixture extra")
        }).unwrap();
        (Some(storage), value)
    } else {
        (None, String::new())
    };
    (Record { name, extra, dropped }, storage, extra_storage)
}

fn insertion_refusal(limit: u64, extra: bool, operation: &'static str, stored_guards: usize) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dropped = Cell::new(false);
    let mut staging = ctx.reserve_scoped(0, "record fixture vector").unwrap();
    let mut guard_storage = ctx.reserve_scoped(0, "record fixture lease vector").unwrap();
    let mut guards = Vec::new();
    let mut records = Vec::new();
    let record = record_with_backing(&ctx, &dropped, extra);
    let error = push_presentation_record(&ctx, &mut staging, &mut guard_storage,
        &mut records, &mut guards, record, "record fixture records").unwrap_err();
    let CodecError::ResourceLimit(ref refusal) = error else { panic!("expected collection refusal") };
    assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
    assert_eq!(refusal.operation, operation);
    assert_eq!(refusal.used, limit);
    assert_eq!(refusal.additional, 1);
    assert_eq!(refusal.limit, limit);
    assert!(dropped.get());
    assert!(records.is_empty());
    assert_eq!(guards.len(), stored_guards);
    drop(records);
    drop(guards);
    drop(guard_storage);
    drop(staging);
    assert_eq!(ctx.finish_session().unwrap_err().to_string(), error.to_string());
}

#[test]
fn destination_refusal_drops_the_owned_record_without_storing_leases() {
    insertion_refusal(0, false, "record fixture records", 0);
}

#[test]
fn first_lease_refusal_drops_the_owned_record_before_return() {
    insertion_refusal(1, false, "Rhino presentation record guards", 0);
}

#[test]
fn second_lease_refusal_keeps_the_first_lease_and_drops_the_owned_record() {
    insertion_refusal(2, true, "Rhino presentation record guards", 1);
}

fn successful_insertion(extra: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One record slot and one slot for each actual backing lease.
    policy.limits.max_collection_items = if extra { 3 } else { 2 };
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dropped = Cell::new(false);
    let mut staging = ctx.reserve_scoped(0, "record fixture vector").unwrap();
    let mut guard_storage = ctx.reserve_scoped(0, "record fixture lease vector").unwrap();
    let mut guards = Vec::new();
    let mut records = Vec::new();
    let record = record_with_backing(&ctx, &dropped, extra);
    push_presentation_record(&ctx, &mut staging, &mut guard_storage,
        &mut records, &mut guards, record, "record fixture records").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "name");
    assert_eq!(records[0].extra, if extra { "extra" } else { "" });
    assert_eq!(guards.len(), if extra { 2 } else { 1 });
    assert!(!dropped.get());
    drop(records);
    assert!(dropped.get());
    drop(guards);
    drop(guard_storage);
    drop(staging);
    ctx.finish_session().unwrap();
}

#[test]
fn one_lease_insertion_preserves_the_owned_record_at_the_exact_slot_limit() {
    successful_insertion(false);
}

#[test]
fn two_lease_insertion_preserves_the_owned_record_at_the_exact_slot_limit() {
    successful_insertion(true);
}

#[test]
fn record_handoff_preserves_the_original_sticky_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 100;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dropped = Cell::new(false);
    let mut staging = ctx.reserve_scoped(0, "record fixture vector").unwrap();
    let mut guard_storage = ctx.reserve_scoped(0, "record fixture lease vector").unwrap();
    let mut guards = Vec::new();
    let mut records = Vec::new();
    let record = record_with_backing(&ctx, &dropped, true);
    let original = ctx.charge_work(u64::MAX, "original record handoff refusal").unwrap_err();
    let error = push_presentation_record(&ctx, &mut staging, &mut guard_storage,
        &mut records, &mut guards, record, "record fixture records").unwrap_err();
    assert_eq!(error.to_string(), original.to_string());
    assert!(dropped.get());
    assert!(records.is_empty());
    assert!(guards.is_empty());
    drop(records);
    drop(guards);
    drop(guard_storage);
    drop(staging);
    assert_eq!(ctx.finish_session().unwrap_err().to_string(), original.to_string());
}
