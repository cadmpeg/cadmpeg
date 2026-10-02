// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub fn generic<T: Clone>(ctx: &DecodeContext, value: T) {
    let _ctx = ctx;
    let _copy = value.clone(); // finding: unproven_decode_charge
}
pub fn callback(ctx: &DecodeContext, f: &dyn Fn() -> usize) {
    let _ctx = ctx;
    let _value = f(); // finding: unproven_decode_charge
}
pub fn opaque_iter(ctx: &DecodeContext, iter: impl Iterator<Item = u8>) {
    let _ctx = ctx;
    let _count = iter.count(); // finding: unproven_decode_charge
}
pub fn dynamic_format(ctx: &DecodeContext, number: u32, width: usize) {
    let _ctx = ctx;
    let _value = format!("{number:width$}"); // finding: unproven_decode_charge
}
pub fn strings(ctx: &DecodeContext, values: &[String]) {
    let _ctx = ctx;
    let _copies: Vec<String> = values.iter().cloned().collect(); // finding: uncharged_decode_allocation, uncharged_decode_allocation
}
