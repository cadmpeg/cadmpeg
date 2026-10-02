// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
trait Recursive { fn scan(&self, bytes: &[u8]); fn encode(&self, bytes: &[u8]); }
struct Worker<T>(std::marker::PhantomData<T>);
impl<T> Recursive for Worker<T> {
    fn scan(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
        let object: &dyn Recursive = self;
        object.scan(bytes); // finding: unproven_decode_charge
    }
    fn encode(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
pub fn decode(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    let object: Box<dyn Recursive> = Box::new(Worker::<u8>(std::marker::PhantomData));
    object.scan(bytes); // finding: unproven_decode_charge
}
