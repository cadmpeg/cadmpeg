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
    }
    fn encode(&self, bytes: &[u8]) {
        cadmpeg_core::encoder_helper(bytes);
        cadmpeg_core::shared_helper(bytes);
    }
}
