// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
trait Work {
    fn work(&self, bytes: &[u8]);
}
struct Worker;
impl Work for Worker {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
fn uncalled_object() {
    let _object: Box<dyn Work> = Box::new(Worker);
}
static ADDRESSES: [fn(&[u8]); 1] = [deferred];
fn deferred(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
fn lifetime<'a: 'static>(worker: &'a dyn Work, bytes: &[u8]) {
    worker.work(bytes); // finding: unproven_decode_charge
}
pub fn decode(ctx: &DecodeContext, worker: &'static dyn Work, bytes: &[u8]) {
    let _ctx = ctx;
    lifetime(worker, bytes);
}
fn encode(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
