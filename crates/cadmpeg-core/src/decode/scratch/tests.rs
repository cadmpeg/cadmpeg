// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use crate::decode::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension, ResourceLimits,
};

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
fn a_scratch_that_runs_out_of_entities_keeps_its_work_and_gives_up_its_value() {
    let policy = policy();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let limit = ctx
        .scratch(|scratch| {
            scratch.charge_entities(4, "read and dropped")?;
            scratch.charge_entities(10, "past the scratch cap")
        })
        .unwrap()
        .unwrap_err();

    assert_eq!(limit.dimension, ResourceDimension::Entities);
    assert_eq!(limit.operation, "past the scratch cap");
    assert!(ctx.resource_refusal().is_none());
    ctx.charge_entities(9, "kept document records").unwrap();
    assert!(ctx.charge_entities(1, "past the cap").is_err());
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
fn a_traced_given_up_scratch_runs_at_its_composition_position() {
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

    assert!(!ctx.accept_trace(&task.take_trace()));
    assert_eq!(ctx.retained_bytes_available(), 100);
    ctx.charge_entities(13, "all the room").unwrap();
}

#[test]
fn a_trace_releases_retained_bytes_at_each_scratch_boundary() {
    let policy = policy();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    task.scratch(|outer| {
        outer.retain_bytes(50, "outer scratch")?;
        for _ in 0..2 {
            outer
                .scratch(|inner| inner.retain_bytes(40, "inner scratch"))?
                .unwrap();
        }
        Ok(())
    })
    .unwrap()
    .unwrap();
    task.retain_bytes(10, "kept").unwrap();

    let trace = task.take_trace();
    ctx.retain_bytes(10, "earlier output").unwrap();
    assert!(ctx.accept_trace(&trace));
    assert_eq!(ctx.retained_bytes_available(), 80);
    assert!(!ctx.accept_trace(&trace));
    assert_eq!(ctx.retained_bytes_available(), 80);
}
