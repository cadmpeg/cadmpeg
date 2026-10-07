// SPDX-License-Identifier: Apache-2.0

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, Confidence};

use super::{classify, insert_attribute, summary_note, InventorContainer};
use crate::test_support::test_fixtures::{fixture, primary_envelope_fixture_with_broken_metadata};
use crate::InventorCodec;

#[test]
fn container_summary_attribute_refuses_before_insert() {
    let bytes = fixture(true);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let snapshot = CompoundSnapshot::new(&setup, root).expect("fixture snapshot");
    let mut entries = snapshot
        .container_entries(&setup, |entry| {
            classify(&setup, entry).expect("classification")
        })
        .expect("summary admission");
    let entry = entries.first_mut().expect("fixture entry");
    insert_attribute(&setup, entry, "test", format_args!("value")).expect("service attribute");
    assert_eq!(entry.attributes["test"], "value");
    // The retained key is four bytes; the five-byte value is admitted before the map node.
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
            insert_attribute(&limited, entry, "next", format_args!("value")),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == dimension && limit.operation == operation
        ));
    }
}

#[test]
fn container_summary_note_refuses_before_text_creation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    assert!(matches!(
        summary_note(&ctx, 3, 1, 1),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "retain Inventor summary note"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    assert_eq!(
        summary_note(&ctx, 3, 1, 1).expect("admitted note"),
        "CFB v3 with 1 RSe segment pair(s) and 1 versioned database(s)"
    );
}

#[test]
fn container_summary_loss_slot_refuses_before_loss_construction() {
    let bytes = primary_envelope_fixture_with_broken_metadata();
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("service context");
    let container = InventorContainer::open(&setup, root).expect("fixture container");
    let recovery = crate::dialect::DialectRecovery::of(&setup, &container).expect("recovery");
    let matched = recovery.classify(&setup).expect("classification");
    assert!(!matches!(
        matched.admission(),
        cadmpeg_core::dialect::Admission::Admitted
    ));
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("limited context");
    let mut losses = Vec::<cadmpeg_ir::report::loss::LossNote>::new();
    assert!(matches!(
        ctx.reserve_vec(&mut losses, 1, "collect Inventor summary loss"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "collect Inventor summary loss"
    ));
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("service context");
    ctx.reserve_vec(&mut losses, 1, "collect Inventor summary loss")
        .expect("admitted loss slot");
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
