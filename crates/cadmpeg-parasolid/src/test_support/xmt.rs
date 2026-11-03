// SPDX-License-Identifier: Apache-2.0
//! Biased XMT index encoding for hand-built streams.

/// Append an XMT-encoded schema reference, wide references split into a
/// negated remainder and a quotient.
pub fn push_xmt(bytes: &mut Vec<u8>, reference: u32) {
    if let Ok(short) = i16::try_from(reference) {
        bytes.extend_from_slice(&short.to_be_bytes());
        return;
    }
    let quotient = (reference - 1) / 32_767;
    let remainder = i16::try_from(reference - quotient * 32_767).unwrap_or(i16::MAX);
    bytes.extend_from_slice(&0_i16.saturating_sub(remainder).to_be_bytes());
    bytes.extend_from_slice(&u16::try_from(quotient).unwrap_or(u16::MAX).to_be_bytes());
}
