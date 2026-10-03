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
impl DecodeContext {
    fn admit_sort<T>(&self, _values: &[T], _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn sorts(ctx: &DecodeContext, values: &mut [u8], other: &mut [u8], flag: bool) -> Result<(), ()> {
    ctx.admit_sort(values, "sort")?;
    values.sort_unstable();
    values.sort_unstable(); // finding: uncharged_decode_work
    ctx.admit_sort(other, "other")?;
    values.sort_unstable(); // finding: uncharged_decode_work
    if flag { ctx.admit_sort(values, "conditional")?; }
    values.sort_unstable(); // finding: uncharged_decode_work
    Ok(())
}

pub fn distinct_windows(ctx: &DecodeContext, values: &[String], other: &[String]) -> Result<(), ()> {
    ctx.charge_key(&values[..1], 1, "prefix")?;
    ctx.charge_key(other, 1, "other")?;
    let _full = values == other; // finding: uncharged_decode_work
    ctx.charge_key(&values[0], 1, "first child")?;
    ctx.charge_key(&other[0], 1, "other child")?;
    let _second = values[1] == other[1]; // finding: uncharged_decode_work
    let first = &values[0];
    let second = &other[0];
    ctx.charge_key(first, 1, "first")?;
    ctx.charge_key(second, 1, "second")?;
    let _paid = first == second;
    Ok(())
}
impl DecodeContext {
    fn admit_moves<T>(&self, _values: &[T], _moves: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn moves<T: Copy>(ctx: &DecodeContext, target: &mut [T], source: &[T]) -> Result<(), ()> {
    ctx.admit_moves(source, 1, "copy")?;
    target.copy_from_slice(source);
    target.copy_from_slice(source); // finding: uncharged_decode_work
    ctx.admit_moves(source, 3, "wrong reverse")?;
    target.reverse(); // finding: uncharged_decode_work
    ctx.admit_moves(target, 3, "reverse")?;
    target.reverse();
    Ok(())
}

pub fn replaced_child(ctx: &DecodeContext, values: &mut [String], other: &String, replacement: String) -> Result<(), ()> {
    ctx.charge_key(&values[0], 1, "child before mutation")?;
    ctx.charge_key(other, 1, "other")?;
    values[0] = replacement;
    let _stale = &values[0] == other; // finding: uncharged_decode_work
    ctx.charge_key(&values[0], 1, "child after mutation")?;
    ctx.charge_key(other, 1, "other after mutation")?;
    let _paid = &values[0] == other;
    Ok(())
}

pub fn replaced_field(ctx: &DecodeContext, values: &mut (String, String), other: &String, replacement: String) -> Result<(), ()> {
    ctx.charge_key(&values.0, 1, "field before mutation")?;
    ctx.charge_key(other, 1, "other")?;
    values.0 = replacement;
    let _stale = &values.0 == other; // finding: uncharged_decode_work
    Ok(())
}

pub fn destructured_keys(ctx: &DecodeContext, hash: &std::collections::HashMap<String, u8>, pair: (String, String), other: &String) -> Result<(), ()> {
    let (first, second) = pair;
    ctx.charge_key(&first, 1, "first key")?;
    let _wrong = hash.get(&second); // finding: uncharged_decode_work
    let _paid = hash.get(&first);
    ctx.charge_key(&first, 1, "first operand")?;
    ctx.charge_key(other, 1, "other operand")?;
    let _wrong = &second == other; // finding: uncharged_decode_work
    let Some((key, _value)) = Some((second, 0_u8)) else { return Ok(()); };
    ctx.charge_key(&key, 1, "matched key")?;
    let _paid = hash.get(&key);
    Ok(())
}

impl DecodeContext {
    fn charge_work(&self, _count: u64, _operation: &str) -> Result<(), ()> { Ok(()) }
}
pub fn loop_replaced_child(ctx: &DecodeContext, values: &mut [String], other: &String, mut replacement: String, count: usize) -> Result<(), ()> {
    ctx.charge_key(&values[0], 1, "child before loop")?;
    ctx.charge_key(other, 1, "other")?;
    for _index in 0..count {
        ctx.charge_work(1, "replace")?;
        values[0] = std::mem::take(&mut replacement);
    }
    let _stale = &values[0] == other; // finding: uncharged_decode_work
    ctx.charge_key(&values[0], 1, "child after loop")?;
    ctx.charge_key(other, 1, "other after loop")?;
    let _paid = &values[0] == other;
    Ok(())
}

pub fn dropped_suffix(ctx: &DecodeContext, values: &mut Vec<String>, other: &mut Vec<String>, length: usize, wrong_length: usize) -> Result<(), ()> {
    if let Some(removed) = values.get(length..) {
        ctx.charge_key(removed, 1, "suffix")?;
        values.truncate(length);
    }
    if let Some(removed) = values.get(length..) {
        ctx.charge_key(removed, 1, "wrong target")?;
        other.truncate(length); // finding: uncharged_decode_work
    }
    if let Some(removed) = values.get(length..) {
        ctx.charge_key(removed, 1, "wrong length")?;
        values.truncate(wrong_length); // finding: uncharged_decode_work
    }
    ctx.charge_key(&values[length..], 1, "indexed suffix")?;
    values.truncate(length);
    values.truncate(length); // finding: uncharged_decode_work
    Ok(())
}

pub fn stored_keys(ctx: &DecodeContext, hash: &mut std::collections::HashMap<String, u8>, tree: &mut std::collections::BTreeMap<String, u8>, key: &str, other: &str) -> Result<(), ()> {
    ctx.charge_key(key, 1, "borrow")?;
    let _stored = hash.get_key_value(key);
    let _reused = hash.remove_entry(key); // finding: uncharged_decode_work
    ctx.charge_key(other, 1, "wrong key")?;
    let _wrong = hash.get_key_value(key); // finding: uncharged_decode_work
    ctx.charge_key(key, ctx.tree_comparisons(tree.len()), "remove")?;
    let _removed = tree.remove_entry(key);
    let _unpaid = tree.get_key_value(key); // finding: uncharged_decode_work
    Ok(())
}
