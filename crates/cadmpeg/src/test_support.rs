// SPDX-License-Identifier: Apache-2.0
//! Shared byte-fixture helpers for the crate's `#[cfg(test)]` suites.

/// Write a little-endian `u16` at `offset`.
pub(crate) fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

/// Write a little-endian `u32` at `offset`.
pub(crate) fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
