// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
pub fn apply<F: Fn(&[u8]) -> usize>(_ctx: &DecodeContext, bytes: &[u8], callback: F) -> usize {
    callback(bytes)
}
pub fn decode(ctx: &DecodeContext, bytes: &[u8], opaque: fn(&[u8]) -> usize) {
    let _fixed = apply(ctx, bytes, |value| value.len());
    let _scan = apply(ctx, bytes, |value| {
        value // finding: uncharged_decode_work
            .iter()
            .fold(0usize, |sum, byte| sum + usize::from(*byte))
    });
    let _copy = apply(ctx, bytes, |value| {
        let copied = value.to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
        copied.len()
    });
    let _opaque = apply(ctx, bytes, opaque); // finding: unproven_decode_charge
}
