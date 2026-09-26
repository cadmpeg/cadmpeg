// SPDX-License-Identifier: Apache-2.0

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, Confidence};

use super::{admit_container_entries, classify, find_summary_entry, insert_attribute};
use crate::test_support::test_fixtures::{fixture, primary_envelope_fixture_with_broken_metadata};
use crate::InventorCodec;

#[test]
fn container_summary_entries_refuse_limits_before_materialization() {
    let bytes = fixture(true);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("fixture snapshot");
    assert!(admit_container_entries(&setup, &snapshot).is_ok());
    for (collection_cap, retained_cap, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "collect Inventor container summary entries",
        ),
        (
            u64::MAX,
            0,
            ResourceDimension::RetainedBytes,
            "retain Inventor summary entry path",
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_cap;
        policy.limits.max_retained_bytes = retained_cap;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            admit_container_entries(&limited, &snapshot),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == dimension && limit.operation == operation
        ));
    }
}

#[test]
fn container_summary_attribute_refuses_before_insert() {
    let bytes = fixture(true);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("fixture snapshot");
    let mut entries = snapshot.container_entries(classify);
    let entry = entries.first_mut().expect("fixture entry");
    insert_attribute(&setup, entry, "test", format_args!("value")).expect("service attribute");
    assert_eq!(entry.attributes["test"], "value");
    for (collection_cap, retained_cap, dimension, operation) in [
        (
            0,
            u64::MAX,
            ResourceDimension::CollectionItems,
            "collect Inventor summary attribute",
        ),
        (
            u64::MAX,
            3,
            ResourceDimension::RetainedBytes,
            "retain Inventor summary attribute key",
        ),
        (
            u64::MAX,
            8,
            ResourceDimension::RetainedBytes,
            "retain Inventor summary attribute value",
        ),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_cap;
        policy.limits.max_retained_bytes = retained_cap;
        let (limited, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
        assert!(matches!(
            insert_attribute(&limited, entry, "test", format_args!("value")),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == dimension && limit.operation == operation
        ));
    }
}

#[test]
fn container_summary_search_refuses_work_limit_before_scan() {
    let bytes = fixture(true);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("fixture snapshot");
    let mut entries = snapshot.container_entries(classify);
    assert!(
        find_summary_entry(&setup, &mut entries, snapshot.entries()[0].directory_id())
            .expect("service search")
            .is_some()
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (limited, _) =
        DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("limited context");
    assert!(matches!(
        find_summary_entry(&limited, &mut entries, snapshot.entries()[0].directory_id()),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "find Inventor summary entry"
    ));
}

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
