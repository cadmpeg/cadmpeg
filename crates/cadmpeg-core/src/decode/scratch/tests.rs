// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use crate::decode::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension, ResourceLimits,
};
use crate::CodecError;

fn policy() -> DecodePolicy {
    DecodePolicy {
        limits: ResourceLimits {
            max_retained_bytes: 100,
            max_entities: 13,
            ..ResourceLimits::default()
        },
    }
}

#[test]
fn a_scratch_releases_its_retained_bytes_and_keeps_its_entities() {
    let policy = policy();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    ctx.retain_bytes(10, "kept").unwrap();
    let kept = ctx
        .scratch(|scratch| {
            scratch.retain_bytes(80, "read and dropped")?;
            scratch.charge_entities(5, "read and dropped")?;
            Ok(7)
        })
        .unwrap()
        .unwrap();
    assert_eq!(kept, 7);
    assert_eq!(ctx.retained_bytes_available(), 90);
    ctx.charge_entities(8, "remainder").unwrap();
    assert!(ctx.charge_entities(1, "past the cap").is_err());
}

#[test]
fn a_scratch_past_the_retained_allowance_gives_up_only_its_output() {
    let policy = policy();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    ctx.retain_bytes(10, "kept").unwrap();
    let given_up = ctx
        .scratch(|scratch| {
            scratch.charge_entities(4, "read and dropped")?;
            scratch.retain_bytes(95, "read and dropped")
        })
        .unwrap()
        .unwrap_err();
    assert_eq!(given_up.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(given_up.operation, "read and dropped");
    assert!(ctx.resource_refusal().is_none());
    ctx.retain_bytes(90, "kept").unwrap();
    ctx.charge_entities(9, "remainder").unwrap();
    assert!(ctx.charge_entities(1, "past the cap").is_err());
}

#[test]
fn a_scratch_refused_in_another_dimension_refuses_its_context() {
    let policy = policy();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let refused = ctx.scratch(|scratch| scratch.charge_entities(14, "read and dropped"));
    let Err(CodecError::ResourceLimit(limit)) = refused else {
        panic!("expected a resource refusal: {refused:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

/// Retains 50 bytes, then 10 more when 45 bytes of room remain.
fn optional_scratch(ctx: &DecodeContext<'_>) -> u64 {
    ctx.scratch(|scratch| {
        scratch.retain_bytes(50, "read and dropped")?;
        scratch.charge_entities(3, "read and dropped")?;
        let room = scratch.retained_allowance();
        scratch.observe_retained_allowance(room, 45);
        if room >= 45 {
            scratch.retain_bytes(10, "optional")?;
            return Ok(1);
        }
        Ok(0)
    })
    .unwrap()
    .unwrap()
}

#[test]
fn a_traced_scratch_composes_where_its_retained_decisions_hold() {
    let policy = policy();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    assert_eq!(optional_scratch(&task), 1);
    let trace = task.take_trace();

    ctx.retain_bytes(4, "prior").unwrap();
    assert_eq!(ctx.retained_bytes_available(), 96);
    assert!(ctx.accept_trace(&trace));
    assert_eq!(ctx.retained_bytes_available(), 96);
    ctx.charge_entities(10, "remainder").unwrap();
    assert!(ctx.charge_entities(1, "past the cap").is_err());

    let other_arena = DecodeArena::new();
    let other = DecodeContext::new(&other_arena, &policy, DecodeDepth::Full);
    other.retain_bytes(10, "prior").unwrap();
    assert!(!other.accept_trace(&trace));
    assert_eq!(optional_scratch(&other), 0);
}

#[test]
fn a_traced_given_up_scratch_composes_only_at_the_same_retained_room() {
    let policy = policy();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    let given_up = task
        .scratch(|scratch| {
            scratch.charge_entities(3, "read and dropped")?;
            scratch.retain_bytes(101, "read and dropped")
        })
        .unwrap();
    assert!(given_up.is_err());
    let trace = task.take_trace();

    let other_arena = DecodeArena::new();
    let other = DecodeContext::new(&other_arena, &policy, DecodeDepth::Full);
    other.retain_bytes(1, "prior").unwrap();
    assert!(!other.accept_trace(&trace));

    assert!(ctx.accept_trace(&trace));
    assert_eq!(ctx.retained_bytes_available(), 100);
    ctx.charge_entities(10, "remainder").unwrap();
    assert!(ctx.charge_entities(1, "past the cap").is_err());
}
