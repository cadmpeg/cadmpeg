// SPDX-License-Identifier: Apache-2.0
//! Primary-layout byte fixtures for current and historical PMARK nodes.

use super::test_deltas::push_xmt;

pub(crate) fn header() -> Vec<u8> {
    let mut bytes = b"PS".to_vec();
    for text in [b"Transmit (deltas)".as_slice(), b"SCH_3501171_35102_13006"] {
        bytes.extend_from_slice(
            &u32::try_from(text.len())
                .expect("fixture length")
                .to_be_bytes(),
        );
        bytes.extend_from_slice(text);
    }
    bytes.extend_from_slice(&239_u16.to_be_bytes());
    bytes.extend_from_slice(&0_u32.to_be_bytes());
    bytes
}

pub(crate) fn node(xmt: u32, preceding: u32, following: u32, identifier: u32) -> Vec<u8> {
    let mut bytes = 4_u16.to_be_bytes().to_vec();
    for value in [xmt, preceding, following, 1, 1] {
        push_xmt(&mut bytes, value);
    }
    for value in [34_u32, 6, 11, 22_362] {
        bytes.extend_from_slice(&value.to_be_bytes());
    }
    bytes.push(0);
    bytes.extend_from_slice(&identifier.to_be_bytes());
    bytes
}

pub(crate) fn prefix(preceding: u32) -> Vec<u8> {
    let mut bytes = header();
    bytes.extend_from_slice(&[0, 3, 255]);
    for value in [49_947, 49_948] {
        push_xmt(&mut bytes, value);
    }
    bytes.extend_from_slice(&u32::MAX.to_be_bytes());
    let mark = node(49_948, preceding, 1, 0x1234_5678);
    bytes.extend_from_slice(&mark[..2]);
    bytes.push(255);
    bytes.extend_from_slice(&mark[2..]);
    bytes
}

pub(crate) fn packet() -> Vec<u8> {
    let mut bytes = vec![0, 1, 0, 1];
    bytes.extend(node(40_000, 1, 49_948, 0x1234_5677));
    bytes
}
