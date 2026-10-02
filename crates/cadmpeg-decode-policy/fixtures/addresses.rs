// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
struct Family { decode: fn(&[u8]) }
const DECODERS: [Family; 1] = [Family { decode: const_target }];
static ENCODERS: [fn(&[u8]); 1] = [encoder_target];
fn const_target(bytes: &[u8]) {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
fn local_target(bytes: &[u8]) {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
fn argument_target(bytes: &[u8]) {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
fn accept(_callback: fn(&[u8])) {}
pub fn decode(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    let _table = &DECODERS;
    let _family = Family { decode: local_target };
    let _array: [fn(&[u8]); 1] = [local_target];
    accept(argument_target);
    let _closure: fn(&[u8]) = |input| {
        for byte in input { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    };
    std::hint::black_box(bytes);
}
fn encoder_target(bytes: &[u8]) {
    for byte in bytes { std::hint::black_box(byte); }
}
fn encode() { let _table = &ENCODERS; }
