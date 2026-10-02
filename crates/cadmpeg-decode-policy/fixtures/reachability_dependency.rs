// SPDX-License-Identifier: Apache-2.0
pub fn decode_helper(bytes: &[u8]) {
    for byte in bytes {
        // reached-loop
        std::hint::black_box(byte);
    }
}
pub fn encoder_helper(bytes: &[u8]) {
    for byte in bytes {
        std::hint::black_box(byte);
    }
}
pub fn shared_helper(bytes: &[u8]) {
    for byte in bytes {
        // reached-loop
        std::hint::black_box(byte);
    }
}

pub static DECODERS: [fn(&[u8]); 1] = [table_target];
fn table_target(bytes: &[u8]) {
    for byte in bytes {
        // reached-loop
        std::hint::black_box(byte);
    }
}
pub trait Base {
    fn base(&self, bytes: &[u8]);
}
pub trait Work: Base {
    fn work(&self, bytes: &[u8]);
}
pub struct Worker;
impl Base for Worker {
    fn base(&self, bytes: &[u8]) {
        for byte in bytes {
            // reached-loop
            std::hint::black_box(byte);
        }
    }
}
impl Work for Worker {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes {
            // reached-loop
            std::hint::black_box(byte);
        }
    }
}
pub fn dispatch<T: Work>(worker: &T, bytes: &[u8]) {
    worker.work(bytes);
}
pub fn nested<T: Work>(worker: &T, bytes: &[u8]) {
    dispatch(worker, bytes);
}

static DEFERRED: [fn(&[u8]); 1] = [deferred_target];
fn deferred_target(bytes: &[u8]) {
    for byte in bytes {
        // reached-loop
        std::hint::black_box(byte);
    }
}
struct DeferredWorker;
impl Base for DeferredWorker {
    fn base(&self, bytes: &[u8]) {
        for byte in bytes {
            // reached-loop
            std::hint::black_box(byte);
        }
    }
}
impl Work for DeferredWorker {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes {
            // reached-loop
            std::hint::black_box(byte);
        }
    }
}
fn uncalled_object() {
    let _object: Box<dyn Work> = Box::new(DeferredWorker);
}
