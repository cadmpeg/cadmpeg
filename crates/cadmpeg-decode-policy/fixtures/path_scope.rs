// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
fn leaf(bytes: &[u8]) {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
fn short(bytes: &[u8]) { leaf(bytes); }
fn long(bytes: &[u8]) { short(bytes); }
fn addressed(bytes: &[u8]) -> usize {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    bytes.len()
}
fn fallback(bytes: &[u8]) -> usize {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    bytes.len()
}
static TABLE: [fn(&[u8]) -> usize; 1] = [fallback];
trait Work { fn work(&self, bytes: &[u8]); }
struct Inner;
impl Work for Inner {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
trait Object {
    fn run(&self, bytes: &[u8]);
    fn write(&self, bytes: &[u8]);
}
struct Wrapper<T>(T);
impl<T: Work> Object for Wrapper<T> {
    fn run(&self, bytes: &[u8]) { self.0.work(bytes); }
    fn write(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
pub fn decode(ctx: &DecodeContext, bytes: &[u8], callback: fn(&[u8]) -> usize) {
    let _ctx = ctx;
    long(bytes);
    short(bytes);
    let _address: fn(&[u8]) -> usize = addressed;
    callback(bytes); // finding: unproven_decode_charge
    let object: Box<dyn Object> = Box::new(Wrapper(Inner));
    object.run(bytes); // finding: unproven_decode_charge
}
fn encode(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
