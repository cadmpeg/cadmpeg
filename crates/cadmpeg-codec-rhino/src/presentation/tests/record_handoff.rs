// SPDX-License-Identifier: Apache-2.0
//! Successful record backing stays live until aggregate staging is released.

use crate::presentation::push_presentation_record;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ScopedReservation,
};
use cadmpeg_core::CodecError;
use std::cell::Cell;

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
) -> (
    Record<'ctx>,
    ScopedReservation<'ctx>,
    Option<ScopedReservation<'ctx>>,
) {
    let (name, storage) = ctx
        .with_scoped_storage("record fixture backing", || {
            ctx.copy_retained_text("name", "record fixture name")
        })
        .unwrap();
    let (extra_storage, extra) = if extra {
        let (value, storage) = ctx
            .with_scoped_storage("record fixture extra backing", || {
                ctx.copy_retained_text("extra", "record fixture extra")
            })
            .unwrap();
        (Some(storage), value)
    } else {
        (None, String::new())
    };
    (
        Record {
            name,
            extra,
            dropped,
        },
        storage,
        extra_storage,
    )
}
#[test]
fn destination_refusal_drops_the_owned_record_without_storing_leases() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dropped = Cell::new(false);
    let mut staging = ctx.reserve_scoped(0, "record fixture vector").unwrap();
    let mut records = Vec::new();
    let record = record_with_backing(&ctx, &dropped, false);
    let error = push_presentation_record(
        &ctx,
        &mut staging,
        &mut records,
        record,
        "record fixture records",
    )
    .unwrap_err();
    let CodecError::ResourceLimit(ref refusal) = error else {
        panic!("expected collection refusal")
    };
    assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
    assert_eq!(refusal.operation, "record fixture records");
    assert_eq!((refusal.used, refusal.additional, refusal.limit), (0, 1, 0));
    assert!(dropped.get());
    assert!(records.is_empty());
    drop(records);
    drop(staging);
    assert_eq!(
        ctx.finish_session().unwrap_err().to_string(),
        error.to_string()
    );
}
fn successful_insertion(extra: bool) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dropped = Cell::new(false);
    let mut staging = ctx.reserve_scoped(0, "record fixture vector").unwrap();
    let mut records = Vec::new();
    let record = record_with_backing(&ctx, &dropped, extra);
    push_presentation_record(
        &ctx,
        &mut staging,
        &mut records,
        record,
        "record fixture records",
    )
    .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "name");
    assert_eq!(records[0].extra, if extra { "extra" } else { "" });
    assert!(!dropped.get());
    drop(records);
    assert!(dropped.get());
    drop(staging);
    let released = ctx.reserve_scoped(4096, "record backing released").unwrap();
    drop(released);
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
fn record_handoff_holds_the_live_payload_without_reservation_slots() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dropped = Cell::new(false);
    let mut staging = ctx.reserve_scoped(0, "record fixture vector").unwrap();
    let mut records = Vec::new();
    let record = record_with_backing(&ctx, &dropped, true);
    push_presentation_record(
        &ctx,
        &mut staging,
        &mut records,
        record,
        "record fixture records",
    )
    .unwrap();
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_scoped(u64::MAX, "probe live record storage")
        .unwrap_err()
    else {
        panic!("live storage refusal")
    };
    // The vector's actual capacity and the two strings are the only live backing.
    assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(
        refusal.used,
        u64::try_from(records.capacity() * std::mem::size_of::<Record<'_>>() + 4 + 5).unwrap()
    );
    assert!(!dropped.get());
    drop(records);
    drop(staging);
    assert!(dropped.get());
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal)
    );
}
#[test]
fn record_handoff_preserves_the_original_sticky_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 100;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dropped = Cell::new(false);
    let mut staging = ctx.reserve_scoped(0, "record fixture vector").unwrap();
    let mut records = Vec::new();
    let record = record_with_backing(&ctx, &dropped, true);
    let original = ctx
        .charge_work(u64::MAX, "original record handoff refusal")
        .unwrap_err();
    let error = push_presentation_record(
        &ctx,
        &mut staging,
        &mut records,
        record,
        "record fixture records",
    )
    .unwrap_err();
    assert_eq!(error.to_string(), original.to_string());
    assert!(dropped.get());
    assert!(records.is_empty());
    drop(records);
    drop(staging);
    assert_eq!(
        ctx.finish_session().unwrap_err().to_string(),
        original.to_string()
    );
}
