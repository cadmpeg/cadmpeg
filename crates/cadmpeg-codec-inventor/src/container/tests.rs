// SPDX-License-Identifier: Apache-2.0

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, Confidence};

use super::{classify, find_summary_entry, insert_attribute};
use crate::test_support::test_fixtures::{fixture, primary_envelope_fixture_with_broken_metadata};
use crate::InventorCodec;

#[test]
fn container_summary_attribute_is_inserted() {
    let bytes = fixture(true);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("fixture snapshot");
    let mut entries = snapshot.container_entries(classify);
    let entry = entries.first_mut().expect("fixture entry");
    insert_attribute(entry, "test", "value");
    assert_eq!(entry.attributes["test"], "value");
}

#[test]
fn container_summary_search_finds_the_directory_entry() {
    let bytes = fixture(true);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("fixture snapshot");
    let mut entries = snapshot.container_entries(classify);
    assert!(find_summary_entry(&mut entries, snapshot.entries()[0].directory_id()).is_some());
}

#[test]
fn detects_only_structurally_corroborated_inventor_cfb() {
    let inventor = fixture(true);
    let unrelated = fixture(false);
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&InventorCodec, &inventor),
        Confidence::High
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&InventorCodec, &unrelated),
        Confidence::No
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&InventorCodec, b"not a compound file"),
        Confidence::No
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&InventorCodec, &inventor[..400]),
        Confidence::No
    );
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

#[test]
fn malformed_metadata_inspection_retains_its_declaration() {
    let mut input = std::io::Cursor::new(primary_envelope_fixture_with_broken_metadata());
    let summary = InventorCodec
        .inspect(&mut input, &cadmpeg_core::decode::InspectOptions::default())
        .expect("malformed metadata remains inspectable");
    let entry = summary
        .entries
        .iter()
        .find(|entry| entry.name.ends_with("/Mseg"))
        .expect("metadata stream entry");
    assert_eq!(entry.attributes["meta_marker"], "RSe Meta Stream Version 8");
    assert_eq!(entry.attributes["meta_stream_version"], "8");
    assert!(entry.attributes.contains_key("framing_error"));
}
