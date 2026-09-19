// SPDX-License-Identifier: Apache-2.0
//! Little-endian integer writes into a hand-built byte fixture.
//!
//! Every fixture builder that lays out a binary container writes the same
//! widths at an absolute offset. They are declared once here.

/// Write a little-endian `u16` at `offset`.
pub fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

/// Write a little-endian `u32` at `offset`.
pub fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Write a little-endian `u64` at `offset`.
pub fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
