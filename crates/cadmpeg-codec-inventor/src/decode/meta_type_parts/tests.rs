// SPDX-License-Identifier: Apache-2.0
//! Metadata descriptors retain their source fields and exact read ownership.

use crate::container::InventorContainer;
use crate::native::MetaTypeRecord;
use crate::rse::SegmentMetaState;
use crate::test_support::given_up_records;
use crate::test_support::test_fixtures::primary_envelope_fixture_with_design;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn fixture(id: [u8; 16]) -> Vec<u8> {
    primary_envelope_fixture_with_design(&[], &[(id, Vec::new()), ([0x22; 16], Vec::new())])
}

fn descriptors(bytes: &[u8]) -> Vec<MetaTypeRecord> {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    assert_eq!(container.rse.source_meta_type_count, 3);
    let output = container.rse.source_parts;
    let (_, native, _, losses) = output.into_parts();
    assert!(losses.is_empty());
    native
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arena_as::<MetaTypeRecord>("meta_types")
        .expect("the meta types arena holds meta type records")
}

#[test]
fn descriptors_keep_exact_source_values_reads_and_section_framing() {
    let bytes = fixture([0x11; 16]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.pair.token.as_str() == "dc")
        .expect("the container holds a dc segment");
    assert!(
        matches!(segment.meta, SegmentMetaState::Parsed(_)),
        "the actual metadata fixture must parse"
    );
    assert_eq!(container.rse.source_meta_type_count, 3);
    let output = container.rse.source_parts;
    let (_, native, fidelity, losses) = output.into_parts();
    assert!(losses.is_empty());
    let records = native
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arena_as::<MetaTypeRecord>("meta_types")
        .expect("the meta types arena holds meta type records");
    let actual = records
        .iter()
        .find(|record| record.token == "dc" && record.index == 0)
        .expect("the dc descriptor 0 is recorded");
    assert_eq!(
        actual,
        &MetaTypeRecord {
            id: "inventor:rse:meta-type#dc-0".into(),
            token: "dc".into(),
            index: 0,
            type_id: "11111111111111111111111111111111".into(),
            fields: [(1, 2), (3, 4)],
        }
    );
    assert!(given_up_records(&fidelity).is_empty());
}

#[test]
fn changing_the_actual_type_identifier_changes_only_its_native_descriptor() {
    let original = descriptors(&fixture([0x11; 16]));
    let changed = descriptors(&fixture([0x33; 16]));
    assert_eq!(original.len(), changed.len());
    for (original, changed) in original.iter().zip(&changed) {
        if original.token == "dc" && original.index == 0 {
            assert_eq!(original.type_id, "11111111111111111111111111111111");
            assert_eq!(changed.type_id, "33333333333333333333333333333333");
            assert_eq!(original.id, changed.id);
            assert_eq!(original.token, changed.token);
            assert_eq!(original.index, changed.index);
            assert_eq!(original.fields, changed.fields);
        } else {
            assert_eq!(original, changed);
        }
    }
}

#[test]
fn source_construction_does_not_credit_independently_changed_cached_values() {
    let bytes = fixture([0x11; 16]);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let mut container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    for segment in &mut container.rse.segments {
        if let SegmentMetaState::Parsed(meta) = &mut segment.meta {
            for descriptor in &mut meta.tables.types {
                descriptor.id = [0xff; 16];
                descriptor.fields = [(7, 8), (9, 10)];
            }
        }
    }
    let output = container.rse.source_parts;
    let (_, native, _, losses) = output.into_parts();
    assert!(losses.is_empty());
    let records = native
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arena_as::<MetaTypeRecord>("meta_types")
        .expect("the meta types arena holds meta type records");
    assert_eq!(records, descriptors(&bytes));
}

#[test]
fn descriptor_native_insertion_keeps_the_actual_session_entity_limit() {
    let bytes = fixture([0x11; 16]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
        .expect("the fixture fits the zero entity policy");
    let Err(CodecError::ResourceLimit(expected)) = InventorContainer::open(&ctx, root) else {
        panic!("native insertion must preserve the actual entity limit")
    };
    assert_eq!(expected.dimension, ResourceDimension::Entities);
}

#[test]
fn over_limit_type_section_is_given_up_before_guid_dispatch_and_keeps_the_exact_peer() {
    let design = vec![([0x11; 16], Vec::new()); 300];
    let bytes = primary_envelope_fixture_with_design(&[], &design);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let rejected = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.pair.token.as_str() == "dc")
        .expect("the container holds a dc segment");
    assert!(matches!(rejected.meta, SegmentMetaState::Malformed { .. }));
    let crate::rse::SegmentBulkState::Framed(bulk) = &rejected.bulk else {
        panic!("independent bulk member still frames")
    };
    assert!(matches!(
        bulk.records,
        crate::rse::RecordFrameState::Partial { .. }
    ));
    let projection = super::super::rse_native_projection::project(&ctx, &container)
        .expect("the rse native projection succeeds");
    assert!(projection.identity_issues.is_empty());
    assert!(projection.segment_meta_issues.is_empty());
    assert!(projection.segment_bulk_issues.is_empty());
    assert_eq!(projection.segment_meta.len(), 1);
    assert_eq!(projection.segment_meta[0].token, "seg");
    let peers = projection
        .meta_sections
        .iter()
        .filter(|section| section.token == "dc")
        .collect::<Vec<_>>();
    assert_eq!(peers.len(), 10);
    assert_eq!(
        peers
            .iter()
            .map(|section| u8::from(section.number))
            .collect::<Vec<_>>(),
        [1, 2, 3, 5, 6, 7, 8, 9, 10, 11]
    );
    assert!(!peers
        .iter()
        .any(|section| section.number == crate::records::MetaSectionNumber::Four));
    let blocks = peers
        .iter()
        .find(|section| section.number == crate::records::MetaSectionNumber::One)
        .expect("the dc peers include section one");
    assert_eq!(blocks.discriminator, 300);
    assert_eq!(blocks.payload_len, 1200);
    assert!(!projection
        .segment_bulk
        .iter()
        .any(|bulk| bulk.token == "dc"));
    let crate::rse::RecordFrameState::Partial {
        table,
        source_record_count,
    } = &bulk.records
    else {
        panic!("source-only body frames remain transient")
    };
    assert_eq!(*source_record_count, 300);
    assert!(table.records.is_empty());
    assert_eq!(table.stream_trailer.window(), &u32::MAX.to_le_bytes());
    assert_eq!(bulk.prefix, [0x3c; 16]);
    assert_eq!(bulk.form.value(), 0x0104);
    assert!(projection
        .rse_records
        .iter()
        .all(|record| record.token != "dc"));
    assert!(container
        .rse
        .source_parts
        .native()
        .namespace("inventor")
        .is_some());
    assert_eq!(container.rse.source_meta_type_count, 1);
    let output = container.rse.source_parts;
    let (_, native, fidelity, losses) = output.into_parts();
    let namespace = native
        .namespace("inventor")
        .expect("the inventor namespace exists");
    let records = namespace
        .arena_as::<MetaTypeRecord>("meta_types")
        .expect("the meta types arena holds meta type records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "inventor:rse:meta-type#seg-0");
    assert_eq!(records[0].token, "seg");
    assert_eq!(records[0].index, 0);
    assert_eq!(records[0].type_id, "5c5945f6d5113313100060a6bba647b5");
    assert_eq!(records[0].fields, [(1, 2), (3, 4)]);
    // The type table is the one given-up part; the records that need its
    // descriptors are not formed and are not given up on their own.
    let given_up = given_up_records(&fidelity);
    assert_eq!(given_up.len(), 1);
    // #215 rule 1: the given-up part keeps one unknowns home.
    let unknowns = namespace
        .arenas()
        .get("unknowns")
        .expect("one unknowns home");
    assert_eq!(unknowns.len(), 1);
    assert_eq!(
        unknowns[0].id(),
        fidelity
            .retained_records()
            .keys()
            .next()
            .expect("one retained part")
            .as_str()
    );
    let mut expected = 300_u32.to_le_bytes().to_vec();
    for _ in 0..300 {
        expected.extend([0x11; 16]);
        expected.extend(1_u16.to_le_bytes());
        expected.extend(2_u32.to_le_bytes());
        expected.extend(3_u16.to_le_bytes());
        expected.extend(4_u32.to_le_bytes());
    }
    expected.extend((4_u32 + 300 * 28).to_le_bytes());
    assert_eq!(given_up[0].data(), Some(expected.as_slice()));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.to_string(), "decode/record.unreadable");
    assert!(losses[0].message.contains("more than 256"));
}

#[test]
fn the_exact_maximum_type_table_remains_admitted() {
    let design = vec![([0x11; 16], Vec::new()); 256];
    let bytes = primary_envelope_fixture_with_design(&[], &design);
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service())
        .expect("the fixture fits the service policy");
    let container =
        InventorContainer::open(&ctx, root).expect("the fixture opens as an inventor container");
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.pair.token.as_str() == "dc")
        .expect("the container holds a dc segment");
    assert!(matches!(segment.meta, SegmentMetaState::Parsed(_)));
    let crate::rse::SegmentBulkState::Framed(bulk) = &segment.bulk else {
        panic!("actual bulk member frames")
    };
    let crate::rse::RecordFrameState::Framed(records) = &bulk.records else {
        panic!("actual source descriptors govern bulk dispatch")
    };
    assert_eq!(records.records.len(), 256);
    assert_eq!(records.records[255].type_id, [0x11; 16]);
    assert_eq!(container.rse.source_meta_type_count, 257);
    let (_, native, fidelity, losses) = container.rse.source_parts.into_parts();
    assert!(losses.is_empty());
    assert!(given_up_records(&fidelity).is_empty());
    let types = native
        .namespace("inventor")
        .expect("the inventor namespace exists")
        .arena_as::<MetaTypeRecord>("meta_types")
        .expect("the meta types arena holds meta type records");
    assert_eq!(types.len(), 257);
    assert!(types
        .iter()
        .any(|record| record.id == "inventor:rse:meta-type#dc-255"
            && record.index == 255
            && record.type_id == "11111111111111111111111111111111"));
}
