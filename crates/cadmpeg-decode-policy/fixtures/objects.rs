// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
trait Base {
    fn base(&self, bytes: &[u8]);
    fn default_method(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
trait Work: Base {
    fn work(&self, bytes: &[u8]);
    fn sized(&self, bytes: &[u8]) where Self: Sized;
}
struct Worker;
impl Base for Worker {
    fn base(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
impl Work for Worker {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
    fn sized(&self, bytes: &[u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
struct Encoder;
impl Base for Encoder {
    fn base(&self, bytes: &[u8]) {
        for byte in bytes { std::hint::black_box(byte); }
    }
}
impl Work for Encoder {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes { std::hint::black_box(byte); }
    }
    fn sized(&self, bytes: &[u8]) {
        for byte in bytes { std::hint::black_box(byte); }
    }
}
pub fn decode(ctx: &DecodeContext) {
    let _ctx = ctx;
    let _boxed: Box<dyn Work> = Box::new(Worker);
    let _reference: &dyn Work = &Worker;
    let _arc: std::sync::Arc<dyn Work> = std::sync::Arc::new(Worker);
    let _cast = &Worker as &dyn Work;
    let _higher: &dyn for<'a> Higher<'a> = &HigherWorker;
}
fn encode() { let _writer: Box<dyn Work> = Box::new(Encoder); }

trait Higher<'a> { fn scan(&self, bytes: &'a [u8]); }
struct HigherWorker;
impl<'a> Higher<'a> for HigherWorker {
    fn scan(&self, bytes: &'a [u8]) {
        for byte in bytes { // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
