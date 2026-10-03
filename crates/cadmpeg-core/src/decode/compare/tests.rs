// SPDX-License-Identifier: Apache-2.0
use crate::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::CodecError;

#[test]
fn charged_comparison_counts_children_and_short_circuit_search() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    assert!(ctx.equal(&vec!["ab", "c"], &vec!["ab", "c"], "equal").expect("equal"));
    assert!(ctx.contains(&["ab", "cd"], &"ab", "contains").expect("contains"));
    assert_eq!(ctx.compare("ab", "cd", "order").expect("order"), std::cmp::Ordering::Less);
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe") else { panic!("refusal") };
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
    let CodecError::ResourceLimit(limit) = ctx.remove_hash_map(&mut values, "long key", "remove").expect_err("refusal") else { panic!("refusal") };
    assert_eq!(values.get("long key"), Some(&7));
    let CodecError::ResourceLimit(repeated) = ctx.equal("a", "b", "equal").expect_err("fused") else { panic!("refusal") };
    assert_eq!(limit, repeated);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}

#[test]
fn charged_tree_lookup_counts_key_bytes_at_depth_bound() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let mut tree = std::collections::BTreeMap::from([("ab", 1), ("cd", 2)]);
    assert_eq!(ctx.get_btree_map(&tree, "ab", "get").expect("get"), Some(&1));
    *ctx.get_mut_btree_map(&mut tree, "cd", "get mut").expect("get mut").expect("entry") = 3;
    assert_eq!(ctx.remove_btree_map(&mut tree, "ab", "remove").expect("remove"), Some(1));
    let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "probe").expect_err("probe") else { panic!("refusal") };
    // Three two-byte keys, eleven comparisons per node, two nodes in the depth bound.
    assert_eq!(limit.used, 132);
}

#[test]
fn charged_hash_refuses_before_the_hash_callback() {
    struct Key<'a> { bytes: &'a str, calls: &'a std::cell::Cell<usize> }
    impl std::hash::Hash for Key<'_> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.calls.set(self.calls.get() + 1);
            self.bytes.hash(state);
        }
    }
    impl crate::decode::cost::DecodeCost for Key<'_> {
        fn decode_cost(&self, _ctx: &DecodeContext<'_>, _operation: &'static str) -> Result<u64, CodecError> {
            Ok(crate::decode::u64_from_index(self.bytes.len()) + crate::decode::u64_from_index(std::mem::size_of::<usize>()))
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let calls = std::cell::Cell::new(0);
    let key = Key { bytes: "key", calls: &calls };
    let CodecError::ResourceLimit(first) = ctx.hash_value(&key, "hash").expect_err("refusal") else { panic!("refusal") };
    let CodecError::ResourceLimit(second) = ctx.hash_value(&key, "again").expect_err("fused refusal") else { panic!("refusal") };
    assert_eq!(first, second);
    assert_eq!(calls.get(), 0);
}
#[test]
fn charged_set_get_borrows_the_stored_variable_size_key() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).expect("context");
    let hash = std::collections::HashSet::from([String::from("stored")]);
    let tree = std::collections::BTreeSet::from([String::from("stored")]);
    assert_eq!(ctx.get_hash_set(&hash, "stored", "get").expect("admission").map(String::as_str), Some("stored"));
    assert_eq!(ctx.get_btree_set(&tree, "stored", "get").expect("admission").map(String::as_str), Some("stored"));
}
