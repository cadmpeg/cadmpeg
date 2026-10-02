// SPDX-License-Identifier: Apache-2.0
use std::hash::{Hash, Hasher};
pub struct DecodeContext;
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Owned { pub values: Vec<u8> }
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct Fixed { pub value: [u64; 4] }
pub fn decode(ctx: &DecodeContext, first: &Owned, second: &Owned, small: &Fixed, hash: &mut impl Hasher) {
    let _ctx = ctx;
    let _clone = first.clone(); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _equal = first == second; // finding: uncharged_decode_work
    let _order = first.cmp(second); // finding: uncharged_decode_work
    let _partial = first.partial_cmp(second); // finding: uncharged_decode_work
    first.hash(hash); // finding: uncharged_decode_work
    let _debug = format!("{first:?}"); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _default = Owned::default();
    let _clone = small.clone();
    let _equal = small == small;
    let _order = small.cmp(small);
    let _partial = small.partial_cmp(small);
    small.hash(hash);
    let _debug = format!("{small:?}");
    let _default = Fixed::default();
}
fn compare<T: PartialEq>(first: &T, second: &T) -> bool { first == second }
pub fn generic(ctx: &DecodeContext, first: &Owned, second: &Owned, small: &Fixed) {
    let _ctx = ctx;
    let _equal = compare(first, second); // finding: uncharged_decode_work
    let _fixed = compare(small, small);
}
