// SPDX-License-Identifier: Apache-2.0
use std::collections::VecDeque;

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

fn compressed_order(dependencies: &[Vec<usize>]) -> Vec<usize> {
    let mut scheduler = DeferredCurves::new(
        (0..dependencies.len())
            .map(|i| u64::try_from(i).expect("index"))
            .collect(),
    );
    let mut done = vec![false; dependencies.len()];
    let mut visits = vec![0; dependencies.len()];
    let mut order = Vec::new();
    while let Some(node) = scheduler.next().expect("consistent scheduler") {
        let node = usize::try_from(node).expect("index");
        visits[node] += 1;
        let missing: Vec<_> = dependencies[node]
            .iter()
            .copied()
            .filter(|&p| !done[p])
            .collect();
        if missing.is_empty() {
            done[node] = true;
            order.push(node);
            scheduler
                .complete(u64::try_from(node).expect("index"))
                .expect("complete");
        } else {
            for parent in missing {
                scheduler
                    .register(
                        u64::try_from(parent).expect("index"),
                        u64::try_from(node).expect("index"),
                    )
                    .expect("register");
            }
        }
    }
    assert!(
        visits.iter().all(|&count| count <= 2),
        "each record is parsed at most twice"
    );
    order
}

#[test]
fn deferred_history_preserves_chained_partial_retry_order() {
    let dependencies = [vec![3, 4], vec![2, 3, 4], vec![3], vec![], vec![2]];
    assert_eq!(compressed_order(&dependencies), [3, 2, 4, 1, 0]);
}

#[test]
fn deferred_history_preserves_later_initial_occurrences_inside_a_wake() {
    let dependencies = [vec![1, 3], vec![3], vec![1, 3], vec![]];
    assert_eq!(compressed_order(&dependencies), [3, 1, 2, 0]);
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
                compressed_order(&dependencies),
                fifo_order(&dependencies),
                "{dependencies:?}"
            );
        }
    }
}

#[test]
fn deferred_history_chain_parses_each_record_at_most_twice() {
    for count in [8, 16, 128, 512] {
        let mut dependencies = vec![(1..=count).collect()];
        dependencies.extend((1..=count).map(|i| if i < count { vec![i + 1] } else { vec![] }));
        let expected: Vec<_> = (0..=count).rev().collect();
        assert_eq!(compressed_order(&dependencies), expected);
    }
}

#[test]
fn deferred_history_compares_long_shared_key_prefixes() {
    let mut scheduler = DeferredCurves::new(Vec::new());
    let check = || -> Result<(), CodecError> {
        let mut a = scheduler.make_key(0, 0, None)?;
        let mut b = scheduler.make_key(0, 1, None)?;
        for group in 1..=256 {
            a = scheduler.make_key(group, 0, Some(a))?;
            b = scheduler.make_key(group, 0, Some(b))?;
            assert_eq!(scheduler.compare(a, b)?, std::cmp::Ordering::Less);
            assert_eq!(scheduler.compare(b, a)?, std::cmp::Ordering::Greater);
            assert_eq!(scheduler.compare(a, a)?, std::cmp::Ordering::Equal);
        }
        Ok(())
    };
    let mut check = check;
    check().expect("indexed prefix comparison");
}
