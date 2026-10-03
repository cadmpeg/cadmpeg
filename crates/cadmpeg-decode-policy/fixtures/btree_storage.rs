// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;

impl DecodeContext {
    fn charge_key<K: ?Sized>(&self, _key: &K, _count: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
    fn tree_comparisons(&self, _length: usize) -> u64 { 1 }
    fn admit_btree_node_storage<K, V>(&self, _length: usize, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn tree_nodes<K: Ord, V>(ctx: &DecodeContext, values: &mut std::collections::BTreeMap<K, V>, key: K, value: V) -> Result<(), ()> {
    ctx.admit_btree_node_storage::<K, V>(values.len(), "nodes")?;
    ctx.charge_key(&key, ctx.tree_comparisons(values.len()), "key")?;
    values.insert(key, value);
    Ok(())
}
pub fn set_nodes<K: Ord>(ctx: &DecodeContext, values: &mut std::collections::BTreeSet<K>, key: K) -> Result<(), ()> {
    ctx.admit_btree_node_storage::<K, ()>(values.len(), "nodes")?;
    ctx.charge_key(&key, ctx.tree_comparisons(values.len()), "key")?;
    values.insert(key);
    Ok(())
}
pub fn wrong_tree_type<K: Ord, V>(ctx: &DecodeContext, values: &mut std::collections::BTreeMap<K, V>, key: K, value: V) -> Result<(), ()> {
    ctx.admit_btree_node_storage::<K, ()>(values.len(), "nodes")?;
    ctx.charge_key(&key, ctx.tree_comparisons(values.len()), "key")?;
    values.insert(key, value); // finding: unproven_decode_charge
    Ok(())
}
pub fn wrong_tree_target<K: Ord>(ctx: &DecodeContext, first: &std::collections::BTreeSet<K>, values: &mut std::collections::BTreeSet<K>, key: K) -> Result<(), ()> {
    ctx.admit_btree_node_storage::<K, ()>(first.len(), "nodes")?;
    ctx.charge_key(&key, ctx.tree_comparisons(values.len()), "key")?;
    values.insert(key); // finding: unproven_decode_charge
    Ok(())
}