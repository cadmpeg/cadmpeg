// SPDX-License-Identifier: Apache-2.0
trait CodecBackend {
    fn decode_impl(&self, bytes: &[u8]);
    fn encode(&self, bytes: &[u8]);
}
struct Backend;
impl CodecBackend for Backend {
    fn decode_impl(&self, bytes: &[u8]) {
        decode_helper(bytes);
        shared_helper(bytes);
        let _deferred = || closure_helper(bytes);
        dispatch(&Worker, bytes);
    }
    fn encode(&self, bytes: &[u8]) {
        encoder_helper(bytes);
        shared_helper(bytes);
    }
}
fn decode_helper(bytes: &[u8]) {
    for byte in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
fn encoder_helper(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
fn shared_helper(bytes: &[u8]) {
    for byte in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
fn closure_helper(bytes: &[u8]) {
    for byte in bytes {
        // finding: uncharged_decode_work
        std::hint::black_box(byte);
    }
}
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
fn dispatch<T: Work>(worker: &T, bytes: &[u8]) {
    worker.work(bytes);
}
fn unrelated(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
