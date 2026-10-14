// SPDX-License-Identifier: Apache-2.0
//! Complete compact OM storage with product and suffix boundary crossings.

pub(crate) fn payload() -> Vec<u8> {
    let mut bytes = vec![0; 31];
    bytes[..4].fill(0xff);
    bytes[12..16].copy_from_slice(b"OM\x01\x01");
    for text in [b"x", b"p"] {
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(text);
    }
    // Header ends at77; three physical spans end at132. No class declarations.
    for word in [3_u32, 7, 0, 0, 2, 0, 0, 0, 132] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for word in [0_u32, 112, 120, 132, 3, 1, 17, 99] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(b"\x04\x01\x06NX x\0\x01\xc0\x44\x04\0");
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(b"\x05\0\x01\x80\0\xa0\0\0\x4e");
    bytes.extend_from_slice(b"\x05\0\xc0\0\0\0\x01\x02\x4e");
    bytes.extend_from_slice(&[0xff; 12]);
    let payload_len = u32::try_from(bytes.len() - 12).expect("small compact OM fixture");
    bytes[8..12].copy_from_slice(&payload_len.to_be_bytes());
    bytes
}

pub(crate) fn prt() -> Vec<u8> {
    super::test_prt::prt_with_named_payloads(&[("/Root/FastLoad/RMFastLoad", payload())])
}
