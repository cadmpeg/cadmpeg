// SPDX-License-Identifier: Apache-2.0
pub fn decode(text: &String, number: u32, bytes: &[u8]) {
    let _length = cadmpeg_core::fixed(bytes);
    let _callback = cadmpeg_core::with(|| bytes.len());
    let _fixed = cadmpeg_core::copy(&number);
    let _text = cadmpeg_core::copy(text); // finding: uncharged_decode_allocation, uncharged_decode_work
}
