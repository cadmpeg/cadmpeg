// SPDX-License-Identifier: Apache-2.0
//! Deferred edges preserve producer wake order and linear storage growth.

use std::collections::{HashMap, VecDeque};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::wake_deferred_dependents;

#[test]
fn deferred_registration_refuses_before_mutation_and_preserves_sticky_error() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let mut waiting = HashMap::new();
    let CodecError::ResourceLimit(first) = ctx
        .push_hash_group(&mut waiting, 1, 2, "test groups", "test members")
        .expect_err("key lookup requires work")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert!(waiting.is_empty());
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == first)
    );
}

#[test]
fn deferred_wakes_preserve_order_and_duplicate_edges_with_linear_storage() {
    for count in [1, 20, 100] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Each distinct edge uses at most a group, member and wake slot.
        // A duplicate edge uses a member and wake slot. Two dependents fit 8N + 2.
        policy.limits.max_collection_items = 8 * count + 2;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
        let mut storage = ctx
            .reserve_scoped(0, "test dependency owner")
            .expect("owner");
        let mut waiting = HashMap::new();
        let mut queue = VecDeque::new();
        storage
            .with_storage(|| -> Result<(), CodecError> {
                for dependency in 1..=count {
                    ctx.push_hash_group(
                        &mut waiting,
                        dependency,
                        101,
                        "test groups",
                        "test members",
                    )?;
                    ctx.push_hash_group(
                        &mut waiting,
                        dependency,
                        102,
                        "test groups",
                        "test members",
                    )?;
                }
                ctx.push_hash_group(&mut waiting, count, 101, "test groups", "test members")?;
                for dependency in 1..=count {
                    wake_deferred_dependents(dependency, &waiting, &mut queue, &ctx, "test queue")?;
                    assert_eq!(queue.pop_front(), Some(101));
                    assert_eq!(queue.pop_front(), Some(102));
                    let expected = if dependency < count {
                        vec![101, 102]
                    } else {
                        vec![101, 102, 101]
                    };
                    assert_eq!(waiting.get(&dependency), Some(&expected));
                }
                assert_eq!(queue, VecDeque::from([101]));
                assert_eq!(
                    waiting.len(),
                    usize::try_from(count).expect("test group count")
                );
                Ok(())
            })
            .expect("linear edge and wake allowance");
        drop(queue);
        drop(waiting);
        drop(storage);
        let reuse = ctx
            .reserve_scoped(
                policy.limits.max_materialized_bytes,
                "test surface wake scratch reuse",
            )
            .expect("released dependency backing");
        drop(reuse);
        ctx.finish_session().expect("released dependency backing");
    }
}
