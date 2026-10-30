// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceLimits};

#[test]
fn decode_trace_replays_charges_in_order_without_partial_commit() {
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_entities: 13,
            ..ResourceLimits::default()
        },
        ..DecodePolicy::default()
    };
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    task.charge_entities(5, "first").unwrap();
    task.charge_entities(7, "second").unwrap();
    let trace = task.take_trace();
    ctx.charge_entities(2, "prior work").unwrap();
    assert!(!ctx.accept_trace(&trace));
    assert!(ctx.resource_refusal().is_none());
    ctx.charge_entities(11, "unchanged remainder").unwrap();
}

#[test]
fn decode_trace_applies_nested_depth_relative_to_its_composition_site() {
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_recursion_depth: 2,
            ..ResourceLimits::default()
        },
        ..DecodePolicy::default()
    };
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    {
        let _first = task.enter_nested("first").unwrap();
        let _second = task.enter_nested("second").unwrap();
    }
    let trace = task.take_trace();
    assert!(ctx.accept_trace(&trace));
    let _outer = ctx.enter_nested("outer").unwrap();
    assert!(!ctx.accept_trace(&trace));
    let _remaining = ctx.enter_nested("remaining").unwrap();
}

#[test]
fn decode_trace_accepts_changed_allowance_only_when_decisions_match() {
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_retained_bytes: 100,
            ..ResourceLimits::default()
        },
        ..DecodePolicy::default()
    };
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    let available = task.retained_allowance();
    task.observe_retained_allowance(available, 40);
    task.observe_retained_allowance(available - 40, 30);
    let trace = task.take_trace();
    ctx.retain_bytes(20, "earlier output").unwrap();
    assert!(ctx.accept_trace(&trace));
    ctx.retain_bytes(20, "more output").unwrap();
    assert!(!ctx.accept_trace(&trace));
    assert_eq!(ctx.retained_bytes_available(), 60);
}

#[test]
fn decode_checkpoint_is_send_sync_and_tasks_use_independent_counters() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<super::DecodeCheckpoint>();
    assert_send_sync::<super::DecodeTrace>();
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let checkpoint = ctx.checkpoint();
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let task_arena = DecodeArena::new();
                let task = checkpoint.resume(&task_arena);
                task.charge_entities(19, "parallel task").unwrap();
            })
            .join()
            .unwrap();
    });
    assert_eq!(ctx.checkpoint().state.entities, 0);
}

#[test]
fn decode_trace_compacts_charges_and_balanced_nesting_between_observations() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    for _ in 0..1024 {
        let _depth = task.enter_nested("nested item").unwrap();
        task.charge_entities(1, "item").unwrap();
        task.retain_bytes(3, "item bytes").unwrap();
    }
    let trace = task.take_trace();
    assert_eq!(trace.events.len(), 1);
    assert!(ctx.accept_trace(&trace));
    assert_eq!(ctx.checkpoint().state.entities, 1024);
    assert_eq!(ctx.checkpoint().state.retained, 3072);
    assert_eq!(ctx.checkpoint().state.depth, 0);
}

#[test]
fn decode_trace_compaction_keeps_retained_observations_in_order() {
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    task.retain_bytes(13, "before observation").unwrap();
    assert_eq!(
        task.retained_bytes_available(),
        DecodePolicy::default()
            .limits
            .max_retained_bytes
            .min(crate::decode::policy::RETAINED_BASE)
            - 13
    );
    task.retain_bytes(17, "after observation").unwrap();
    let trace = task.take_trace();
    assert_eq!(trace.events.len(), 3);
    assert!(ctx.accept_trace(&trace));
    ctx.retain_bytes(1, "prior output").unwrap();
    assert!(!ctx.accept_trace(&trace));
}

#[test]
fn decode_context_temporary_arena_commits_charges_and_refusal() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_entities: 7,
            ..ResourceLimits::default()
        },
        ..DecodePolicy::default()
    };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let temporary = DecodeArena::new();
    let error = ctx
        .with_arena(&temporary, |local| {
            local.charge_entities(5, "scratch entities")?;
            local.charge_entities(3, "scratch refusal")
        })
        .unwrap_err();
    let crate::CodecError::ResourceLimit(limit) = error else {
        panic!("must refuse");
    };
    assert_eq!(ctx.checkpoint().state.entities, 5);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(limit.operation, "scratch refusal");
}

#[test]
fn decode_context_temporary_arena_commits_trace_and_space_ids() {
    use crate::decode::{ByteRange, View};
    let arena = DecodeArena::new();
    let ctx = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    let temporary = DecodeArena::new();
    let space = task.with_arena(&temporary, |local| {
        local.charge_entities(11, "temporary entities").unwrap();
        local
            .register_slice(View::over_retained(b"abc"), ByteRange { start: 0, end: 2 })
            .unwrap()
            .space()
    });
    assert_eq!(space.index(), 1);
    let trace = task.take_trace();
    assert!(ctx.accept_trace(&trace));
    assert_eq!(ctx.checkpoint().state.entities, 11);
    assert_eq!(ctx.checkpoint().spaces, 1);
}

#[test]
fn decode_trace_refused_task_releases_its_recording() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy {
        limits: ResourceLimits {
            max_entities: 13,
            ..ResourceLimits::default()
        },
        ..DecodePolicy::default()
    };
    let ctx = DecodeContext::new(&arena, &policy, DecodeDepth::Full);
    let task_arena = DecodeArena::new();
    let task = ctx.checkpoint().resume(&task_arena);
    task.charge_entities(11, "before refusal").unwrap();
    assert!(task.charge_entities(3, "refused charge").is_err());
    let trace = task.take_trace();
    assert_eq!(trace.retained_bytes_estimate(), 0);
    assert!(!ctx.accept_trace(&trace));
    assert!(ctx.resource_refusal().is_none());
}
