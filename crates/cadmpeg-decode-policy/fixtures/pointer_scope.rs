// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
static MATCHING: [fn(&[u8]) -> usize; 1] = [matching];
static WIDE: [fn(&[u16]) -> usize; 1] = [wide];
static OTHER: [fn(&str) -> String; 1] = [other];
static RETURN_MISMATCH: [fn(&[u8]) -> u8; 1] = [return_mismatch];
fn matching<'a>(bytes: &'a [u8]) -> usize {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    bytes.len()
}
fn other(text: &str) -> String {
    for byte in text.bytes() {
        std::hint::black_box(byte);
    }
    text.to_owned()
}
fn return_mismatch(bytes: &[u8]) -> u8 {
    for byte in bytes {
        std::hint::black_box(byte);
    }
    0
}
fn stored_closures() {
    let _matching: fn(&[u8]) -> usize = |bytes| {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
        bytes.len()
    };
    let _other: fn(&str) -> String = |text| {
        for byte in text.bytes() {
            std::hint::black_box(byte);
        }
        text.to_owned()
    };
}
fn invoke<T>(callback: fn(&[T]) -> usize, input: &[T]) -> usize {
    callback(input); // finding: unproven_decode_charge
    0
}
pub fn decode(ctx: &DecodeContext, callback: fn(&[u8]) -> usize, bytes: &[u8]) {
    let _ctx = ctx;
    callback(bytes); // finding: unproven_decode_charge
    invoke(callback, bytes);
    nested(callback, bytes);
}

fn nested<T>(callback: fn(&[T]) -> usize, input: &[T]) {
    let invoke = || callback(input); // finding: unproven_decode_charge
    invoke();
}

fn wide(bytes: &[u16]) -> usize {
    for byte in bytes {
        std::hint::black_box(byte);
    }
    bytes.len()
}

fn capturing_closure() {
    let seed = 3;
    let _callback = |bytes: &[u8]| -> usize {
        for byte in bytes {
            std::hint::black_box((byte, seed));
        }
        bytes.len()
    };
}
