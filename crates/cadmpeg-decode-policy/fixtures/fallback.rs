// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
static CALLBACKS: [fn(&[u8]); 1] = [deferred];
fn deferred(bytes: &[u8]) {
    for byte in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
trait Base {
    fn base(&self, bytes: &[u8]);
}
trait Work: Base {
    fn work(&self, bytes: &[u8]);
}
struct Factory;
impl Base for Factory {
    fn base(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
impl Work for Factory {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
}
fn uncalled() {
    let _object: std::sync::Arc<dyn Work> = std::sync::Arc::new(Factory);
    let _closure: fn(&[u8]) = |bytes| {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    };
}
pub fn decode(ctx: &DecodeContext, callback: fn(&[u8]), bytes: &[u8]) {
    let _ctx = ctx;
    callback(bytes); // finding: unproven_decode_charge
}
fn encode(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
#[cfg(test)]
fn test_only() {
    let _callback: fn(&[u8]) = encode;
}
