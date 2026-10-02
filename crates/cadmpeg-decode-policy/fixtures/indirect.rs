// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
trait Work {
    fn work(&self, bytes: &[u8]);
}
struct Worker;
impl Work for Worker {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
struct Family {
    decode: fn(&[u8]),
}
static DECODERS: [Family; 1] = [Family { decode: table_target }];
fn table_target(bytes: &[u8]) {
    for byte in bytes { // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
pub fn decode(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    (DECODERS[0].decode)(bytes); // finding: unproven_decode_charge
    let worker: Box<dyn Work> = Box::new(Worker);
    worker.work(bytes); // finding: unproven_decode_charge
}
fn encode(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
