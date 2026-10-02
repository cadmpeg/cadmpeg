// SPDX-License-Identifier: Apache-2.0
pub fn fixed(bytes: &[u8], value: usize) {
    let _length = bytes.len();
    let _windows = bytes.windows(4);
    let _sum = value.checked_add(1);
}
pub fn linear(bytes: &[u8]) {
    let _copy = bytes.to_vec(); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _valid = std::str::from_utf8(bytes); // finding: uncharged_decode_work
}
pub fn missing() {
    let _thread = std::thread::current(); // finding: unproven_decode_charge
}
