// SPDX-License-Identifier: Apache-2.0

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
fn discarded_evidence_leaves_the_document_its_remaining_bytes_and_entities() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy(), DecodeDepth::Full);
    ctx.retain_bytes(10, "kept").unwrap();
    let result = ctx.scratch(0, |scratch| {
        scratch.retain_bytes(95, "discarded bytes")?;
        scratch.charge_entities(13, "discarded records")?;
        Ok(7)
    });
    assert_eq!(result.unwrap().unwrap(), 7);
    ctx.retain_bytes(90, "kept document bytes").unwrap();
    ctx.charge_entities(13, "kept document records").unwrap();
}

#[test]
fn a_local_refusal_leaves_the_session_usable() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy(), DecodeDepth::Full);
    let limit = ctx
        .scratch(0, |scratch| {
            scratch.charge_entities(4, "read and dropped")?;
            scratch.retain_bytes(101, "discarded bytes")
        })
        .unwrap()
        .unwrap_err();

    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert!(ctx.resource_refusal().is_none());
    ctx.retain_bytes(100, "kept document bytes").unwrap();
    ctx.charge_entities(13, "kept document records").unwrap();
}

#[test]
fn a_captured_parent_charge_fails_closed_without_erasing_the_charge() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_entities = 1;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let changed = ctx.scratch(0, |_| ctx.charge_entities(1, "parent"));
    assert!(matches!(changed, Err(CodecError::Malformed(_))));
    assert!(ctx.charge_entities(1, "parent again").is_err());
}

#[test]
fn a_captured_parent_refusal_remains_in_force() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_entities = 1;
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let refused = ctx.scratch(0, |_| {
        assert!(ctx.charge_entities(2, "parent").is_err());
        Ok(())
    });
    let Err(CodecError::ResourceLimit(limit)) = refused else {
        panic!("the parent refusal must escape");
    };
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(ctx
        .scratch(0, |_| -> Result<(), CodecError> { panic!("parent fused") })
        .is_err());
}

#[test]
fn evidence_allowances_depend_on_the_carrier_at_every_composition_position() {
    let source_arena = DecodeArena::new();
    let task_arena = DecodeArena::new();
    let parent_arena = DecodeArena::new();
    let source = DecodeContext::new(&source_arena, &policy(), DecodeDepth::Full);
    let parent = DecodeContext::new(&parent_arena, &policy(), DecodeDepth::Full);
    source.charge_input(4096, "source input").unwrap();
    let task = source.checkpoint().resume(&task_arena);
    let read = |context: &DecodeContext<'_>, input_bytes| {
        context
            .scratch(input_bytes, |scratch| {
                Ok(scratch.per_expand_allowance() >= 17 * 1024 * 1024)
            })
            .unwrap()
            .unwrap()
    };
    assert!(read(&task, 4096));
    assert!(read(&parent, 4096));
    assert!(!read(&parent, 0));
    assert_eq!(
        task.retained_bytes_available(),
        parent.retained_bytes_available()
    );
    assert!(parent.accept_trace(&task.take_trace()));
}

#[test]
fn a_local_refusal_does_not_discard_a_parallel_result() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy(), DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    assert!(task
        .scratch(0, |scratch| scratch
            .charge_entities(14, "discarded records"))
        .unwrap()
        .is_err());
    task.retain_bytes(10, "kept").unwrap();
    assert!(ctx.accept_trace(&task.take_trace()));
    assert_eq!(ctx.retained_bytes_available(), 90);
}

#[test]
fn an_optional_admission_is_atomic_and_a_refusal_does_not_fuse() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy(), DecodeDepth::Full);
    assert_eq!(
        ctx.admit_optional(1, 14, "optional").unwrap_err().dimension,
        ResourceDimension::Entities
    );
    assert_eq!(
        ctx.admit_optional(101, 1, "optional")
            .unwrap_err()
            .dimension,
        ResourceDimension::RetainedBytes
    );
    ctx.admit_optional(100, 13, "whole allowance").unwrap();
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn a_trace_checks_only_the_optional_output_admission() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy(), DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    task.admit_optional(100, 1, "keep optional evidence")
        .unwrap();

    let trace = task.take_trace();
    let other = DecodeContext::new(&arena, &policy(), DecodeDepth::Full);
    other.retain_bytes(1, "prior").unwrap();
    assert!(!other.accept_trace(&trace));
    assert_eq!(other.retained_bytes_available(), 99);
    assert!(ctx.accept_trace(&trace));
    assert_eq!(ctx.retained_bytes_available(), 0);
}
