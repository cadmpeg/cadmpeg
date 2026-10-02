// SPDX-License-Identifier: Apache-2.0
pub fn decode(text: &String, number: u32, bytes: &[u8]) {
    let _length = cadmpeg_core::fixed(bytes);
    cadmpeg_core::writer::writer(&cadmpeg_core::writer::DecodeContext, text); // finding: unproven_decode_charge
    let _callback = cadmpeg_core::with(|| bytes.len());
    let _fixed = cadmpeg_core::copy(&number);
    let _default = cadmpeg_core::Fixed::fixed(&number);
    let _capacity = cadmpeg_core::capacity::<u8>();
    let _constant_capacity = cadmpeg_core::constant_capacity::<u8, 4>();
    let _slice = cadmpeg_core::slice_copy(bytes); // finding: uncharged_decode_allocation
    let _context = cadmpeg_core::context_copy(&cadmpeg_core::DecodeContext, text); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _text = cadmpeg_core::copy(text); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn grow(values: &mut Vec<u8>, value: u8) {
    cadmpeg_core::grow(values, value); // finding: uncharged_decode_allocation
}
