// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn working_text_reservation_preserves_bytes_without_retained_output_charge() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let mut text = String::from("held");
    ctx.reserve_text_working(&mut text, 64, "working text control")
        .expect("working storage is not retained output");
    assert_eq!(text, "held");
    assert!(text.capacity() >= 68);
    assert_eq!(ctx.retained_bytes_available(), 0);
    ctx.finish_session()
        .expect("no retained bytes were charged");
}

#[test]
fn overflowing_text_reservation_latches_the_same_failure_for_all_later_work() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let mut text = String::from("held");
    assert!(matches!(
        ctx.reserve_text_working(&mut text, usize::MAX, "text overflow control"),
        Err(crate::CodecError::ResourceLimit(_))
    ));
    assert_eq!(text, "held");
    let refusal = ctx
        .resource_refusal()
        .expect("actual allocator refusal is fused");
    assert!(
        matches!(ctx.reserve_text_working(&mut text,0,"later text"),Err(crate::CodecError::ResourceLimit(error)) if error == refusal)
    );
    let mut working = Vec::<u8>::new();
    assert!(
        matches!(ctx.reserve_working(&mut working,0,"later vector"),Err(crate::CodecError::ResourceLimit(error)) if error == refusal)
    );
    assert!(
        matches!(crate::encode::EncodeSession::from_context(&ctx),Err(error) if error == refusal)
    );
    assert!(
        matches!(ctx.finish_session(),Err(crate::CodecError::ResourceLimit(error)) if error == refusal)
    );
}

#[test]
fn text_reservation_observes_prior_vector_allocation_failure_even_when_empty() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let mut working = Vec::<u64>::new();
    assert!(ctx
        .reserve_working(&mut working, usize::MAX, "prior vector overflow")
        .is_err());
    let refusal = ctx.resource_refusal().expect("prior allocation refusal");
    let mut text = String::new();
    assert!(
        matches!(ctx.reserve_text_working(&mut text,0,"empty text"),Err(crate::CodecError::ResourceLimit(error)) if error == refusal)
    );
    assert!(text.is_empty());
}

#[test]
fn encode_working_text_preserves_the_live_output_account_and_entity_slot() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    policy.limits.max_entities = 1;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = crate::encode::EncodeSession::from_context(&ctx)
        .expect("encode session opens from a fresh context");
    ctx.retain_bytes(3, "held typed output")
        .expect("three retained bytes fit the eight-byte ceiling");
    let mut output = session.buffer();
    output
        .append(b"12", "actual live output")
        .expect("two output bytes fit the remaining ceiling");
    assert_eq!(ctx.retained_bytes_available(), 3);
    let mut text = String::from("held");
    ctx.reserve_text_working(&mut text, 64, "temporary encode text")
        .expect("working text needs no retained allowance");
    assert_eq!(text, "held");
    assert!(text.capacity() >= 68);
    assert_eq!(ctx.retained_bytes_available(), 3);
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(ctx.retained_bytes_available(), 3);
    ctx.charge_entities(1, "the actual output record")
        .expect("one entity fits the ceiling");
    session
        .finish()
        .expect("the session finishes without a refusal");
    drop(output);
    assert_eq!(ctx.retained_bytes_available(), 5);
    ctx.finish_session()
        .expect("the session finishes without a refusal");
}

#[test]
fn encode_text_allocation_failure_records_absolute_usage_and_fuses_both_helpers() {
    use crate::decode::{LimitScope, ResourceDimension, ResourceFailure};

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = crate::encode::EncodeSession::from_context(&ctx)
        .expect("encode session opens from a fresh context");
    ctx.retain_bytes(3, "held typed output")
        .expect("three retained bytes fit the eight-byte ceiling");
    let mut output = session.buffer();
    output
        .append(b"12", "actual live output")
        .expect("two output bytes fit the remaining ceiling");
    let mut text = String::from("held");
    let crate::CodecError::ResourceLimit(refusal) = ctx
        .reserve_text_working(&mut text, usize::MAX, "actual encode text overflow")
        .expect_err("a usize::MAX text reservation is refused")
    else {
        panic!("the fallible reservation latches an allocation refusal");
    };
    assert_eq!(refusal.scope, LimitScope::Session);
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.reason, ResourceFailure::AllocationFailed);
    assert_eq!(
        (refusal.limit, refusal.used, refusal.additional),
        (8, 5, crate::decode::u64_from_index(usize::MAX))
    );
    assert_eq!(refusal.operation, "actual encode text overflow");
    assert_eq!(text, "held");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
    assert!(matches!(
        ctx.reserve_text_working(&mut text, 0, "later text"),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    let mut vector = Vec::<u8>::new();
    assert!(matches!(
        ctx.reserve_working(&mut vector, 0, "later vector"),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    drop(output);
    assert_eq!(ctx.retained_bytes_available(), 0);
    assert!(matches!(
        ctx.finish_session(),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
}

#[test]
fn encode_text_observes_an_entity_refusal_before_even_an_empty_reservation() {
    use crate::decode::ResourceDimension;

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = crate::encode::EncodeSession::from_context(&ctx)
        .expect("encode session opens from a fresh context");
    let crate::CodecError::ResourceLimit(refusal) = ctx
        .charge_entities(1, "actual emitted record")
        .expect_err("one entity exceeds a zero-entity ceiling")
    else {
        panic!("the caller's entity ceiling refuses its actual record");
    };
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (0, 0, 1));
    let mut text = String::from("held");
    let capacity = text.capacity();
    assert!(matches!(
        ctx.reserve_text_working(&mut text, 0, "text after entity refusal"),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal
    ));
    assert_eq!(text, "held");
    assert_eq!(text.capacity(), capacity);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
}
