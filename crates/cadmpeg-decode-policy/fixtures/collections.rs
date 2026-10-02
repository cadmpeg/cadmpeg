// SPDX-License-Identifier: Apache-2.0
use std::collections::{HashMap, HashSet, BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::Arc;
pub struct DecodeContext;
impl DecodeContext {
    pub fn copy_slice(&self, values: &[u8]) -> Vec<u8> { values.to_vec() } // finding: uncharged_decode_allocation
    pub fn alloc_filled<T: Clone>(&self, n: usize, value: T) -> Vec<T> { vec![value; n] } // finding: uncharged_decode_allocation
}
#[derive(Clone)]
pub struct Record { pub values: Vec<u8> }
pub fn decode(ctx: &DecodeContext, bytes: &[u8], text: &str, n: usize, record: &Record, shared: Rc<String>, atomic: Arc<Vec<u8>>) {
    let _ctx = ctx;
    let _a = bytes.to_vec(); // finding: uncharged_decode_allocation
    let _b: Vec<_> = bytes.iter().copied().collect(); // finding: uncharged_decode_allocation
    let _c: String = text.into(); // finding: uncharged_decode_allocation
    let _d = String::from(text); // finding: uncharged_decode_allocation
    let _e: Box<[u8]> = bytes.into(); // finding: uncharged_decode_allocation
    let _f = vec![0; n]; // finding: uncharged_decode_allocation
    let _g = record.clone(); // finding: uncharged_decode_allocation
    let _h: HashSet<_> = bytes.iter().copied().collect(); // finding: uncharged_decode_allocation
    let _i: BTreeSet<_> = bytes.iter().copied().collect(); // finding: uncharged_decode_allocation
    let _j: HashMap<_, _> = bytes.iter().map(|b| (*b, *b)).collect(); // finding: uncharged_decode_allocation
    let _k: BTreeMap<_, _> = bytes.iter().map(|b| (*b, *b)).collect(); // finding: uncharged_decode_allocation
    let _l = Vec::<u8>::with_capacity(n); // finding: uncharged_decode_allocation
    let mut values = Vec::new();
    values.extend(bytes); // finding: uncharged_decode_allocation
    values.push(0); // finding: uncharged_decode_allocation
    let _new = String::new();
    let _empty = Vec::<u8>::new();
    let _map = HashMap::<u8, u8>::new();
    let _set = BTreeSet::<u8>::new();
    let _fixed = vec![1, 2, 3];
    let _fixed_repeat = vec![0; 4];
    let _fixed_collect: Vec<_> = [1, 2, 3].into_iter().collect();
    let _fixed_range: Vec<_> = (0..4).collect();
    let _count = bytes.iter().count();
    let _copy = ctx.copy_slice(bytes);
    let _filled = ctx.alloc_filled(n, 0u8);
    let child = record.clone(); // finding: uncharged_decode_allocation
    let _child = ctx.alloc_filled(n, child); // finding: uncharged_decode_allocation
    let _ref = Rc::clone(&shared);
    let _arc = atomic.clone();
}
