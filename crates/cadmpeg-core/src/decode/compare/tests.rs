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
