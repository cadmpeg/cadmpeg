// SPDX-License-Identifier: Apache-2.0
pub struct DecodeContext;
fn safe_target(bytes: &[u8]) -> usize {
    for byte in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
    bytes.len()
}
static UNSAFE_TABLE: [unsafe fn(&[u8]) -> usize; 1] = [safe_target];
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
fn store_method() {
    let _method: fn(&Worker, &[u8]) = <Worker as Work>::work;
}
trait Dynamic {
    fn read(&self, bytes: &[u8]);
    fn write(&self, bytes: &[u8]);
}
struct Backend;
impl Dynamic for Backend {
    fn read(&self, bytes: &[u8]) {
        for byte in bytes {
            // finding: uncharged_decode_work
            std::hint::black_box(byte);
        }
    }
    fn write(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
fn store_virtual() {
    let _method: fn(&(dyn Dynamic + 'static), &[u8]) = <dyn Dynamic as Dynamic>::read;
}
pub fn decode(
    ctx: &DecodeContext,
    callback: unsafe fn(&[u8]) -> usize,
    method: fn(&Worker, &[u8]),
    virtual_method: fn(&(dyn Dynamic + 'static), &[u8]),
    worker: &Worker,
    object: &(dyn Dynamic + 'static),
    bytes: &[u8],
) {
    let _ctx = ctx;
    unsafe { callback(bytes) }; // finding: unproven_decode_charge
    method(worker, bytes); // finding: unproven_decode_charge
    virtual_method(object, bytes); // finding: unproven_decode_charge
}
