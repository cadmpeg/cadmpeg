// SPDX-License-Identifier: Apache-2.0
//! Biased XMT index encoding for hand-built streams.

/// Append an XMT-encoded schema reference, wide references split into a
/// negated remainder and a quotient.
pub fn push_xmt(bytes: &mut Vec<u8>, reference: u32) {
    if i16::try_from(reference).is_ok() {
        bytes.extend_from_slice(
            &(u16::try_from(reference).expect("fixture value fits u16")).to_be_bytes(),
        );
        return;
    }
    let quotient = reference / 32_767;
    let remainder = reference % 32_767;
    bytes.extend_from_slice(
        &(-(i16::try_from(remainder).expect("fixture value fits i16"))).to_be_bytes(),
    );
    bytes.extend_from_slice(
        &(u16::try_from(quotient).expect("fixture value fits u16")).to_be_bytes(),
    );
}
