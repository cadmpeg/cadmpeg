// SPDX-License-Identifier: Apache-2.0
//! Seekable serialization preserves bytes and the actual caller's first refusal.

use std::cell::Cell;
use std::io::{self, Cursor, Seek, SeekFrom, Write};

use super::OutputSink;
use crate::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceFailure,
};
use crate::encode::EncodeSession;
use crate::io::WriteSeek;
use crate::CodecError;

fn policy(limit: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    policy
}

fn edit(output: &mut dyn WriteSeek) -> io::Result<u64> {
    output.write_all(b"abcdef")?;
    output.seek(SeekFrom::Start(2))?;
    output.write_all(b"XYZ")?;
    output.seek(SeekFrom::End(-1))?;
    output.write_all(b"tail")?;
    output.seek(SeekFrom::Current(3))?;
    output.write_all(b"end")?;
    output.flush()?;
    output.stream_position()
}

#[test]
fn seekable_edits_match_cursor_bytes_positions_and_sparse_zeroes() {
    let mut cursor = Cursor::new(Vec::new());
    let expected_position = edit(&mut cursor).expect("cursor edits succeed");
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let (bytes, position) = session
        .write_seekable("authored seek edits", edit)
        .expect("seek edits fit the service ceiling")
        .expect("the edit callback succeeds");
    assert_eq!(bytes, cursor.into_inner());
    assert_eq!(bytes, b"abXYZtail\0\0\0end");
    assert_eq!(position, expected_position);
    assert_eq!(
        ctx.retained_bytes_available(),
        ctx.policy().limits.max_retained_bytes
    );
    session
        .finish()
        .expect("the session finishes without a refusal");
    ctx.finish_session()
        .expect("the context finishes without a refusal");
}

#[test]
fn ignored_write_zero_cannot_hide_the_actual_resource_refusal() {
    let arena = DecodeArena::new();
    let policy = policy(0);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let result = session.write_seekable("four authored bytes", |output| {
        assert_eq!(
            output
                .write_all(b"data")
                .expect_err("four bytes exceed a zero-byte ceiling")
                .kind(),
            io::ErrorKind::WriteZero
        );
        Ok::<_, io::Error>("ignored operation error")
    });
    let refusal = result.expect_err("the ignored write refusal still reaches the caller");
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.reason, ResourceFailure::BudgetExceeded);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (0, 0, 4));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
    assert_eq!(ctx.retained_bytes_available(), 0);
}

#[test]
fn independent_live_output_uses_the_same_aggregate_allowance() {
    let arena = DecodeArena::new();
    let policy = policy(8);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let mut held = session.buffer();
    held.append(b"held", "four existing output bytes")
        .expect("four held bytes fit the eight-byte ceiling");
    let used = u64_from_index(held.capacity());
    let refusal = session
        .write_seekable("five simultaneous bytes", |output| {
            output.write_all(b"five!")
        })
        .expect_err("five more bytes exceed the shared ceiling");
    assert_eq!(
        (refusal.limit, refusal.used, refusal.additional),
        (8, used, 5)
    );
    assert_eq!(held.as_bytes(), b"held");
    assert_eq!(session.account.retained_used(), used);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    drop(held);
    assert_eq!(session.account.retained_used(), 0);
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn a_preexisting_caller_refusal_prevents_callback_entry() {
    let arena = DecodeArena::new();
    let policy = policy(8);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let CodecError::ResourceLimit(refusal) = ctx
        .retain_bytes(9, "nine actual helper bytes")
        .expect_err("nine retained bytes exceed the eight-byte ceiling")
    else {
        panic!("the caller ceiling refuses nine bytes")
    };
    let called = Cell::new(false);
    let result = session.write_seekable("after helper refusal", |_| {
        called.set(true);
        Ok::<_, io::Error>(())
    });
    assert_eq!(
        result.expect_err("the prior refusal blocks the callback"),
        refusal
    );
    assert!(!called.get());
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn invalid_standard_seek_remains_an_operation_error_without_output() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let result = session.write_seekable("invalid authored seek", |output| {
        output.write_all(b"kept")?;
        output.seek(SeekFrom::Start(0))?;
        output.seek(SeekFrom::Current(-1))?;
        Ok::<_, io::Error>(())
    });
    assert_eq!(
        result
            .expect("an invalid seek charges no resource")
            .expect_err("a negative position is an operation error")
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(session.account.retained_used(), 0);
    assert_eq!(ctx.resource_refusal(), None);
    session
        .finish()
        .expect("the session finishes without a refusal");
}

#[test]
fn full_growth_is_admitted_before_any_prefix_or_gap_mutation() {
    let arena = DecodeArena::new();
    let policy = policy(12);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let mut output = OutputSink {
        buffer: session.buffer(),
        position: 0,
        operation: "cross-end write",
    };
    output
        .write_all(b"abcd")
        .expect("four bytes fit the twelve-byte ceiling");
    output
        .seek(SeekFrom::Start(2))
        .expect("a seek inside written bytes succeeds");
    assert_eq!(
        output
            .write(b"abcdefghijkl")
            .expect("a refused write reports zero bytes"),
        0
    );
    let refusal = session
        .finish()
        .expect_err("the oversized write latches a refusal");
    assert_eq!(output.buffer.as_bytes(), b"abcd");
    assert_eq!(output.position, 2);
    output
        .seek(SeekFrom::Start(0))
        .expect("a seek after a refusal succeeds");
    assert_eq!(
        output
            .write(b"z")
            .expect("a write after a refusal reports zero bytes"),
        0
    );
    assert_eq!(output.buffer.as_bytes(), b"abcd");
    assert_eq!(session.finish(), Err(refusal));
    drop(output);
    assert_eq!(session.account.retained_used(), 0);
}

#[test]
fn gap_reservation_keeps_the_remaining_full_write_allowance() {
    let arena = DecodeArena::new();
    let policy = policy(150);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let (bytes, ()) = session
        .write_seekable("gap and payload", |output| {
            output.write_all(b"12345678")?;
            output.seek(SeekFrom::Start(100))?;
            output.write_all(&[9; 50])
        })
        .expect("gap and payload fit the ceiling")
        .expect("the gap callback succeeds");
    assert_eq!(&bytes[..8], b"12345678");
    assert_eq!(&bytes[8..100], &[0; 92]);
    assert_eq!(&bytes[100..], &[9; 50]);
    assert_eq!(bytes.len(), 150);
    assert!(bytes.capacity() <= 150);
    assert_eq!(ctx.resource_refusal(), None);
    assert_eq!(session.account.retained_used(), 0);
}

#[test]
fn unaddressable_gap_latches_the_real_minimum_backing_growth() {
    let arena = DecodeArena::new();
    let policy = policy(u64::MAX);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let mut output = OutputSink {
        buffer: session.buffer(),
        position: 0,
        operation: "unaddressable gap",
    };
    output
        .write_all(b"old")
        .expect("three bytes fit an unbounded ceiling");
    let capacity = output.buffer.capacity();
    output
        .seek(SeekFrom::Start(u64::MAX))
        .expect("a seek to u64::MAX succeeds");
    assert_eq!(
        output
            .write(b"x")
            .expect("an unaddressable write reports zero bytes"),
        0
    );
    let refusal = session
        .finish()
        .expect_err("the unaddressable gap latches a refusal");
    assert_eq!(refusal.reason, ResourceFailure::AllocationFailed);
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(refusal.limit, u64::MAX);
    assert_eq!(refusal.used, u64_from_index(capacity));
    assert_eq!(refusal.additional, u64::MAX - u64_from_index(capacity));
    assert_eq!(output.buffer.as_bytes(), b"old");
    assert_eq!(output.buffer.capacity(), capacity);
    assert_eq!(output.position, u64::MAX);
    drop(output);
    assert_eq!(session.account.retained_used(), 0);
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}

#[test]
fn empty_write_at_a_sparse_position_neither_allocates_nor_refuses() {
    let arena = DecodeArena::new();
    let policy = policy(0);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let (bytes, position) = session
        .write_seekable("empty sparse write", |output| {
            output.seek(SeekFrom::Start(u64::MAX))?;
            assert_eq!(output.write(&[])?, 0);
            output.stream_position()
        })
        .expect("an empty sparse write needs no allowance")
        .expect("the sparse callback succeeds");
    assert!(bytes.is_empty());
    assert_eq!(bytes.capacity(), 0);
    assert_eq!(position, u64::MAX);
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn final_handoff_refunds_only_the_owned_output() {
    let arena = DecodeArena::new();
    let policy = policy(64);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let mut held = session.buffer();
    held.append(b"held", "independent output")
        .expect("four held bytes fit the ceiling");
    let charge = u64_from_index(held.capacity());
    let (bytes, ()) = session
        .write_seekable("final output", |output| output.write_all(b"final"))
        .expect("final output fits the ceiling")
        .expect("the final callback succeeds");
    assert_eq!(bytes, b"final");
    assert_eq!(held.as_bytes(), b"held");
    assert_eq!(session.account.retained_used(), charge);
    drop(held);
    assert_eq!(session.account.retained_used(), 0);
    assert_eq!(ctx.retained_bytes_available(), 64);
}

#[test]
fn serializer_drop_refusal_precedes_a_callback_success() {
    struct Trailer<'a>(&'a mut dyn WriteSeek);
    impl Drop for Trailer<'_> {
        fn drop(&mut self) {
            let _result = self.0.write_all(b"trailer");
        }
    }
    let arena = DecodeArena::new();
    let policy = policy(0);
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&ctx).expect("encode session opens from a fresh context");
    let refusal = session
        .write_seekable("drop trailer", |output| {
            let _trailer = Trailer(output);
            Ok::<_, io::Error>(())
        })
        .expect_err("the trailer write exceeds a zero-byte ceiling");
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (0, 0, 7));
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(session.account.retained_used(), 0);
}
