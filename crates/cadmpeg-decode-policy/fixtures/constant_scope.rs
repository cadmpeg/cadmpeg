// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
const fn only_constant(bytes: &[u8]) -> usize {
    let mut count = 0;
    while count < bytes.len() {
        count += 1;
    }
    count
}
const LENGTH: usize = only_constant(b"fixed");
static STATIC_LENGTH: usize = only_constant(b"static");
struct Constants;
impl Constants {
    const LENGTH: usize = only_constant(b"associated");
}
const fn also_runtime(bytes: &[u8]) -> usize {
    let mut count = 0;
    while count < bytes.len() {
        // finding: uncharged_decode_work
        count += 1;
    }
    count
}
const RUNTIME_LENGTH: usize = also_runtime(b"fixed");
fn callback(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
} // finding: uncharged_decode_allocation, uncharged_decode_work
const fn select() -> fn(&[u8]) -> Vec<u8> {
    callback
}
const CALLBACK: fn(&[u8]) -> Vec<u8> = select();
const fn generic_length<T>(bytes: &[u8]) -> usize {
    only_constant(bytes)
}
const GENERIC_LENGTH: usize = generic_length::<u64>(b"generic");
pub fn decode(_ctx: &DecodeContext, bytes: &[u8]) -> usize {
    let _constant = LENGTH + STATIC_LENGTH + Constants::LENGTH + RUNTIME_LENGTH + GENERIC_LENGTH;
    let _inline = const { only_constant(b"inline") };
    let _array = [0; only_constant(b"array")];
    drop(CALLBACK(bytes)); // finding: unproven_decode_charge
    also_runtime(bytes)
}
