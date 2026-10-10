// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::BTreeMap;

std::thread_local! {
    static COMPARISONS: Cell<u64> = const { Cell::new(0) };
}

#[derive(Clone)]
struct CountedKey {
    text: String,
}

impl PartialEq for CountedKey {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for CountedKey {}

impl PartialOrd for CountedKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CountedKey {
    fn cmp(&self, other: &Self) -> Ordering {
        COMPARISONS.set(COMPARISONS.get() + 1);
        self.text.cmp(&other.text)
    }
}

fn key(index: usize) -> CountedKey {
    CountedKey {
        text: format!("test:model:point#counted-tree-search-{index:08}"),
    }
}

fn admitted_work(entries: usize, bytes: usize) -> u64 {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, false);
    super::super::admit_identity_work(&ctx, entries, bytes, 1, "test tree search")
        .expect("bounded tree search");
    ctx.charge_work_limit(u64::MAX, "measure admitted search")
        .expect_err("measurement exceeds the unchanged ceiling")
        .used
}

fn check_searches(map: &BTreeMap<CountedKey, bool>, count: usize) {
    for index in [0, 1, count, count * 2, count * 2 + 1] {
        let query = key(index);
        let expected = map.keys().find(|key| key.text == query.text).map(|_| true);
        let key_work = u64::try_from(query.text.len()).expect("short test key") + 1;
        let work = admitted_work(map.len(), query.text.len());
        COMPARISONS.set(0);
        assert_eq!(map.get(&query).copied(), expected);
        let observed = COMPARISONS.get() * key_work;
        assert!(
            work >= observed,
            "search: {count} entries, {index}: {work} < {observed}"
        );

        let mut changed = map.clone();
        COMPARISONS.set(0);
        assert_eq!(changed.insert(query.clone(), true), expected);
        let observed = COMPARISONS.get() * key_work;
        assert!(
            work >= observed,
            "insert: {count} entries, {index}: {work} < {observed}"
        );

        let mut changed = map.clone();
        COMPARISONS.set(0);
        assert_eq!(changed.remove(&query), expected);
        let observed = COMPARISONS.get() * key_work;
        assert!(
            work >= observed,
            "remove: {count} entries, {index}: {work} < {observed}"
        );
    }
}

#[test]
fn admitted_search_bounds_cover_standard_library_comparisons_after_growth_and_deletion() {
    for count in [0, 1, 5, 10, 11, 12, 70, 71, 72, 4096, 32768] {
        for reverse in [false, true] {
            let mut map = BTreeMap::new();
            for position in 0..count {
                let index = if reverse {
                    count - position - 1
                } else {
                    position
                };
                assert!(map.insert(key(index * 2), true).is_none());
            }
            check_searches(&map, count);
            for index in (0..count).step_by(2) {
                assert!(map.remove(&key(index * 2)).is_some());
            }
            check_searches(&map, count);
        }
    }
}

#[test]
fn unrepresentable_comparison_work_refuses_and_preserves_the_first_refusal() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let ctx = DecodeContext::new(&arena, &policy, false);
    let error = super::super::admit_identity_work(
        &ctx,
        usize::MAX,
        usize::MAX,
        u64::MAX,
        "test comparison overflow",
    )
    .expect_err("unrepresentable comparison work refuses");
    assert_eq!(
        ctx.finish_session()
            .expect_err("sticky refusal")
            .to_string(),
        error.to_string()
    );
}
