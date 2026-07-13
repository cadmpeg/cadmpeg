// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_ir::codec::{Codec, Confidence};

use crate::IgesCodec;

fn card(data: &[u8], section: u8, sequence: u32) -> Vec<u8> {
    assert!(data.len() <= 72);
    let mut card = vec![b' '; 80];
    card[..data.len()].copy_from_slice(data);
    card[72] = section;
    card[73..80].copy_from_slice(format!("{sequence:>7}").as_bytes());
    card.push(b'\n');
    card
}

#[test]
fn fixed_ascii_detection_requires_two_consistent_cards() {
    let mut valid = card(b"generated fixture", b'S', 1);
    valid.extend(card(b"", b'G', 1));
    assert_eq!(IgesCodec.detect(&valid), Confidence::High);

    assert_eq!(IgesCodec.detect(&valid[..81]), Confidence::No);

    let mut arbitrary = vec![b'x'; 72];
    arbitrary.extend_from_slice(b"S      1\nsecond line\n");
    assert_eq!(IgesCodec.detect(&arbitrary), Confidence::No);
}
