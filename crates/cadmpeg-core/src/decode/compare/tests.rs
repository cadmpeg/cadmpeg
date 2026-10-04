// SPDX-License-Identifier: Apache-2.0
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::CodecError;

#[test]
fn charged_comparison_counts_children_and_short_circuit_search() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert!(ctx
        .equal(&vec!["ab", "c"], &vec!["ab", "c"], "equal")
        .expect("equal"));
    assert!(ctx
        .contains(&["ab", "cd"], &"ab", "contains")
        .expect("contains"));
    assert_eq!(
        ctx.compare("ab", "cd", "order").expect("order"),
        std::cmp::Ordering::Less
    );
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe")
    else {
        panic!("refusal")
    };
    // Four child visits, six equality bytes, one search visit, four search bytes, four order bytes.
    assert_eq!(limit.used, 19);
}

#[test]
fn charged_lookup_and_comparison_refuse_before_access() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let mut values = std::collections::HashMap::from([("long key", 7)]);
    let CodecError::ResourceLimit(limit) = ctx
        .remove_hash_map(&mut values, "long key", "remove")
        .expect_err("refusal")
    else {
        panic!("refusal")
    };
    assert_eq!(values.get("long key"), Some(&7));
    let CodecError::ResourceLimit(repeated) = ctx.equal("a", "b", "equal").expect_err("fused")
    else {
        panic!("refusal")
    };
    assert_eq!(limit, repeated);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn charged_tree_lookup_counts_key_bytes_at_depth_bound() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut tree = std::collections::BTreeMap::from([("ab", 1), ("cd", 2)]);
    assert_eq!(
        ctx.get_btree_map(&tree, "ab", "get").expect("get"),
        Some(&1)
    );
    *ctx.get_mut_btree_map(&mut tree, "cd", "get mut")
        .expect("get mut")
        .expect("entry") = 3;
    assert_eq!(
        ctx.remove_btree_map(&mut tree, "ab", "remove")
            .expect("remove"),
        Some(1)
    );
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe")
    else {
        panic!("refusal")
    };
    // Three lookups each compare a two-byte key with both stored keys; the
    // removal makes four mutation passes over the root and a possible new root.
    let node_bytes = 11 * (std::mem::size_of::<&str>() + std::mem::size_of::<i32>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<usize>();
    assert_eq!(
        limit.used,
        3 * 2 * 2 + 4 * 2 * u64::try_from(node_bytes).expect("test operation succeeds")
    );
}

#[test]
fn charged_hash_refuses_before_the_hash_callback() {
    struct Key<'a> {
        bytes: &'a str,
        calls: &'a std::cell::Cell<usize>,
    }
    impl std::hash::Hash for Key<'_> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.calls.set(self.calls.get() + 1);
            self.bytes.hash(state);
        }
    }
    impl crate::decode::cost::DecodeCost for Key<'_> {
        fn decode_cost(
            &self,
            _ctx: &DecodeContext<'_>,
            _operation: &'static str,
        ) -> Result<u64, CodecError> {
            Ok(crate::decode::u64_from_index(self.bytes.len())
                + crate::decode::u64_from_index(std::mem::size_of::<usize>()))
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let calls = std::cell::Cell::new(0);
    let key = Key {
        bytes: "key",
        calls: &calls,
    };
    let CodecError::ResourceLimit(first) = ctx.hash_value(&key, "hash").expect_err("refusal")
    else {
        panic!("refusal")
    };
    let CodecError::ResourceLimit(second) =
        ctx.hash_value(&key, "again").expect_err("fused refusal")
    else {
        panic!("refusal")
    };
    assert_eq!(first, second);
    assert_eq!(calls.get(), 0);
}
#[test]
fn charged_set_get_borrows_the_stored_variable_size_key() {
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let hash = std::collections::HashSet::from([String::from("stored")]);
    let tree = std::collections::BTreeSet::from([String::from("stored")]);
    assert_eq!(
        ctx.get_hash_set(&hash, "stored", "get")
            .expect("admission")
            .map(String::as_str),
        Some("stored")
    );
    assert_eq!(
        ctx.get_btree_set(&tree, "stored", "get")
            .expect("admission")
            .map(String::as_str),
        Some("stored")
    );
}

#[test]
fn set_relations_and_stored_map_keys_use_complete_query_work() {
    use std::collections::{BTreeMap, BTreeSet, HashMap};
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let left = BTreeSet::from([String::from("alpha")]);
    let right = BTreeSet::from([String::from("beta")]);
    assert!(ctx
        .is_disjoint_btree_set(&left, &right, "tree relation")
        .expect("admission"));
    assert!(!ctx
        .is_subset_btree_set(&left, &right, "tree relation")
        .expect("admission"));
    let mut hash = HashMap::from([(String::from("alpha"), 1)]);
    let mut tree = BTreeMap::from([(String::from("alpha"), 2)]);
    assert_eq!(
        ctx.get_key_value_hash_map(&hash, "alpha", "stored")
            .expect("admission")
            .map(|(key, value)| (key.as_str(), *value)),
        Some(("alpha", 1))
    );
    assert_eq!(
        ctx.get_key_value_btree_map(&tree, "alpha", "stored")
            .expect("admission")
            .map(|(key, value)| (key.as_str(), *value)),
        Some(("alpha", 2))
    );
    assert_eq!(
        ctx.remove_entry_hash_map(&mut hash, "alpha", "remove")
            .expect("admission"),
        Some((String::from("alpha"), 1))
    );
    assert_eq!(
        ctx.remove_entry_btree_map(&mut tree, "alpha", "remove")
            .expect("admission"),
        Some((String::from("alpha"), 2))
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(first) = ctx
        .is_disjoint_btree_set(&left, &right, "refuse relation")
        .expect_err("work")
    else {
        panic!("refusal")
    };
    let CodecError::ResourceLimit(repeated) = ctx
        .remove_entry_hash_map(&mut hash, "alpha", "refuse remove")
        .expect_err("fused")
    else {
        panic!("refusal")
    };
    assert_eq!(first, repeated);
}

#[test]
fn tree_mutation_refuses_inline_moves_before_insertion_or_removal() {
    use std::collections::{BTreeMap, BTreeSet};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
    let mut empty = BTreeMap::<u8, [u8; 4096]>::new();
    assert!(matches!(
        ctx.insert_btree_map(&mut empty, 1, [0; 4096], "insert slots"),
        Err(CodecError::ResourceLimit(_))
    ));
    assert!(empty.is_empty());

    let mut insertion_policy = DecodePolicy::service();
    insertion_policy.limits.max_work_units = 22;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &insertion_policy)
        .expect("test operation succeeds");
    let mut insertion = BTreeMap::<u8, [u8; 4096]>::from([(1, [0; 4096])]);
    // The one-comparison key lookup fits; movement work must refuse.
    let CodecError::ResourceLimit(insert_refusal) = ctx
        .insert_btree_map(&mut insertion, 2, [0; 4096], "insert without node growth")
        .expect_err("movement work refuses before insertion")
    else {
        panic!("resource refusal")
    };
    assert_eq!(insert_refusal.used, 1);
    assert!(insert_refusal.additional > 4096);
    assert_eq!(insertion.len(), 1);
    assert_eq!(insertion.get(&1), Some(&[0; 4096]));
    assert!(!insertion.contains_key(&2));

    policy.limits.max_work_units = 11;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
    let mut map = BTreeMap::from([(1_u8, [0_u8; 4096])]);
    let CodecError::ResourceLimit(first) = ctx
        .remove_btree_map(&mut map, &1, "remove slots")
        .expect_err("operation refuses")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first.used, 1);
    assert!(first.additional > 4096);
    assert_eq!(map.len(), 1);
    let CodecError::ResourceLimit(repeated) = ctx
        .remove_entry_btree_map(&mut map, &1, "remove entry slots")
        .expect_err("operation refuses")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first, repeated);

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("test operation succeeds");
    let mut set = BTreeSet::from([1_u8]);
    assert!(matches!(
        ctx.remove_btree_set(&mut set, &1, "remove set slots"),
        Err(CodecError::ResourceLimit(_))
    ));
    assert_eq!(set.len(), 1);
}

#[test]
fn tree_height_counts_levels_of_a_minimally_filled_btree() {
    // A tree of h levels holds at least 2 * 6^(h-1) - 1 entries.
    for (length, height) in [
        (0, 0),
        (1, 1),
        (10, 1),
        (11, 2),
        (70, 2),
        (71, 3),
        (1_000_000, 8),
    ] {
        assert_eq!(DecodeContext::tree_height(length), height, "{length}");
    }
    assert_eq!(DecodeContext::tree_comparisons(3), 3);
    assert_eq!(DecodeContext::tree_comparisons(1_000_000), 88);
}
