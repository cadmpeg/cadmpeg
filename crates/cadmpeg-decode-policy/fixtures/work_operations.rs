// SPDX-License-Identifier: Apache-2.0
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
pub struct DecodeContext;
impl DecodeContext {
    pub fn charge_work(&self, _n: u64, _operation: &str) -> Result<(), ()> {
        Ok(())
    }
}
#[derive(Clone, Copy, PartialEq)]
pub struct Fixed {
    pub x: u32,
    pub y: u32,
}
pub struct Heap {
    pub id: u32,
    pub text: String,
}
impl PartialEq for Heap {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
pub fn decode(
    ctx: &DecodeContext,
    bytes: &[u8],
    other: &[u8],
    target: &mut [u8],
    n: usize,
    record: &Heap,
) -> Result<(), ()> {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "hash")?;
    bytes.hash(&mut hasher);
    4u32.hash(&mut hasher);
    let _hash = hasher.finish();
    target.copy_from_slice(bytes); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "copy")?;
    target.copy_from_slice(bytes);
    let _search = bytes.iter().take(n).find(|b| **b == 1); // finding: uncharged_decode_work
    let _compare = bytes.cmp(other); // finding: uncharged_decode_work
    let _eq = bytes.eq(other); // finding: uncharged_decode_work
    ctx.charge_work(bytes.len() as u64, "compare")?;
    let _paid = bytes == other;
    let _arrays = [1u8, 2, 3] == [4u8, 5, 6];
    let _fixed = Fixed { x: 1, y: 2 } == Fixed { x: 3, y: 4 };
    let _custom = record == record;
    let _array_search = [1u8, 2, 3].iter().any(|b| *b == 1);
    let strings = ["a", "b"];
    let variable_text = std::str::from_utf8(bytes).unwrap_or(""); // finding: uncharged_decode_work
    let _string_search = strings.iter().any(|text| *text == variable_text); // finding: uncharged_decode_work
    Ok(())
}
