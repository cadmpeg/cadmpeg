// SPDX-License-Identifier: Apache-2.0
pub mod decode {
    pub mod cost {
        pub trait DecodeCost {}
        impl<T: ?Sized> DecodeCost for T {}
    }
}
pub struct DecodeContext;
impl DecodeContext {
    fn charge_key<T: decode::cost::DecodeCost + ?Sized>(&self, _key: &T, _count: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
    fn tree_comparisons(&self, _length: usize) -> u64 { 1 }
}
pub fn decode(ctx: &DecodeContext, hash: &std::collections::HashMap<Vec<String>, u8>, tree: &std::collections::BTreeMap<Vec<String>, u8>, other_tree: &std::collections::BTreeMap<Vec<String>, u8>, key: &Vec<String>, other: &Vec<String>, flag: bool) -> Result<(), ()> {
    ctx.charge_key(key, 1, "hash")?;
    let _paid = hash.get(key);
    let _reuse = hash.get(key); // finding: uncharged_decode_work
    ctx.charge_key(other, 1, "unrelated")?;
    let _wrong = hash.get(key); // finding: uncharged_decode_work
    let _other = hash.get(other);
    ctx.charge_key(key, ctx.tree_comparisons(tree.len()), "tree")?;
    let _tree = tree.get(key);
    ctx.charge_key(key, ctx.tree_comparisons(other_tree.len()), "wrong tree")?;
    let _wrong_tree = tree.get(key); // finding: uncharged_decode_work
    if flag { ctx.charge_key(key, 1, "conditional")?; }
    let _conditional = hash.get(key); // finding: uncharged_decode_work
    ctx.charge_key(key, 1, "first operand")?;
    ctx.charge_key(other, 1, "second operand")?;
    let _equal = key == other;
    let _reused = key == other; // finding: uncharged_decode_work
    ctx.charge_key(key, 1, "first only")?;
    let _partial = key == other; // finding: uncharged_decode_work
    Ok(())
}
