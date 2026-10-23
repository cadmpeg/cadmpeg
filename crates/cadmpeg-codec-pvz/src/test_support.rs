// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.pvz` packages for crate tests.
//!
//! Member bodies after the signature are arbitrary bytes: the codec reads
//! signatures only.

use cadmpeg_test_support::zip_fixture::{archive, RawEntry};

/// A structure member with the `PV-PVS0301` signature.
pub(crate) const PVS_0301: &[u8] = b"PV-PVS0301\0synthetic structure body";
/// A structure member with the `PV-PVS0304` signature.
pub(crate) const PVS_0304: &[u8] = b"PV-PVS0304\0synthetic structure body";
/// A geometry member with the `DIV-OL08BL` signature.
pub(crate) const OL_08: &[u8] = b"DIV-OL08BL\0synthetic geometry body";
/// A geometry member with the `DIV-OL09BL` signature.
pub(crate) const OL_09: &[u8] = b"DIV-OL09BL\0synthetic geometry body";

/// A ZIP archive of stored members, in the given order.
pub(crate) fn package(members: &[(&str, &[u8])]) -> Vec<u8> {
    archive(
        &members
            .iter()
            .map(|(name, bytes)| RawEntry::stored(name, bytes))
            .collect::<Vec<_>>(),
    )
}

/// `plain` as one final stored DEFLATE block (RFC 1951 section 3.2.4).
pub(crate) fn deflate_stored_block(plain: &[u8]) -> Vec<u8> {
    let len = u16::try_from(plain.len()).expect("a stored block holds at most 65535 bytes");
    let mut block = vec![0x01];
    block.extend_from_slice(&len.to_le_bytes());
    block.extend_from_slice(&(!len).to_le_bytes());
    block.extend_from_slice(plain);
    block
}
