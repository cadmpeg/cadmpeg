// SPDX-License-Identifier: Apache-2.0
use std::collections::VecDeque;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::DeferredCurves;

fn fifo_order(dependencies: &[Vec<usize>]) -> Vec<usize> {
    let mut queue: VecDeque<_> = (0..dependencies.len()).collect();
    let mut waiting = vec![Vec::new(); dependencies.len()];
    let mut done = vec![false; dependencies.len()];
    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        if done[node] {
            continue;
        }
        let missing: Vec<_> = dependencies[node]
            .iter()
            .copied()
            .filter(|&p| !done[p])
            .collect();
        if missing.is_empty() {
            done[node] = true;
            order.push(node);
            queue.extend(std::mem::take(&mut waiting[node]));
        } else {
            for parent in missing {
                waiting[parent].push(node);
            }
        }
    }
    order
}

fn compressed_order(dependencies: &[Vec<usize>], work: u64) -> Vec<usize> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let mut storage = ctx
        .reserve_scoped(0, "test scheduler owner")
        .expect("owner");
    let mut scheduler = DeferredCurves::new(
        (0..dependencies.len())
            .map(|i| u64::try_from(i).expect("small test index"))
            .collect(),
    );
    let mut done = vec![false; dependencies.len()];
    let mut order = Vec::new();
    storage
        .with_storage(|| -> Result<(), CodecError> {
            while scheduler.has_next() {
                let node =
                    usize::try_from(scheduler.next(&ctx)?.expect("pending node")).expect("index");
                let missing: Vec<_> = dependencies[node]
                    .iter()
                    .copied()
                    .filter(|&p| !done[p])
                    .collect();
                if missing.is_empty() {
                    done[node] = true;
                    order.push(node);
                    scheduler.complete(u64::try_from(node).expect("index"), &ctx)?;
                } else {
                    for parent in missing {
                        scheduler.register(
                            &ctx,
                            u64::try_from(parent).expect("index"),
                            u64::try_from(node).expect("index"),
                            "test groups",
                            "test members",
                        )?;
                    }
                }
            }
            Ok(())
        })
        .expect("bounded scheduling");
    drop(scheduler);
    drop(storage);
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test full scheduler scratch reuse",
        )
        .expect("all scheduler storage was released");
    drop(reuse);
    ctx.finish_session().expect("released scheduler storage");
    order
}

#[test]
fn deferred_history_preserves_chained_partial_retry_order() {
    let dependencies = [vec![3, 4], vec![2, 3, 4], vec![3], vec![], vec![2]];
    assert_eq!(compressed_order(&dependencies, 100_000), [3, 2, 4, 1, 0]);
}

#[test]
fn deferred_history_preserves_later_initial_occurrences_inside_a_wake() {
    let dependencies = [vec![1, 3], vec![3], vec![1, 3], vec![]];
    assert_eq!(compressed_order(&dependencies, 100_000), [3, 1, 2, 0]);
}

#[test]
fn deferred_history_matches_fifo_on_shared_graphs_and_cycles() {
    let mut state = 7919_u64;
    for count in 2..=7 {
        for _ in 0..300 {
            let mut dependencies = vec![Vec::new(); count];
            for (node, parents) in dependencies.iter_mut().enumerate() {
                for parent in 0..count {
                    state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1);
                    if parent != node && state >> 61 == 0 {
                        parents.push(parent);
                        if state & 1 == 0 {
                            parents.push(parent);
                        }
                    }
                }
            }
            assert_eq!(
                compressed_order(&dependencies, 100_000),
                fifo_order(&dependencies),
                "{dependencies:?}"
            );
        }
    }
}

#[test]
fn deferred_history_chain_fits_linear_work_allowance() {
    for count in [8, 16, 128, 512] {
        let mut dependencies = vec![(1..=count).collect()];
        dependencies.extend((1..=count).map(|i| if i < count { vec![i + 1] } else { vec![] }));
        let expected: Vec<_> = (0..=count).rev().collect();
        // At most 64 prefix levels per key, with 24-byte hash keys and
        // amortized table growth, plus bounded history and heap searches.
        let graph_items = dependencies.len() + dependencies.iter().map(Vec::len).sum::<usize>();
        let work = 16_384 * u64::try_from(graph_items).expect("test graph size");
        assert_eq!(compressed_order(&dependencies, work), expected);
    }
}

#[test]
fn deferred_history_releases_scoped_storage_after_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let mut storage = ctx
        .reserve_scoped(0, "test scheduler owner")
        .expect("owner");
    let mut scheduler = DeferredCurves::new(vec![1]);
    let error = storage
        .with_storage(|| scheduler.next(&ctx))
        .expect_err("key storage admission");
    assert!(
        matches!(&error, CodecError::ResourceLimit(refusal) if refusal.dimension == ResourceDimension::CollectionItems)
    );
    drop(scheduler);
    drop(storage);
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(_))
    ));
}

#[test]
fn deferred_history_compares_long_shared_key_prefixes() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("root");
    let mut storage = ctx.reserve_scoped(0, "test key owner").expect("owner");
    let mut scheduler = DeferredCurves::new(Vec::new());
    storage
        .with_storage(|| -> Result<(), CodecError> {
            let mut a = scheduler.make_key(0, 0, None, &ctx)?;
            let mut b = scheduler.make_key(0, 1, None, &ctx)?;
            for group in 1..=256 {
                a = scheduler.make_key(group, 0, Some(a), &ctx)?;
                b = scheduler.make_key(group, 0, Some(b), &ctx)?;
                assert_eq!(scheduler.compare(a, b)?, std::cmp::Ordering::Less);
                assert_eq!(scheduler.compare(b, a)?, std::cmp::Ordering::Greater);
                assert_eq!(scheduler.compare(a, a)?, std::cmp::Ordering::Equal);
            }
            Ok(())
        })
        .expect("indexed prefix comparison");
    drop(scheduler);
    drop(storage);
    let reuse = ctx
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "test key scratch reuse",
        )
        .expect("key storage was released");
    drop(reuse);
    ctx.finish_session().expect("released key owner");
}
