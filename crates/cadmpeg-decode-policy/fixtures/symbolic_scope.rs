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
fn nested<T: Work>(worker: &T, bytes: &[u8]) {
    worker.work(bytes);
}
pub fn decode<T: Work>(ctx: &DecodeContext, worker: &T, bytes: &[u8]) {
    let _ctx = ctx;
    nested(worker, bytes);
}
fn encode(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
