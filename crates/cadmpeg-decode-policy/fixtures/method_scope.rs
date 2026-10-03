// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
trait CodecLike {
    fn decode(&self, bytes: &[u8]);
    fn encode(&self, bytes: &[u8]);
}
struct Backend;
impl CodecLike for Backend {
    fn decode(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
    fn encode(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
trait Base {
    fn scan(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
    fn unused(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
trait Sub: Base {}
impl Base for Backend {}
impl Sub for Backend {}
pub fn decode(ctx: &DecodeContext, bytes: &[u8]) {
    let _ctx = ctx;
    let codec: &dyn CodecLike = &Backend;
    codec.decode(bytes); // finding: unproven_decode_charge
    let sub: Box<dyn Sub> = Box::new(Backend);
    sub.scan(bytes); // finding: unproven_decode_charge
}
