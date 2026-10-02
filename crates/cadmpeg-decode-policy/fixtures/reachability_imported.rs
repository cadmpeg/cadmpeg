// SPDX-License-Identifier: Apache-2.0
trait CodecBackend {
    fn decode_impl(&self, bytes: &[u8]);
    fn encode(&self, bytes: &[u8]);
}
struct Backend;
impl CodecBackend for Backend {
    fn decode_impl(&self, bytes: &[u8]) {
        cadmpeg_core::decode_helper(bytes);
        cadmpeg_core::shared_helper(bytes);
        (cadmpeg_core::DECODERS[0])(bytes);
        let _worker: Box<dyn cadmpeg_core::Work> = Box::new(cadmpeg_core::Worker);
        cadmpeg_core::nested(&LocalWorker, bytes);
    }
    fn encode(&self, bytes: &[u8]) {
        cadmpeg_core::encoder_helper(bytes);
        cadmpeg_core::shared_helper(bytes);
    }
}

struct LocalWorker;
impl cadmpeg_core::Base for LocalWorker {
    fn base(&self, _bytes: &[u8]) {}
}
impl cadmpeg_core::Work for LocalWorker {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes {
            // reached-loop
            std::hint::black_box(byte);
        }
    }
}
struct EncoderWorker;
impl cadmpeg_core::Base for EncoderWorker {
    fn base(&self, _bytes: &[u8]) {}
}
impl cadmpeg_core::Work for EncoderWorker {
    fn work(&self, bytes: &[u8]) {
        for byte in bytes {
            std::hint::black_box(byte);
        }
    }
}
fn encode_generic(bytes: &[u8]) {
    cadmpeg_core::nested(&EncoderWorker, bytes);
}
