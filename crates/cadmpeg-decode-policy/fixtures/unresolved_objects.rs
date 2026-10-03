// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
trait Work { fn work(&self, bytes: &[u8]); }
struct Inner;
impl Work for Inner {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
trait Object { fn read(&self, bytes: &[u8]); fn write(&self, bytes: &[u8]); }
struct Wrapper<T>(T);
impl<T: Work> Object for Wrapper<T> {
    fn read(&self, bytes: &[u8]) { self.0.work(bytes); }
    fn write(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
fn store_objects() {
    let _object: Box<dyn Object> = Box::new(Wrapper(Inner));
    let captured = 7;
    let _closure: Box<dyn Fn(&[u8])> = Box::new(|bytes| {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box((byte, captured));
        }
    });
}
pub fn decode(ctx: &DecodeContext, object: &dyn Object, callback: &dyn Fn(&[u8]), bytes: &[u8]) {
    let _ctx = ctx;
    object.read(bytes); // finding: unproven_decode_charge
    callback(bytes); // finding: unproven_decode_charge
}
