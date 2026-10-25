// SPDX-License-Identifier: Apache-2.0
use super::*;
use std::cell::Cell;

#[derive(Debug)]
struct Counted<'a> {
    value: String,
    drops: &'a Cell<usize>,
}

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

std::thread_local! {
    static ZERO_DROPS: Cell<usize> = const { Cell::new(0) };
}

#[derive(Debug)]
struct ZeroSized;

impl Drop for ZeroSized {
    fn drop(&mut self) {
        ZERO_DROPS.with(|drops| drops.set(drops.get() + 1));
    }
}

#[test]
fn nonzero_working_box_preserves_the_value_and_its_single_drop() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let drops = Cell::new(0);
    let value = Counted {
        value: String::from("held non-Copy value"),
        drops: &drops,
    };
    assert!(size_of::<Counted<'_>>() > 0);
    let boxed: Box<Counted<'_>> = ctx.alloc_box_working(value, "working nonzero box").unwrap();
    assert_eq!(boxed.value, "held non-Copy value");
    assert_eq!(drops.get(), 0);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert_eq!(ctx.resource_refusal(), None);
    drop(boxed);
    assert_eq!(drops.get(), 1);
    ctx.finish_session().unwrap();
}

#[test]
fn zero_sized_working_box_keeps_ownership_until_the_box_drops() {
    ZERO_DROPS.with(|drops| drops.set(0));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    assert_eq!(size_of::<ZeroSized>(), 0);
    let boxed: Box<ZeroSized> = ctx
        .alloc_box_working(ZeroSized, "working zero-sized box")
        .unwrap();
    ZERO_DROPS.with(|drops| assert_eq!(drops.get(), 0));
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert_eq!(ctx.resource_refusal(), None);
    drop(boxed);
    ZERO_DROPS.with(|drops| assert_eq!(drops.get(), 1));
    ctx.finish_session().unwrap();
}

#[test]
fn working_boxes_preserve_live_output_entity_and_source_capture_state() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    policy.limits.max_entities = 1;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = crate::encode::EncodeSession::from_context(&ctx).unwrap();
    ctx.retain_bytes(3, "held typed output").unwrap();
    let mut output = session.buffer();
    output.append(b"12", "actual live output").unwrap();
    assert_eq!(ctx.retained_bytes_available(), 3);
    let boxed = ctx
        .alloc_box_working([1_u64, 2, 3], "temporary working box")
        .unwrap();
    assert_eq!(*boxed, [1, 2, 3]);
    assert_eq!(ctx.retained_bytes_available(), 3);
    assert!(ctx.take_record_captures().unwrap().is_empty());
    assert!(matches!(
        ctx.take_record_captures(),
        Err(crate::CodecError::Malformed(_))
    ));
    assert_eq!(ctx.resource_refusal(), None);
    ctx.charge_entities(1, "the actual output record").unwrap();
    drop(boxed);
    assert_eq!(ctx.retained_bytes_available(), 3);
    session.finish().unwrap();
    drop(output);
    assert_eq!(ctx.retained_bytes_available(), 5);
    ctx.finish_session().unwrap();
}

#[test]
fn prior_refusal_reaches_nonzero_and_zero_sized_boxes_without_a_new_account() {
    use crate::decode::{LimitScope, ResourceDimension, ResourceFailure};

    ZERO_DROPS.with(|drops| drops.set(0));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = crate::encode::EncodeSession::from_context(&ctx).unwrap();
    ctx.retain_bytes(3, "held typed output").unwrap();
    let mut output = session.buffer();
    output.append(b"12", "actual live output").unwrap();
    let mut impossible = Vec::<u64>::new();
    let crate::CodecError::ResourceLimit(refusal) = ctx
        .reserve_working(&mut impossible, usize::MAX, "prior working overflow")
        .unwrap_err()
    else {
        panic!("the overflowing request records its actual allocation refusal");
    };
    assert_eq!(refusal.scope, LimitScope::Session);
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.reason, ResourceFailure::AllocationFailed);
    assert_eq!(
        (refusal.limit, refusal.used, refusal.additional),
        (8, 5, u64::MAX)
    );
    assert_eq!(refusal.operation, "prior working overflow");
    assert!(matches!(
        ctx.alloc_box_working([42_u64], "later nonzero box"),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert!(matches!(
        ctx.alloc_box_working(ZeroSized, "later zero-sized box"),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    ZERO_DROPS.with(|drops| assert_eq!(drops.get(), 1));
    assert!(matches!(
        ctx.alloc_box_working((), "later unit box"),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert_eq!(session.finish(), Err(refusal));
    assert!(matches!(
        ctx.finish_session(),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
}
