// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence};

use crate::test_support::fixture;
use crate::InventorCodec;

#[test]
fn detects_only_structurally_corroborated_inventor_cfb() {
    let inventor = fixture(true);
    let unrelated = fixture(false);
    assert_eq!(InventorCodec.detect(&inventor), Confidence::High);
    assert_eq!(InventorCodec.detect(&unrelated), Confidence::No);
    assert_eq!(InventorCodec.detect(b"not a compound file"), Confidence::No);
    assert_eq!(InventorCodec.detect(&inventor[..400]), Confidence::No);
}

#[test]
fn inspects_the_complete_synthetic_hierarchy() {
    let mut input = std::io::Cursor::new(fixture(true));
    let summary = InventorCodec
        .inspect(&mut input, &cadmpeg_core::decode::InspectOptions::default())
        .expect("synthetic Inventor container inspects");
    assert_eq!(summary.format(), "inventor");
    assert!(summary
        .entries
        .iter()
        .any(|entry| entry.name == "RSeStorage/RSeSegInfo"));
}
