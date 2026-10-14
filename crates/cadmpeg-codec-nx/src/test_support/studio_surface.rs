// SPDX-License-Identifier: Apache-2.0
//! Synthetic complete Studio allocation from the independently checked byte grammar.

pub(crate) fn payload() -> Vec<u8> {
    let mut bytes = vec![0x14, 0, 0, 1, 0];
    for index in 1..=3u8 {
        bytes.extend([0xf0, index]);
    }
    bytes.extend([1, 9, 2, 2, 2, 5, 2, 1, 1, 1, 1, 9]);
    for index in 4..=11u8 {
        bytes.extend([0xf0, index]);
    }
    bytes.extend([2, 1, 0xa0, 0x5a, 0x50, 0x15, 1, 3]);
    for (member, terminal, suffix) in [
        (12, 13, &[0xa0, 0x10, 0x54][..]),
        (14, 15, &[0x5d, 1, 2][..]),
        (16, 17, &[0x89, 0x1c][..]),
    ] {
        bytes.extend([0x16, 1, 2, 0xf0, member, 1, 2]);
        bytes.extend([0; 5]);
        bytes.extend([0xff, 1, 2, 0xf0, terminal, 0]);
        bytes.extend(suffix);
    }
    bytes.extend([0, 0, 0, 0, 0, 0xff, 0xff, 1]);
    bytes.extend([
        0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 4, 2, 1, 1, 1, 5, 1, 1, 1, 1, 1, 9,
    ]);
    for index in 18..=25u8 {
        bytes.extend([0xf0, index]);
    }
    bytes.extend([
        1, 5, 0xff, 0xff, 0xff, 0xff, 0x15, 1, 0x10, 4, 4, 2, 2, 0xa0, 0x5a, 0x50, 8, 8, 2, 2, 1,
        1, 1, 1, 2, 2, 1, 5, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b,
    ]);
    bytes.extend([3, 3, 0x2f, 0xa4, 0x7a, 0xe1, 0x47, 0xae, 0x14, 0x7b]);
    for index in 26..=28u8 {
        bytes.extend([0xf0, index]);
    }
    bytes.extend([
        1, 1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 1, 2,
    ]);
    bytes
}

pub(crate) fn prt() -> Vec<u8> {
    let store: [&[u8]; 28] = [b"A"; 28];
    let part = super::test_om::composed_feature_history_payload(
        &[(&[0xff; 4], "Studio Surface", payload())],
        &store,
    );
    super::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)])
}

/// The common envelope and counted SKIN branch grammar, with no Studio lane.
pub(crate) fn skin_prt() -> Vec<u8> {
    let studio = payload();
    let mut skin = studio[..109].to_vec();
    skin[0] = 0x3f;
    skin.extend([0, 0, 0, 1, 3, 0, 0, 0, 0xff, 0xff, 1]);
    skin.extend(&studio[191..201]);
    for index in 18..=20u8 {
        skin.extend([0xf0, index]);
    }
    skin.extend(&studio[207..]);
    let store: [&[u8]; 20] = [b"A"; 20];
    let part =
        super::test_om::composed_feature_history_payload(&[(&[0xff; 4], "SKIN", skin)], &store);
    super::test_prt::prt_with_named_payloads(&[("/Root/UG_PART/UG_PART", part)])
}
