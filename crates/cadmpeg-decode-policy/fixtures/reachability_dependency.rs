// SPDX-License-Identifier: Apache-2.0
pub fn decode_helper(bytes: &[u8]) {
    for byte in bytes {
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
        std::hint::black_box(byte);
    }
}
