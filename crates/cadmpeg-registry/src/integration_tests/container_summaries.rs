// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::container::{ContainerEntry, ContainerRole, EntryCompression};
use serde::Deserialize;

#[derive(Deserialize)]
struct Summary {
    entries: Vec<ContainerEntry>,
}

#[test]
fn native_summary_labels_distinguish_ranges_storages_and_streams() {
    let rhino: Summary = serde_json::from_str(include_str!(
        "../../../cadmpeg-codec-rhino/tests/golden/inspect/point.json"
    ))
    .expect("native summary witness");
    let table = rhino
        .entries
        .iter()
        .find(|entry| entry.role == ContainerRole::Table)
        .expect("native summary witness");
    assert_eq!(table.compression, EntryCompression::None);
    let body_offset: u64 = table.attributes["body_offset"]
        .parse()
        .expect("native summary witness");
    let offset: u64 = table.attributes["offset"]
        .parse()
        .expect("native summary witness");
    assert_eq!(
        table.compressed_size - table.uncompressed_size,
        body_offset - offset
    );
    assert!(body_offset > offset);

    let inventor: Summary = serde_json::from_str(include_str!(
        "../../../cadmpeg-codec-inventor/tests/golden/inspect/structural.json"
    ))
    .expect("native summary witness");
    let storage = inventor
        .entries
        .iter()
        .find(|entry| entry.name == "RSeStorage")
        .expect("native summary witness");
    assert_eq!(storage.compression, EntryCompression::Storage);
    let stream = inventor
        .entries
        .iter()
        .find(|entry| entry.name == "RSeStorage/RSeSegInfo")
        .expect("native summary witness");
    assert_eq!(stream.compression, EntryCompression::Stored);
    assert_eq!(stream.compressed_size, stream.uncompressed_size);

    let step: Summary = serde_json::from_str(include_str!(
        "../../../cadmpeg-codec-step/tests/golden/inspect/ap242_ed3_sections.json"
    ))
    .expect("native summary witness");
    let references = step
        .entries
        .iter()
        .find(|entry| entry.name == "REFERENCE")
        .expect("native summary witness");
    assert_eq!(references.role, ContainerRole::ExternalReferences);
    assert_eq!(references.compression, EntryCompression::None);
    assert_eq!(references.attributes["external_count"], "1");
}
