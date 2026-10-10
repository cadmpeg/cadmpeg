// SPDX-License-Identifier: Apache-2.0

use cadmpeg_container::compound::CompoundSnapshot;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, CodecBackend, Confidence};

use super::{classify, insert_attribute, summary_note, InventorContainer};
use crate::test_support::test_fixtures::{fixture, primary_envelope_fixture_with_broken_metadata};
use crate::InventorCodec;

#[test]
fn inspection_scopes_parsed_container_storage() {
    let bytes = crate::test_support::test_fixtures::primary_envelope_fixture();
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("container setup context");
    let container = InventorContainer::open(&setup, root).expect("parsed container");
    let (output, _) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("output context");
    let expected = container.summary(&output).expect("summary projection");
    let CodecError::ResourceLimit(output_limit) =
        output.charge_retained(u64::MAX, "measure inspection output storage")
            .expect_err("the measurement exceeds the retained allowance")
    else {
        panic!("retained output measurement must be a resource refusal");
    };
    assert_eq!(output_limit.dimension, ResourceDimension::RetainedBytes);
    assert!(output_limit.used > 0);

    let mut policy = DecodePolicy::service();
    // Let R be the summary-only retained cost. The route must fit R, rather
    // than R plus parsed-container storage C. Temporary C is released on return.
    policy.limits.max_retained_bytes = output_limit.used;
    policy.limits.max_materialized_bytes = 1024 * 1024;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("exact output context");
    let actual = InventorCodec.inspect_impl(&ctx, root)
        .expect("only summary storage is retained");
    assert_eq!(actual, expected);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "reuse parsed container storage")
        .expect("all temporary storage is released"));
    assert!(matches!(ctx.charge_retained(1, "probe inspection output storage"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.used == output_limit.used
                && limit.additional == 1));
}

#[test]
fn summary_segment_walk_refuses_only_the_next_source_step() {
    use crate::rse::{SegmentBulkState, SegmentDescriptor, SegmentKind, SegmentMetaState};
    use crate::test_support::test_fixtures::{primary_envelope_fixture_with, EnvelopeDeclarations};
    let bytes = primary_envelope_fixture_with(EnvelopeDeclarations::default());
    for count in [1_usize, 512] {
        let arena = DecodeArena::new();
        let (setup, root) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
                .expect("summary fixture context");
        let mut container = InventorContainer::open(&setup, root).expect("summary fixture");
        let pair = container.rse.segments[0].pair.clone();
        container.rse.segments = (0..count)
            .map(|_| SegmentDescriptor {
                pair: pair.clone(),
                registry: None,
                kind: SegmentKind::Unresolved,
                identity_issues: Vec::new(),
                meta: SegmentMetaState::Malformed {
                    declared: None,
                    detail: "metadata".into(),
                },
                bulk: SegmentBulkState::Malformed("bulk".into()),
            })
            .collect();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &policy).expect("summary context");
        let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::WorkUnits,
            "visit Inventor summary segments",
            Some(1),
        );
        let error = container
            .summary(&ctx)
            .expect_err("first segment step refuses");
        assert!(matches!(&error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == "visit Inventor summary segments"
                && limit.additional == 1));
        drop(probe);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
            if matches!(&error, CodecError::ResourceLimit(original) if original == &limit))
        );
    }
}

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
    insert_attribute(&setup, entry, b"test", format_args!("value")).expect("service attribute");
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
            insert_attribute(&limited, entry, b"next", format_args!("value")),
            Err(CodecError::ResourceLimit(limit))
                if limit.dimension == dimension && limit.operation == operation
        ));
    }
}

#[test]
fn fixed_summary_attribute_key_needs_no_work_charge() {
    let mut entry = cadmpeg_core::ContainerEntry {
        name: String::new(),
        role: cadmpeg_core::container::ContainerRole::Storage,
        storage: cadmpeg_core::container::EntryStorage::Directory,
        attributes: std::collections::BTreeMap::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("fixed attribute context");
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "retain Inventor summary attribute key",
        None,
    );
    insert_attribute(&ctx, &mut entry, b"test", format_args!("value"))
        .expect("fixed key copy uses no input-sized work");
    drop(probe);
    assert_eq!(entry.attributes["test"], "value");
    ctx.finish_session()
        .expect("fixed attribute key leaves the session clean");
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
fn bounded_summary_note_admits_exact_storage_without_work() {
    for (major, segments, databases, expected) in [
        (
            0,
            0,
            0,
            "CFB v0 with 0 RSe segment pair(s) and 0 versioned database(s)",
        ),
        (
            3,
            1,
            1,
            "CFB v3 with 1 RSe segment pair(s) and 1 versioned database(s)",
        ),
        (
            u16::MAX,
            10,
            100,
            "CFB v65535 with 10 RSe segment pair(s) and 100 versioned database(s)",
        ),
    ] {
        for exact in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_retained_bytes =
                cadmpeg_core::decode::u64_from_index(expected.len()) - u64::from(!exact);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
                .expect("bounded summary note context");
            let result = summary_note(&ctx, major, segments, databases);
            if exact {
                assert_eq!(result.expect("exact summary note storage"), expected);
                ctx.finish_session().expect("bounded note needs no work");
            } else {
                let error = result.expect_err("note storage refuses before format");
                assert!(matches!(&error, CodecError::ResourceLimit(limit)
                    if limit.dimension == ResourceDimension::RetainedBytes
                        && limit.operation == "retain Inventor summary note"
                        && limit.used == 0
                        && limit.additional == cadmpeg_core::decode::u64_from_index(expected.len())));
                assert!(
                    matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
                    if matches!(&error, CodecError::ResourceLimit(original) if original == &limit))
                );
            }
        }
    }
}

#[test]
fn summary_note_vector_refuses_before_its_output_slot() {
    let bytes = fixture(true);
    let arena = DecodeArena::new();
    let (setup, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("summary fixture context");
    let container = InventorContainer::open(&setup, root).expect("summary fixture");
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64::MAX;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("summary note vector context");
    let probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::CollectionItems,
        "retain Inventor summary note entries",
        Some(1),
    );
    let error = container
        .summary(&ctx)
        .expect_err("note vector slot refuses");
    assert!(matches!(&error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "retain Inventor summary note entries"
            && limit.additional == 1));
    drop(probe);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit))
        if matches!(&error, CodecError::ResourceLimit(original) if original == &limit))
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
