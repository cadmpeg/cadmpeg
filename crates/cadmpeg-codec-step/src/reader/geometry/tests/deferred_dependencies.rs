// SPDX-License-Identifier: Apache-2.0
//! Deferred edges preserve producer wake order and linear storage growth.

use std::collections::VecDeque;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{wake_deferred_dependents, DeferredDependencies};

#[test]
fn deferred_registration_refuses_before_mutation_and_preserves_sticky_error() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut waiting = DeferredDependencies::default();
    let CodecError::ResourceLimit(first) = waiting
        .register(&ctx, 1, 2, "test groups", "test members")
        .expect_err("key lookup requires work")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert!(waiting.waiting_on.is_empty());
    assert!(waiting.remaining.is_empty());
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
    );
}

#[test]
fn deferred_wakes_preserve_order_and_duplicate_edges_with_linear_storage() {
    for count in [1, 20, 100] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Each distinct edge uses at most a count, group, member and wake slot.
        // A duplicate edge uses a member and wake slot. Two dependents fit 8N + 2.
        policy.limits.max_collection_items = 8 * count + 2;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
        let mut storage = ctx
            .reserve_scoped(0, "test dependency owner")
            .expect("owner");
        let mut waiting = DeferredDependencies::default();
        let mut queue = VecDeque::new();
        storage
            .with_storage(|| -> Result<(), CodecError> {
                for dependency in 1..=count {
                    waiting.register(&ctx, dependency, 101, "test groups", "test members")?;
                    waiting.register(&ctx, dependency, 102, "test groups", "test members")?;
                }
                waiting.register(&ctx, count, 101, "test groups", "test members")?;
                for dependency in 1..=count {
                    wake_deferred_dependents(
                        dependency,
                        &mut waiting,
                        &mut queue,
                        &ctx,
                        "test queue",
                    )?;
                    assert_eq!(queue.pop_front(), Some(101));
                    assert_eq!(queue.pop_front(), Some(102));
                    if dependency < count {
                        assert_eq!(
                            waiting.remaining.get(&101),
                            Some(
                                &(usize::try_from(count - dependency + 1)
                                    .expect("test count fits usize"))
                            )
                        );
                        assert_eq!(
                            waiting.remaining.get(&102),
                            Some(
                                &(usize::try_from(count - dependency)
                                    .expect("test count fits usize"))
                            )
                        );
                    }
                }
                assert_eq!(queue, VecDeque::from([101]));
                assert!(waiting.waiting_on.is_empty());
                assert!(waiting.remaining.is_empty());
                Ok(())
            })
            .expect("linear edge and wake allowance");
        drop(queue);
        drop(waiting);
        drop(storage);
        ctx.finish_session().expect("released dependency backing");
    }
}
