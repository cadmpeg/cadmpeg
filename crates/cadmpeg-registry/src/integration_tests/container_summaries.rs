// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::container::{
    ContainerEntry, ContainerRole, EntryStorage, VerbatimLabel, VerbatimSize,
};
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
    let body_offset: u64 = table.attributes["body_offset"]
        .parse()
        .expect("native summary witness");
    let offset: u64 = table.attributes["offset"]
        .parse()
        .expect("native summary witness");
    let EntryStorage::Verbatim {
        label,
        size: VerbatimSize::Framed(span),
    } = table.storage
    else {
        panic!("a rhino table reports framing overhead beside its body");
    };
    assert_eq!(label, VerbatimLabel::None);
    assert_eq!(span.framing(), body_offset - offset);
    assert_eq!(Some(span.payload()), table.expanded_size());
    assert_eq!(Some(span.stored().get()), table.stored_size());
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
    assert_eq!(storage.storage, EntryStorage::Directory);
    let stream = inventor
        .entries
        .iter()
        .find(|entry| entry.name == "RSeStorage/RSeSegInfo")
        .expect("native summary witness");
    assert_eq!(
        stream.storage,
        EntryStorage::unreported(VerbatimLabel::Stored)
    );
    assert_eq!(stream.stored_size(), None);
    assert_eq!(stream.expanded_size(), None);

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
    assert_eq!(
        references.storage,
        EntryStorage::unreported(VerbatimLabel::None)
    );
    assert_eq!(references.stored_size(), None);
    assert_eq!(references.expanded_size(), None);
    assert_eq!(references.attributes["external_count"], "1");
}
