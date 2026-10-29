// SPDX-License-Identifier: Apache-2.0
//! Real caller limits govern retained bytes, lifetime, and emitted records.

use super::EncodeSession;
use crate::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimits};

fn session_with_limits(limits: ResourceLimits) -> EncodeSession {
    let arena = crate::decode::DecodeArena::new();
    let policy = crate::decode::DecodePolicy {
        limits,
        ..crate::decode::DecodePolicy::service()
    };
    let ctx = crate::decode::DecodeContext::for_encode(&arena, &policy);
    EncodeSession::from_context(&ctx).expect("actual caller account")
}

#[test]
fn simultaneous_buffers_share_the_exact_live_byte_ceiling_and_fuse_refusal() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 5;
    let session = session_with_limits(limits);
    let mut first = session.buffer();
    let mut second = session.buffer();
    first
        .append(b"abc", "first retained record")
        .expect("three bytes");
    second
        .append(b"de", "second retained record")
        .expect("two remaining bytes");
    assert_eq!(first.as_bytes(), b"abc");
    assert_eq!(second.as_bytes(), b"de");
    let error = first
        .append(b"f", "next retained record")
        .expect_err("shared ceiling");
    assert_eq!(error.scope, LimitScope::Session);
    assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(error.reason, ResourceFailure::BudgetExceeded);
    assert_eq!((error.limit, error.used, error.additional), (5, 5, 1));
    assert_eq!(error.operation, "next retained record");
    drop(second);
    assert_eq!(
        session
            .finish()
            .expect_err("drop cannot clear a fused refusal"),
        error
    );
    assert_eq!(
        first
            .append(b"", "later caller")
            .expect_err("no swallowed failure"),
        error
    );
    assert_eq!(first.as_bytes(), b"abc");
}

#[test]
fn dropping_a_buffer_releases_only_its_actual_live_bytes() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 5;
    let session = session_with_limits(limits);
    {
        let mut first = session.buffer();
        first
            .append(b"three", "temporary record")
            .expect("exact ceiling");
    }
    let mut second = session.buffer();
    second
        .append(b"again", "replacement record")
        .expect("temporary bytes were released");
    assert_eq!(second.as_bytes(), b"again");
    session.finish().expect("actual live byte allowance");
}

#[test]
fn entity_charges_are_cumulative_and_do_not_follow_buffer_lifetime() {
    let mut limits = ResourceLimits::service();
    limits.max_entities = 2;
    let session = session_with_limits(limits);
    session
        .charge_entities(1, "emit first record")
        .expect("first record");
    {
        let buffer = session.buffer();
        assert!(buffer.is_empty());
    }
    session
        .charge_entities(1, "emit second record")
        .expect("second record");
    let error = session
        .charge_entities(1, "emit third record")
        .expect_err("records remain charged");
    assert_eq!(error.dimension, ResourceDimension::Entities);
    assert_eq!((error.limit, error.used, error.additional), (2, 2, 1));
    assert_eq!(error.operation, "emit third record");
}

#[test]
fn an_in_place_length_patch_cannot_grow_or_capture_a_missing_range() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 4;
    let session = session_with_limits(limits);
    let mut buffer = session.buffer();
    buffer
        .append(b"head", "four byte header")
        .expect("exact retained ceiling");
    assert!(buffer.overwrite(1, b"XY"));
    assert_eq!(buffer.as_bytes(), b"hXYd");
    assert!(!buffer.overwrite(3, b"XX"));
    assert!(!buffer.overwrite(usize::MAX, b"XX"));
    assert_eq!(buffer.as_bytes(), b"hXYd");
    assert_eq!(buffer.len(), 4);
    session
        .finish()
        .expect("overwriting is not a retained allocation");
}

#[test]
fn an_in_place_rotation_keeps_the_length_and_the_charge() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 6;
    let session = session_with_limits(limits);
    let mut buffer = session.buffer();
    buffer
        .append(b"bodyhd", "body then header")
        .expect("exact retained ceiling");
    assert!(buffer.rotate_tail_to_front(4));
    assert_eq!(buffer.as_bytes(), b"hdbody");
    assert!(!buffer.rotate_tail_to_front(7));
    assert_eq!(buffer.as_bytes(), b"hdbody");
    assert!(buffer.rotate_tail_to_front(6));
    assert_eq!(buffer.as_bytes(), b"hdbody");
    let error = buffer
        .append(b"x", "after rotation")
        .expect_err("rotation is not a retained allocation");
    assert_eq!((error.limit, error.used, error.additional), (6, 6, 1));
}

#[test]
fn handing_over_the_bytes_refunds_their_charge() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 5;
    let session = session_with_limits(limits);
    let mut first = session.buffer();
    first
        .append(b"final", "final output")
        .expect("exact ceiling");
    assert_eq!(first.into_vec(), b"final");
    let mut second = session.buffer();
    second
        .append(b"again", "next output")
        .expect("handed-over bytes are no longer charged");
    session.finish().expect("no refusal");
}

#[test]
fn the_largest_caller_entity_ceiling_does_not_wrap() {
    let mut limits = ResourceLimits::service();
    limits.max_entities = u64::MAX;
    let session = session_with_limits(limits);
    session
        .charge_entities(u64::MAX, "emit a full counter")
        .expect("exact caller ceiling");
    let error = session
        .charge_entities(1, "next actual record")
        .expect_err("counter does not wrap");
    assert_eq!(
        (error.limit, error.used, error.additional),
        (u64::MAX, u64::MAX, 1)
    );
}

#[test]
fn geometric_growth_charges_actual_backing_capacity_instead_of_only_written_bytes() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 64;
    let session = session_with_limits(limits);
    let mut first = session.buffer();
    first
        .append(b"abc", "first bytes")
        .expect("initial allocation");
    first
        .append(b"d", "next bytes")
        .expect("geometric capacity growth");
    assert_eq!(first.len(), 4);
    assert!(first.capacity() >= 6);
    assert_eq!(first.capacity(), first.bytes.capacity());
    assert_eq!(
        session.account.retained_used(),
        crate::decode::u64_from_index(first.capacity())
    );
    let mut second = session.buffer();
    second
        .append(b"ef", "second backing allocation")
        .expect("shared capacity");
    assert_eq!(
        session.account.retained_used(),
        crate::decode::u64_from_index(first.capacity() + second.capacity())
    );
    drop(first);
    assert_eq!(
        session.account.retained_used(),
        crate::decode::u64_from_index(second.capacity())
    );
}

#[test]
fn excess_actual_capacity_is_released_before_a_fused_refusal_returns() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 10;
    let session = session_with_limits(limits);
    let mut unrelated = session.buffer();
    unrelated
        .append(b"keep", "other live record")
        .expect("four bytes");
    let mut buffer = session.buffer();
    buffer
        .append(b"abc", "initial record")
        .expect("three bytes");
    let previous_charge = buffer.capacity();
    let other_charge = unrelated.capacity();
    assert_eq!(
        session.account.retained_used(),
        crate::decode::u64_from_index(previous_charge + other_charge)
    );

    // Vec permits capacity above the requested minimum. Reserve real backing
    // storage at the private post-reservation seam to exercise that outcome
    // deterministically, without constructing or injecting a resource finding.
    buffer
        .bytes
        .try_reserve_exact(5)
        .expect("small real allocation");
    let actual_capacity = buffer.bytes.capacity();
    assert!(actual_capacity >= 8);
    let error = buffer
        .admit_reserved_capacity("actual reserved record", 0)
        .expect_err("the actual backing capacity exceeds the shared allowance");
    assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(error.reason, ResourceFailure::BudgetExceeded);
    assert_eq!(error.limit, 10);
    assert_eq!(
        error.used,
        crate::decode::u64_from_index(previous_charge + other_charge)
    );
    assert_eq!(
        error.additional,
        crate::decode::u64_from_index(actual_capacity - previous_charge)
    );
    assert_eq!(error.operation, "actual reserved record");
    assert!(buffer.is_empty());
    assert_eq!(buffer.capacity(), 0);
    assert_eq!(buffer.bytes.capacity(), 0);
    assert_eq!(
        session.account.retained_used(),
        crate::decode::u64_from_index(other_charge)
    );
    assert_eq!(unrelated.as_bytes(), b"keep");
    assert_eq!(session.finish().expect_err("fatal fused session"), error);
    assert_eq!(
        unrelated
            .append(b"x", "later record")
            .expect_err("no later emission"),
        error,
    );
    drop(buffer);
    assert_eq!(
        session.account.retained_used(),
        crate::decode::u64_from_index(other_charge)
    );
    drop(unrelated);
    assert_eq!(session.account.retained_used(), 0);
}

#[test]
fn context_and_output_share_eight_bytes_in_both_allocation_orders() {
    use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    for native_first in [true, false] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 8;
        let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
        let session = EncodeSession::from_context(&ctx).expect("one caller account");
        let mut output = session.buffer();
        let mut native = None;
        let refusal = if native_first {
            ctx.retain_bytes(6, "retain six native bytes")
                .expect("native fits");
            native = Some(Box::new(*b"native"));
            output
                .append(b"output", "retain six output bytes")
                .expect_err("aggregate exceeds eight")
        } else {
            output
                .append(b"output", "retain six output bytes")
                .expect("output fits");
            let crate::CodecError::ResourceLimit(refusal) = ctx
                .retain_bytes(6, "retain six native bytes")
                .expect_err("aggregate exceeds eight")
            else {
                panic!("resource refusal")
            };
            refusal
        };
        assert_eq!(refusal.scope, LimitScope::Session);
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
        assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 6, 6));
        assert_eq!(
            native.as_ref().map(|bytes| bytes.as_slice()),
            if native_first {
                Some(b"native".as_slice())
            } else {
                None
            },
        );
        assert_eq!(
            output.as_bytes(),
            if native_first {
                b"".as_slice()
            } else {
                b"output".as_slice()
            }
        );
        drop(output);
        assert_eq!(session.finish(), Err(refusal));
        assert!(
            matches!(ctx.finish_session(), Err(crate::CodecError::ResourceLimit(error)) if error == refusal)
        );
    }
}

#[test]
fn verification_charges_overlap_live_output_and_refund_only_that_buffer() {
    use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).expect("caller account");
    let mut output = session.buffer();
    output
        .append(b"output", "serialized output")
        .expect("six bytes");
    let verification = Box::new(*b"ok");
    ctx.retain_bytes(2, "verification fields")
        .expect("two bytes remain");
    assert_eq!(output.as_bytes(), b"output");
    assert_eq!(verification.as_slice(), b"ok");
    assert_eq!(ctx.retained_bytes_available(), 0);
    drop(output);
    assert_eq!(ctx.retained_bytes_available(), 6);
    let mut final_output = session.buffer();
    final_output
        .append(b"output", "same live output allowance")
        .expect("only buffer was refunded");
    let error = final_output
        .append(b"!", "extra serialized byte")
        .expect_err("verification remains charged");
    assert_eq!((error.limit, error.used, error.additional), (8, 8, 1));
    assert_eq!(verification.as_slice(), b"ok");
}

#[test]
fn shared_entity_refusal_overrides_a_later_structural_error() {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("source");
    let session = EncodeSession::from_context(&ctx).expect("caller account");
    let result = ctx.run_part(|| {
        let _ignored = session.charge_entities(1, "emit actual record");
        Err::<(), _>(crate::CodecError::malformed("later structural error"))
    });
    let Err(crate::CodecError::ResourceLimit(error)) = result else {
        panic!("fatal caller refusal wins")
    };
    assert_eq!(error.dimension, ResourceDimension::Entities);
    assert_eq!((error.limit, error.used, error.additional), (0, 0, 1));
    assert_eq!(session.finish(), Err(error));
    assert!(
        matches!(ctx.finish_session(), Err(crate::CodecError::ResourceLimit(actual)) if actual == error)
    );
}

#[test]
fn shared_entities_and_retention_survive_context_moves_and_drop() {
    use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 2;
    policy.limits.max_retained_bytes = 8;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).expect("caller account");
    let moved = ctx;
    moved
        .charge_entities(1, "first written record")
        .expect("first entity");
    session
        .charge_entities(1, "second written record")
        .expect("second entity");
    let native = Box::new(*b"native");
    moved
        .retain_bytes(6, "native remains live")
        .expect("six bytes");
    drop(moved);
    let mut output = session.buffer();
    let error = output
        .append(b"abc", "output after context drop")
        .expect_err("context drop cannot refund native content");
    assert_eq!((error.limit, error.used, error.additional), (8, 6, 3));
    assert_eq!(native.as_slice(), b"native");
    assert_eq!(session.charge_entities(1, "third record"), Err(error));
}

#[test]
fn context_and_encoder_entities_use_one_cumulative_counter() {
    use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 2;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).expect("caller account");
    ctx.charge_entities(1, "first native record")
        .expect("first record");
    session
        .charge_entities(1, "second written record")
        .expect("second record");
    let crate::CodecError::ResourceLimit(error) = ctx
        .charge_entities(1, "verification record")
        .expect_err("two prior records remain charged")
    else {
        panic!("entity resource refusal")
    };
    assert_eq!(error.dimension, ResourceDimension::Entities);
    assert_eq!((error.limit, error.used, error.additional), (2, 2, 1));
    assert_eq!(session.finish(), Err(error));
    assert!(
        matches!(ctx.finish_session(), Err(crate::CodecError::ResourceLimit(actual)) if actual == error)
    );
}

#[test]
fn encode_output_does_not_inherit_the_decode_only_proportional_ceiling() {
    use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let decode_allowance = ctx.retained_bytes_available();
    assert!(decode_allowance < policy.limits.max_retained_bytes);
    let output_len = usize::try_from(decode_allowance + 1).expect("test allocation fits");
    let serialized = vec![0_u8; output_len];
    let session = EncodeSession::from_context(&ctx).expect("caller account");
    let mut output = session.buffer();
    output
        .append(&serialized, "source-free output")
        .expect("caller absolute ceiling permits the output");
    let field = Box::new([1_u8]);
    ctx.retain_bytes(1, "verification field")
        .expect("decode-local proportional counter is separate");
    assert_eq!(output.as_bytes(), serialized);
    assert_eq!(field.as_slice(), &[1]);
    session.finish().expect("shared account remains valid");
    ctx.finish_session()
        .expect("decode local allowance remains valid");
}

#[test]
fn accounts_moved_to_threads_reserve_against_one_aggregate_ceiling() {
    use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    use std::sync::mpsc;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let session = EncodeSession::from_context(&ctx).expect("same account");
    let (ready, received) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let mut buffer = session.buffer();
        buffer
            .append(b"output", "thread's six output bytes")
            .expect("six bytes fit");
        ready.send(()).expect("signal actual live allocation");
        released.recv().expect("caller checked overlapping charge");
        (buffer.len(), session.finish())
    });
    received.recv().expect("thread's output is live");
    let result = ctx.retain_bytes(6, "caller's six native bytes");
    release.send(()).expect("release thread");
    let (len, finish) = thread.join().expect("bounded thread");
    let Err(crate::CodecError::ResourceLimit(error)) = result else {
        panic!("one aggregate resource ceiling")
    };
    assert_eq!((error.limit, error.used, error.additional), (8, 6, 6));
    assert_eq!(len, 6);
    assert_eq!(finish, Err(error));
    assert!(
        matches!(ctx.finish_session(), Err(crate::CodecError::ResourceLimit(actual)) if actual == error)
    );
}

#[test]
fn source_free_encode_helpers_use_absolute_retention_on_the_same_actual_account() {
    use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};
    const DECODE_FLOOR: u64 = 16 * 1024 * 1024;
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 2 * DECODE_FLOOR;
    let decode_arena = DecodeArena::new();
    let decode = DecodeContext::new(&decode_arena, &policy, DecodeDepth::Full);
    assert!(!decode.is_encoding());
    assert_eq!(decode.retained_bytes_available(), DECODE_FLOOR);
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &policy);
    assert!(ctx.is_encoding());
    assert_eq!(ctx.depth(), DecodeDepth::Full);
    assert_eq!(ctx.policy(), &policy);
    assert_eq!(
        ctx.retained_bytes_available(),
        policy.limits.max_retained_bytes
    );
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    assert!(std::sync::Arc::ptr_eq(
        &ctx.operation_budget(),
        &session.account
    ));
    ctx.retain_bytes(DECODE_FLOOR + 1, "actual retained encode helper")
        .expect("the encode allowance exceeds the decode floor");
    let available = policy.limits.max_retained_bytes - DECODE_FLOOR - 1;
    assert_eq!(ctx.retained_bytes_available(), available);
    let mut output = session.buffer();
    output
        .append(b"abc", "simultaneous encoded bytes")
        .expect("three output bytes fit the remaining allowance");
    assert_eq!(output.as_bytes(), b"abc");
    assert!(ctx.retained_bytes_available() < available);
    drop(output);
    assert_eq!(ctx.retained_bytes_available(), available);
    session
        .finish()
        .expect("the session finishes without a refusal");
    ctx.finish_session()
        .expect("the context finishes without a refusal");
    let crate::CodecError::ResourceLimit(error) = decode
        .retain_bytes(DECODE_FLOOR + 1, "unchanged decode allowance")
        .expect_err("the decode allowance stays at its floor")
    else {
        panic!("decode proportional refusal");
    };
    assert_eq!(
        (error.limit, error.used, error.additional),
        (DECODE_FLOOR, 0, DECODE_FLOOR + 1)
    );
    assert_eq!(decode.resource_refusal(), Some(error));
}

#[test]
fn output_refusal_is_observed_by_source_free_helpers_without_a_new_account() {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let mut output = session.buffer();
    output
        .append(b"1234", "full retained output")
        .expect("four output bytes fit the four-byte ceiling");
    let error = output
        .append(b"5", "fifth output byte")
        .expect_err("the fifth output byte exceeds the ceiling");
    assert_eq!((error.limit, error.used, error.additional), (4, 4, 1));
    assert!(
        matches!(ctx.retain_bytes(0,"ignored output refusal"),Err(crate::CodecError::ResourceLimit(actual)) if actual==error)
    );
    assert_eq!(session.finish(), Err(error));
    assert_eq!(ctx.resource_refusal(), Some(error));
}

#[test]
fn encode_helper_working_overflow_reports_absolute_usage_and_fuses_the_same_session() {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 32 * 1024 * 1024;
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    ctx.retain_bytes(5, "five held report bytes")
        .expect("five retained bytes fit the ceiling");
    let mut values = Vec::<u64>::new();
    let crate::CodecError::ResourceLimit(error) = ctx
        .reserve_working(&mut values, usize::MAX, "actual working size overflow")
        .expect_err("a usize::MAX working reservation is refused")
    else {
        panic!("working allocation refusal");
    };
    assert_eq!(error.reason, ResourceFailure::AllocationFailed);
    assert_eq!(error.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        (error.limit, error.used, error.additional),
        (policy.limits.max_retained_bytes, 5, u64::MAX)
    );
    assert_eq!(session.finish(), Err(error));
    assert_eq!(ctx.resource_refusal(), Some(error));
    assert!(values.is_empty());
}

#[test]
fn encode_helper_allocation_observes_the_existing_shared_refusal_before_cloning() {
    use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4;
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let crate::CodecError::ResourceLimit(error) = ctx
        .retain_bytes(5, "first absolute helper refusal")
        .expect_err("five retained bytes exceed the four-byte ceiling")
    else {
        panic!("actual caller ceiling");
    };
    assert_eq!((error.limit, error.used, error.additional), (4, 0, 5));
    assert!(matches!(
        ctx.alloc_filled(1, 0_u64, "allocation after prior refusal"),
        Err(crate::CodecError::ResourceLimit(actual)) if actual == error
    ));
    assert_eq!(ctx.resource_refusal(), Some(error));
    assert_eq!(session.finish(), Err(error));
}

#[test]
fn failed_backing_allocation_reports_only_live_bytes_and_keeps_the_first_refusal() {
    let arena = crate::decode::DecodeArena::new();
    let mut policy = crate::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::MAX;
    let ctx = crate::decode::DecodeContext::for_encode(&arena, &policy);
    ctx.retain_bytes(3, "held decoded record")
        .expect("three retained bytes fit an unbounded ceiling");
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let mut survivor = session.buffer();
    survivor
        .append(b"keep", "independent live bytes")
        .expect("four live bytes fit an unbounded ceiling");
    let mut buffer = session.buffer();
    buffer
        .append(b"old", "existing target bytes")
        .expect("three live bytes fit an unbounded ceiling");
    let old_capacity = buffer.bytes.capacity();
    let old_charge = buffer.capacity();
    let survivor_charge = survivor.capacity();
    let live = 3 + crate::decode::u64_from_index(old_charge + survivor_charge);
    assert_eq!(session.account.retained_used(), live);

    // Vec<u8> cannot reserve a capacity beyond isize::MAX. This real request
    // deterministically returns capacity overflow before allocating any heap.
    let additional = isize::MAX.unsigned_abs() + 1;
    let unused = old_capacity - buffer.len();
    let growth = additional - unused;
    assert!(growth > old_capacity);
    let refusal = buffer
        .reserve_append(additional, "impossible backing capacity")
        .expect_err("the actual Vec capacity cannot be represented");
    assert_eq!(refusal.scope, LimitScope::Session);
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.reason, ResourceFailure::AllocationFailed);
    assert_eq!(refusal.limit, policy.limits.max_retained_bytes);
    assert_eq!(refusal.used, live);
    assert_eq!(refusal.additional, crate::decode::u64_from_index(growth));
    assert_eq!(refusal.operation, "impossible backing capacity");
    assert_eq!(session.account.retained_used(), live);
    assert_eq!(buffer.bytes.capacity(), old_capacity);
    assert_eq!(buffer.capacity(), old_charge);
    assert_eq!(buffer.as_bytes(), b"old");
    assert_eq!(survivor.as_bytes(), b"keep");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
    assert_eq!(
        buffer.reserve_append(1, "later target request"),
        Err(refusal)
    );
    assert_eq!(
        survivor.append(b"later", "later independent request"),
        Err(refusal)
    );
    assert_eq!(buffer.as_bytes(), b"old");
    assert_eq!(survivor.as_bytes(), b"keep");
    drop(buffer);
    assert_eq!(
        session.account.retained_used(),
        live - crate::decode::u64_from_index(old_charge)
    );
    drop(survivor);
    assert_eq!(session.account.retained_used(), 3);
    assert_eq!(session.finish(), Err(refusal));
    assert!(
        matches!(ctx.finish_session(), Err(crate::CodecError::ResourceLimit(actual)) if actual == refusal)
    );
}
