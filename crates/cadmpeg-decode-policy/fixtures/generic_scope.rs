// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
trait Work { fn work(&self, bytes: &[u8]); }
struct First;
struct Second;
struct Encoder;
impl Work for First {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
impl Work for Second {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
impl Work for Encoder {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { std::hint::black_box(byte); }
    }
}
fn dispatch<T: Work>(worker: &T, bytes: &[u8]) { worker.work(bytes); }
fn nested<T: Work>(worker: &T, bytes: &[u8]) { dispatch(worker, bytes); }
trait Associated { type Worker: Work; fn worker() -> Self::Worker; }
struct Provider;
impl Associated for Provider {
    type Worker = Second;
    fn worker() -> Self::Worker { Second }
}
fn associated<T: Associated>(bytes: &[u8]) { dispatch(&T::worker(), bytes); }
pub fn decode(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    nested(&First, bytes);
    associated::<Provider>(bytes);
}
fn encode(bytes: &[u8]) { nested(&Encoder, bytes); }
