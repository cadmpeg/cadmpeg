// SPDX-License-Identifier: Apache-2.0
pub fn decode(text: &String, number: u32, bytes: &[u8]) {
    let _length = cadmpeg_core::fixed(bytes);
    let _minimum = cadmpeg_core::minimum(number, number);
    cadmpeg_core::writer::writer(&cadmpeg_core::writer::DecodeContext, text); // finding: unproven_decode_charge
    let _callback = cadmpeg_core::with(|| bytes.len());
    let _fixed = cadmpeg_core::copy(&number);
    let _default = cadmpeg_core::Fixed::fixed(&number);
    let _capacity = cadmpeg_core::capacity::<u8>();
    let _constant_capacity = cadmpeg_core::constant_capacity::<u8, 4>();
    let _array = cadmpeg_core::array_copy(&[0u8; 4]);
    let _slice = cadmpeg_core::slice_copy(bytes); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _context = cadmpeg_core::context_copy(&cadmpeg_core::DecodeContext, text); // finding: uncharged_decode_allocation, uncharged_decode_work
    let _text = cadmpeg_core::copy(text); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn grow(values: &mut Vec<u8>, value: u8) {
    cadmpeg_core::grow(values, value); // finding: uncharged_decode_allocation
}

pub fn unit_vectors(values: &Vec<()>) { let _copy = cadmpeg_core::vector_copy(values); }
pub fn byte_vectors(values: &Vec<u8>) {
    let _copy = cadmpeg_core::vector_copy(values); // finding: uncharged_decode_allocation, uncharged_decode_work
}

pub fn moved_arrays(first: [String; 4], second: [String; 4]) {
    let _first = cadmpeg_core::array_move(first);
    let _second = cadmpeg_core::array_collect(second);
}
