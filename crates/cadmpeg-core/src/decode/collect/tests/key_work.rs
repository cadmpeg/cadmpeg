// SPDX-License-Identifier: Apache-2.0
use super::operation_context;
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;
use std::cell::Cell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

#[derive(Eq, PartialEq)]
struct ObservedKey {
    text: String,
    hashes: Rc<Cell<usize>>,
}

impl Hash for ObservedKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hashes.set(self.hashes.get() + 1);
        self.text.hash(state);
    }
}

impl crate::decode::cost::DecodeCost for ObservedKey {
    fn decode_cost(
        &self,
        _ctx: &DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, CodecError> {
        Ok(crate::decode::u64_from_index(self.text.len()))
    }
}

#[test]
#[allow(
    clippy::mutable_key_type,
    reason = "The counter observes hashing without changing the key text."
)]
fn key_work_refuses_before_hashing_or_rehash_growth() {
    let hashes = Rc::new(Cell::new(0));
    let mut map = HashMap::new();
    for text in ["long-a", "long-b", "long-c"] {
        map.insert(
            ObservedKey {
                text: text.into(),
                hashes: Rc::clone(&hashes),
            },
            1,
        );
    }
    assert_eq!(map.len(), map.capacity());
    hashes.set(0);
    let old_capacity = map.capacity();
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::WorkUnits, 6);
    let CodecError::ResourceLimit(refusal) = ctx
        .reserve_map(&mut map, 1, "grow")
        .expect_err("child hash work")
    else {
        panic!("resource refusal")
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(hashes.get(), 0);
    assert_eq!(map.capacity(), old_capacity);
    assert_eq!(map.len(), 3);
    let key = ObservedKey {
        text: "next".into(),
        hashes: Rc::clone(&hashes),
    };
    let CodecError::ResourceLimit(repeated) = ctx
        .insert_hash_map(&mut map, key, 2, "insert")
        .expect_err("fused refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(repeated, refusal);
    assert_eq!(hashes.get(), 0);
}

#[test]
fn collection_keys_charge_owned_children_before_lookup() {
    let arena = DecodeArena::new();
    let ctx = operation_context(&arena, ResourceDimension::WorkUnits, 1);
    let mut map = HashMap::<Vec<String>, u8>::new();
    let CodecError::ResourceLimit(limit) = ctx
        .insert_hash_map(&mut map, vec!["a".into(), "b".into()], 1, "nested key")
        .expect_err("child visits")
    else {
        panic!("resource refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert!(map.is_empty());
    assert_eq!(map.capacity(), 0);
}

#[test]
fn rehash_charges_stored_visits_and_key_bytes() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut map = HashMap::from([("ab", 1), ("cd", 2), ("ef", 3)]);
    ctx.reserve_map(&mut map, 1, "grow").expect("growth");
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe")
    else {
        panic!("resource refusal")
    };
    // Three rehash visits, three measuring visits, six key bytes and the old bucket storage move.
    let old_storage = 4 * std::mem::size_of::<(&str, i32)>() + 15 + 4 + 16;
    assert_eq!(
        limit.used,
        12 + u64::try_from(old_storage).expect("test operation succeeds")
    );
}

#[test]
fn scoped_borrowed_keys_charge_lookup_and_preserve_prepaid_slots() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let (set, set_storage) = ctx
        .collect_scoped_string_set(2, ["long", "long", "other"], "set")
        .expect("set");
    assert_eq!(set.len(), 2);
    let (map, map_storage) = ctx
        .collect_scoped_string_map(2, [("long", 1), ("long", 2), ("other", 3)], "map")
        .expect("map");
    assert_eq!(map["long"], 2);
    assert_eq!(map.len(), 2);
    let CodecError::ResourceLimit(limit) = ctx
        .charge_collection_items(u64::MAX, "probe")
        .expect_err("probe")
    else {
        panic!("resource refusal")
    };
    // Two prepaid set slots and two prepaid map slots; duplicate keys add no slots.
    assert_eq!(limit.used, 4);
    drop((set_storage, map_storage));

    let refusal_ctx = operation_context(&arena, ResourceDimension::WorkUnits, 1);
    let CodecError::ResourceLimit(first) = refusal_ctx
        .collect_scoped_string_set(1, ["long"], "refuse set")
        .expect_err("key work")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    let CodecError::ResourceLimit(repeated) = refusal_ctx
        .collect_scoped_string_map(1, [("long", 1)], "refuse map")
        .expect_err("fused refusal")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first, repeated);
}

#[test]
#[allow(
    clippy::mutable_key_type,
    reason = "The counter observes hashing without changing the key text."
)]
fn hash_growth_refuses_inline_slot_moves_before_rehashing() {
    let hashes = Rc::new(Cell::new(0));
    let mut map = HashMap::new();
    for text in ["a", "b", "c"] {
        map.insert(
            ObservedKey {
                text: text.into(),
                hashes: Rc::clone(&hashes),
            },
            [0_u8; 4096],
        );
    }
    hashes.set(0);
    let capacity = map.capacity();
    let arena = DecodeArena::new();
    // Bucket movement is charged before any key is rehashed or measured.
    let ctx = operation_context(&arena, ResourceDimension::WorkUnits, 9);
    let CodecError::ResourceLimit(limit) = ctx
        .reserve_map(&mut map, 1, "large slots")
        .expect_err("operation refuses")
    else {
        panic!("resource refusal")
    };
    assert_eq!(limit.used, 0);
    assert!(limit.additional > 3 * 4096);
    assert_eq!(hashes.get(), 0);
    assert_eq!(map.capacity(), capacity);
    assert_eq!(map.len(), 3);
}

/// Returns the work one insertion charges into a collection of `len` keys.
fn insertion_work<C>(
    len: u32,
    new: impl Fn() -> C,
    insert: impl Fn(&DecodeContext<'_>, &mut C, u32) -> bool,
) -> u64 {
    let used = |count: u32| {
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
        let mut values = new();
        for value in 0..count {
            assert!(insert(&ctx, &mut values, value));
        }
        let CodecError::ResourceLimit(limit) =
            ctx.charge_work(u64::MAX, "probe").expect_err("probe")
        else {
            panic!("resource refusal")
        };
        limit.used
    };
    used(len + 1) - used(len)
}

#[test]
fn set_insertion_work_does_not_grow_with_the_stored_length() {
    use std::collections::{BTreeSet, HashSet};
    // Both lengths have the same B-tree depth bound, so equal work shows that
    // no charge is proportional to the number of stored keys.
    let btree = |len| {
        insertion_work(len, BTreeSet::new, |ctx, values, value| {
            ctx.insert_btree_set(values, value, "insert")
                .expect("insertion fits")
        })
    };
    assert_eq!(btree(1024), btree(2046));
    let scoped = |len| {
        insertion_work(len, BTreeSet::new, |ctx, values, value| {
            let mut storage = ctx.reserve_scoped(0, "scope").expect("scope");
            ctx.insert_scoped_btree_set(&mut storage, values, value, "lookup", "insert")
                .expect("insertion fits")
        })
    };
    assert_eq!(scoped(1024), scoped(2046));
    let groups = |len| {
        insertion_work(len, std::collections::BTreeMap::new, |ctx, groups, key| {
            let mut storage = ctx.reserve_scoped(0, "scope").expect("scope");
            ctx.push_scoped_btree_group(&mut storage, groups, key, || (), 0, "group")
                .expect("insertion fits");
            true
        })
    };
    assert_eq!(groups(1024), groups(2046));
    let collected = |len: u32| {
        let used = |count: u32| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
                .expect("context");
            let (map, storage) = ctx
                .collect_scoped_btree_map((0..count).map(|key| (key, ())), "collect")
                .expect("collection fits");
            drop((map, storage));
            let CodecError::ResourceLimit(limit) =
                ctx.charge_work(u64::MAX, "probe").expect_err("probe")
            else {
                panic!("resource refusal")
            };
            limit.used
        };
        used(len + 1) - used(len)
    };
    assert_eq!(collected(1024), collected(2046));
    // Capacity for every key is reserved first, so no insertion grows the table.
    let hash = |len| {
        insertion_work(
            len,
            || HashSet::with_capacity(4096),
            |ctx, values, value| {
                ctx.insert_hash_set(values, value, "insert")
                    .expect("insertion fits")
            },
        )
    };
    assert_eq!(hash(1024), hash(2046));
}
