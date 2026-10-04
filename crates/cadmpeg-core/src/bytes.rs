// SPDX-License-Identifier: Apache-2.0
//! Shared byte-order assembly over decode data.

/// Assemble a 16-bit little-endian integer from an exact byte array.
pub const fn assemble_u16_le(bytes: [u8; 2]) -> u16 {
    // endian-exception: reconstructed-scalar
    u16::from_le_bytes(bytes)
}

/// Assemble a 16-bit big-endian integer from an exact byte array.
pub const fn assemble_u16_be(bytes: [u8; 2]) -> u16 {
    // endian-exception: reconstructed-scalar
    u16::from_be_bytes(bytes)
}

/// Assemble a 24-bit little-endian integer from an exact byte array.
pub const fn assemble_u24_le(bytes: [u8; 3]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
}

/// Assemble a 24-bit big-endian integer from an exact byte array.
pub const fn assemble_u24_be(bytes: [u8; 3]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_be_bytes([0, bytes[0], bytes[1], bytes[2]])
}

/// Assemble a 32-bit little-endian integer from an exact byte array.
pub const fn assemble_u32_le(bytes: [u8; 4]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_le_bytes(bytes)
}

/// Assemble a 32-bit big-endian integer from an exact byte array.
pub const fn assemble_u32_be(bytes: [u8; 4]) -> u32 {
    // endian-exception: reconstructed-scalar
    u32::from_be_bytes(bytes)
}

/// Assemble a 64-bit little-endian integer from an exact byte array.
pub const fn assemble_u64_le(bytes: [u8; 8]) -> u64 {
    // endian-exception: reconstructed-scalar
    u64::from_le_bytes(bytes)
}

/// Assemble a 64-bit big-endian integer from an exact byte array.
pub const fn assemble_u64_be(bytes: [u8; 8]) -> u64 {
    // endian-exception: reconstructed-scalar
    u64::from_be_bytes(bytes)
}

/// Assemble an IEEE-754 binary32 value from exact little-endian bytes.
pub const fn assemble_f32_le(bytes: [u8; 4]) -> f32 {
    f32::from_bits(assemble_u32_le(bytes))
}

/// Assemble an IEEE-754 binary32 value from exact big-endian bytes.
pub const fn assemble_f32_be(bytes: [u8; 4]) -> f32 {
    f32::from_bits(assemble_u32_be(bytes))
}

/// Assemble an IEEE-754 binary64 value from exact little-endian bytes.
pub const fn assemble_f64_le(bytes: [u8; 8]) -> f64 {
    f64::from_bits(assemble_u64_le(bytes))
}

/// Assemble an IEEE-754 binary64 value from exact big-endian bytes.
pub const fn assemble_f64_be(bytes: [u8; 8]) -> f64 {
    f64::from_bits(assemble_u64_be(bytes))
}

#[cfg(test)]
mod tests {

    #[test]
    fn assembles_wire_byte_orders_without_host_endian_assumptions() {
        assert_eq!(super::assemble_u16_le([0x02, 0x01]), 0x0102);
        assert_eq!(super::assemble_u24_be([0x01, 0x02, 0x03]), 0x0001_0203);
        assert_eq!(
            super::assemble_u32_be([0x01, 0x02, 0x03, 0x04]),
            0x0102_0304
        );
        assert_eq!(super::assemble_u64_le([1, 0, 0, 0, 0, 0, 0, 0]), 1);
        assert_eq!(super::assemble_f32_be([0x3f, 0xc0, 0, 0]), 1.5);
        assert_eq!(super::assemble_f64_le([0, 0, 0, 0, 0, 0, 0xf0, 0x3f]), 1.0);
    }
}
