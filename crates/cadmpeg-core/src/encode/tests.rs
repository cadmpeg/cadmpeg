// SPDX-License-Identifier: Apache-2.0
//! Real caller limits govern retained bytes, lifetime, and emitted records.

use super::EncodeSession;
use crate::decode::{LimitScope, ResourceDimension, ResourceFailure, ResourceLimits};

#[test]
fn simultaneous_buffers_share_the_exact_live_byte_ceiling_and_fuse_refusal() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 5;
    let session = EncodeSession::new(limits);
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
    let session = EncodeSession::new(limits);
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
    let session = EncodeSession::new(limits);
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
    let session = EncodeSession::new(limits);
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
    let session = EncodeSession::new(limits);
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
    let session = EncodeSession::new(limits);
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
    let session = EncodeSession::new(limits);
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
    let session = EncodeSession::new(limits);
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
        session.retained.get(),
        crate::decode::u64_from_index(first.capacity())
    );
    let mut second = session.buffer();
    second
        .append(b"ef", "second backing allocation")
        .expect("shared capacity");
    assert_eq!(
        session.retained.get(),
        crate::decode::u64_from_index(first.capacity() + second.capacity())
    );
    drop(first);
    assert_eq!(
        session.retained.get(),
        crate::decode::u64_from_index(second.capacity())
    );
}

#[test]
fn excess_actual_capacity_is_released_before_a_fused_refusal_returns() {
    let mut limits = ResourceLimits::service();
    limits.max_retained_bytes = 10;
    let session = EncodeSession::new(limits);
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
        session.retained.get(),
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
        .admit_reserved_capacity("actual reserved record")
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
        session.retained.get(),
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
        session.retained.get(),
        crate::decode::u64_from_index(other_charge)
    );
    drop(unrelated);
    assert_eq!(session.retained.get(), 0);
}
