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
    // Three key lookups plus four mutation passes over three bounded tree nodes.
    let node_bytes = 11 * (std::mem::size_of::<&str>() + std::mem::size_of::<i32>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<usize>();
    assert_eq!(
        limit.used,
        132 + 4 * 3 * u64::try_from(node_bytes).expect("test operation succeeds")
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
    use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let left = HashSet::from([String::from("alpha")]);
    let right = HashSet::from([String::from("alpha"), String::from("beta")]);
    assert!(!ctx
        .is_disjoint_hash_set(&left, &right, "hash relation")
        .expect("admission"));
    assert!(ctx
        .is_subset_hash_set(&left, &right, "hash relation")
        .expect("admission"));
    assert!(!ctx
        .is_subset_hash_set(&right, &left, "hash relation")
        .expect("length rejection"));
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
    // The key lookup and stored-key admission fit; movement work must refuse.
    let CodecError::ResourceLimit(insert_refusal) = ctx
        .insert_btree_map(&mut insertion, 2, [0; 4096], "insert without node growth")
        .expect_err("movement work refuses before insertion")
    else {
        panic!("resource refusal")
    };
    assert_eq!(insert_refusal.used, 11);
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
    assert_eq!(first.used, 11);
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
fn hash_set_equality_charges_collision_bound_and_preserves_contents() {
    use crate::decode::cost::DecodeCost;
    use std::collections::HashSet;
    use std::hash::{BuildHasherDefault, Hash, Hasher};
    use std::rc::Rc;

    #[derive(Default)]
    struct ConstantHasher;
    impl Hasher for ConstantHasher {
        fn finish(&self) -> u64 {
            0
        }

        fn write(&mut self, _bytes: &[u8]) {}
    }

    struct Key {
        value: u8,
        cost: u64,
        hashes: Rc<std::cell::Cell<usize>>,
        comparisons: Rc<std::cell::Cell<usize>>,
    }
    impl Hash for Key {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.hashes.set(self.hashes.get() + 1);
            self.value.hash(state);
        }
    }
    impl PartialEq for Key {
        fn eq(&self, other: &Self) -> bool {
            self.comparisons.set(self.comparisons.get() + 1);
            self.value == other.value
        }
    }
    impl Eq for Key {}
    impl DecodeCost for Key {
        fn decode_cost(
            &self,
            _ctx: &DecodeContext<'_>,
            _operation: &'static str,
        ) -> Result<u64, CodecError> {
            Ok(self.cost)
        }
    }
    fn key(
        value: u8,
        cost: u64,
        hashes: &Rc<std::cell::Cell<usize>>,
        comparisons: &Rc<std::cell::Cell<usize>>,
    ) -> Key {
        Key {
            value,
            cost,
            hashes: Rc::clone(hashes),
            comparisons: Rc::clone(comparisons),
        }
    }

    let hashes = Rc::new(std::cell::Cell::new(0));
    let comparisons = Rc::new(std::cell::Cell::new(0));
    type Set = HashSet<Key, BuildHasherDefault<ConstantHasher>>;
    let mut left = Set::with_hasher(BuildHasherDefault::default());
    left.insert(key(1, 3, &hashes, &comparisons));
    left.insert(key(2, 4, &hashes, &comparisons));
    let mut right = Set::with_hasher(BuildHasherDefault::default());
    right.insert(key(1, 2, &hashes, &comparisons));
    right.insert(key(2, 8, &hashes, &comparisons));
    hashes.set(0);
    comparisons.set(0);

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert!(ctx
        .equal_hash_set(&left, &right, "set equality")
        .expect("admitted equality"));
    assert_eq!(hashes.get(), left.len());
    assert!(comparisons.get() >= left.len());
    assert!(comparisons.get() <= left.len() * right.len());

    let CodecError::ResourceLimit(limit) = ctx
        .charge_work(u64::MAX, "probe")
        .expect_err("work was charged")
    else {
        panic!("resource refusal")
    };
    let bucket_visits =
        u64::try_from(left.capacity() + right.capacity()).expect("test capacities fit work units");
    // For each source key: one hash plus two comparisons against the largest target key.
    assert_eq!(limit.used, bucket_visits + 25 + 28);

    let empty_left = HashSet::<String>::new();
    let empty_right = HashSet::<String>::new();
    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert!(ctx
        .equal_hash_set(&empty_left, &empty_right, "empty set equality")
        .expect("empty sets are equal"));
    let CodecError::ResourceLimit(empty_limit) = ctx
        .charge_work(u64::MAX, "empty probe")
        .expect_err("zero-work equality leaves no work")
    else {
        panic!("resource refusal")
    };
    assert_eq!(empty_limit.used, 0);
}

#[test]
fn hash_set_equality_refuses_before_hash_or_comparison_and_checks_length_first() {
    use crate::decode::cost::DecodeCost;
    use std::collections::HashSet;
    use std::hash::{BuildHasherDefault, Hash, Hasher};
    use std::rc::Rc;

    #[derive(Default)]
    struct ConstantHasher;
    impl Hasher for ConstantHasher {
        fn finish(&self) -> u64 {
            0
        }

        fn write(&mut self, _bytes: &[u8]) {}
    }

    struct Key {
        value: u8,
        hashes: Rc<std::cell::Cell<usize>>,
        comparisons: Rc<std::cell::Cell<usize>>,
    }
    impl Hash for Key {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.hashes.set(self.hashes.get() + 1);
            self.value.hash(state);
        }
    }
    impl PartialEq for Key {
        fn eq(&self, other: &Self) -> bool {
            self.comparisons.set(self.comparisons.get() + 1);
            self.value == other.value
        }
    }
    impl Eq for Key {}
    impl DecodeCost for Key {
        fn decode_cost(
            &self,
            _ctx: &DecodeContext<'_>,
            _operation: &'static str,
        ) -> Result<u64, CodecError> {
            Ok(1)
        }
    }
    fn key(
        value: u8,
        hashes: &Rc<std::cell::Cell<usize>>,
        comparisons: &Rc<std::cell::Cell<usize>>,
    ) -> Key {
        Key {
            value,
            hashes: Rc::clone(hashes),
            comparisons: Rc::clone(comparisons),
        }
    }

    type Set = HashSet<Key, BuildHasherDefault<ConstantHasher>>;
    let hashes = Rc::new(std::cell::Cell::new(0));
    let comparisons = Rc::new(std::cell::Cell::new(0));
    let mut left = Set::with_hasher(BuildHasherDefault::default());
    left.insert(key(1, &hashes, &comparisons));
    let mut right = Set::with_hasher(BuildHasherDefault::default());
    right.insert(key(1, &hashes, &comparisons));
    hashes.set(0);
    comparisons.set(0);

    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units =
        u64::try_from(left.capacity() + right.capacity()).expect("test capacities fit work units");
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let CodecError::ResourceLimit(first) = ctx
        .equal_hash_set(&left, &right, "set equality")
        .expect_err("lookup work refuses")
    else {
        panic!("resource refusal")
    };
    assert_eq!(hashes.get(), 0);
    assert_eq!(comparisons.get(), 0);

    let empty = Set::with_hasher(BuildHasherDefault::default());
    let CodecError::ResourceLimit(repeated) = ctx
        .equal_hash_set(&left, &empty, "unequal length after refusal")
        .expect_err("sticky refusal precedes length result")
    else {
        panic!("resource refusal")
    };
    assert_eq!(first, repeated);

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("unlimited context");
    assert!(!ctx
        .equal_hash_set(&left, &empty, "unequal length")
        .expect("length check precedes budget admission"));
    assert_eq!(hashes.get(), 0);
    assert_eq!(comparisons.get(), 0);

    let mismatch_hashes = Rc::new(std::cell::Cell::new(0));
    let mismatch_comparisons = Rc::new(std::cell::Cell::new(0));
    let mut same_length_left = Set::with_hasher(BuildHasherDefault::default());
    same_length_left.insert(key(1, &mismatch_hashes, &mismatch_comparisons));
    same_length_left.insert(key(2, &mismatch_hashes, &mismatch_comparisons));
    let mut same_length_right = Set::with_hasher(BuildHasherDefault::default());
    same_length_right.insert(key(3, &mismatch_hashes, &mismatch_comparisons));
    same_length_right.insert(key(4, &mismatch_hashes, &mismatch_comparisons));
    mismatch_hashes.set(0);
    mismatch_comparisons.set(0);
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert!(!ctx
        .equal_hash_set(&same_length_left, &same_length_right, "unequal contents")
        .expect("admitted unequal comparison"));
    assert_eq!(mismatch_hashes.get(), 1);
    assert!(mismatch_comparisons.get() > 0);
}

#[test]
fn hash_set_equality_projects_direct_and_optional_sets() {
    use std::collections::HashSet;

    let arena = DecodeArena::new();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let direct_left = HashSet::from([String::from("member")]);
    let direct_right = HashSet::from([String::from("member")]);
    let optional_left = Some(HashSet::from([String::from("member")]));
    let optional_right = Some(HashSet::from([String::from("member")]));
    let absent: Option<HashSet<String>> = None;
    let absent_again: Option<HashSet<String>> = None;
    let present_empty = Some(HashSet::<String>::new());

    assert!(ctx
        .equal_hash_set(&direct_left, &optional_right, "direct optional equality")
        .expect("direct and optional sets compare"));
    assert!(ctx
        .equal_hash_set(&optional_left, &direct_right, "optional direct equality")
        .expect("optional and direct sets compare"));
    assert!(ctx
        .equal_hash_set(&absent, &absent_again, "absent equality")
        .expect("absent options compare"));
    assert!(!ctx
        .equal_hash_set(&absent, &optional_right, "absent left")
        .expect("absent and present sets differ"));
    assert!(!ctx
        .equal_hash_set(&optional_left, &absent, "absent right")
        .expect("present and absent sets differ"));
    assert!(!ctx
        .equal_hash_set(&absent, &present_empty, "absent versus empty")
        .expect("absent differs from present empty set"));
}
