// SPDX-License-Identifier: Apache-2.0
//! Metadata descriptors retain their source fields and exact read ownership.

use crate::container::InventorContainer;
use crate::native::MetaTypeRecord;
use crate::rse::SegmentMetaState;
use crate::test_support::test_fixtures::primary_envelope_fixture_with_design;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartState;

fn fixture(id: [u8; 16]) -> Vec<u8> {
    primary_envelope_fixture_with_design(&[], &[(id, Vec::new()), ([0x22; 16], Vec::new())])
}

fn descriptors(bytes: &[u8]) -> Vec<MetaTypeRecord> {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    assert_eq!(container.rse.source_meta_type_count, 3);
    let output = container.rse.source_parts;
    let (_, native, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert!(losses.is_empty());
    native
        .namespace("inventor")
        .unwrap()
        .arena_as::<MetaTypeRecord>("meta_types")
        .unwrap()
}

#[test]
fn descriptors_keep_exact_source_values_reads_and_section_framing() {
    let bytes = fixture([0x11; 16]);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.pair.token.as_str() == "dc")
        .unwrap();
    let SegmentMetaState::Parsed(meta) = &segment.meta else {
        panic!("the actual metadata fixture must parse")
    };
    let frame = crate::records::frame_type_table(meta.body).unwrap();
    let entry = frame
        .payload
        .child(frame.payload.start(), frame.payload.start() + 28)
        .unwrap();
    let expected = ctx.capture_part_extent(entry).unwrap();
    let count = super::interval(&ctx, frame.count_source).unwrap();
    let span = super::interval(&ctx, frame.span_source).unwrap();
    let compressed = ctx.capture_part_extent(meta.compressed).unwrap();
    assert_eq!(container.rse.source_meta_type_count, 3);
    let output = container.rse.source_parts;
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert!(losses.is_empty());
    let records = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<MetaTypeRecord>("meta_types")
        .unwrap();
    let actual = records
        .iter()
        .find(|record| record.token == "dc" && record.index == 0)
        .unwrap();
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
    let (_, child) = fidelity
        .parts()
        .iter()
        .find(|(_, part)| {
            part.level().as_str() == "inventor.type_descriptor"
                && part.stream() == expected.stream()
                && part.offset() == expected.offset()
        })
        .unwrap();
    assert_eq!(child.byte_len(), 28);
    assert!(child.framing_bytes().is_empty());
    let section = &fidelity.parts()[child.parent().unwrap()];
    assert_eq!(section.level().as_str(), "inventor.meta_forward_section");
    assert_eq!(section.framing_bytes(), &[count, span]);
    let body = &fidelity.parts()[section.parent().unwrap()];
    assert_eq!(body.level().as_str(), "inventor.meta_body");
    assert_eq!(body.stream(), compressed.stream());
    assert_eq!(body.offset(), compressed.offset());
    assert_eq!(body.byte_len(), compressed.byte_len());
    let member = &fidelity.parts()[body.parent().unwrap()];
    assert_eq!(member.level().as_str(), "cfb.stream");
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 3);
    let capture = captures
        .iter()
        .find(|capture| capture.id() == actual.id)
        .unwrap();
    assert_eq!(capture.stream(), expected.stream());
    assert_eq!(capture.offset(), expected.offset());
    assert_eq!(capture.byte_len(), 28);
    assert_eq!(
        capture.reads()[expected.stream()],
        [expected.offset()..expected.offset() + 28]
    );
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
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let mut container = InventorContainer::open(&ctx, root).unwrap();
    for segment in &mut container.rse.segments {
        if let SegmentMetaState::Parsed(meta) = &mut segment.meta {
            for descriptor in &mut meta.tables.types {
                descriptor.id = [0xff; 16];
                descriptor.fields = [(7, 8), (9, 10)];
            }
        }
    }
    let output = container.rse.source_parts;
    let (_, native, _, losses) = output.into_parts(&ctx).unwrap().into_parts();
    assert!(losses.is_empty());
    let records = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<MetaTypeRecord>("meta_types")
        .unwrap();
    assert_eq!(records, descriptors(&bytes));
}

#[test]
fn descriptor_native_insertion_keeps_the_actual_session_entity_limit() {
    let bytes = fixture([0x11; 16]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 0;
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(expected)) = InventorContainer::open(&ctx, root) else {
        panic!("native insertion must preserve the actual entity limit")
    };
    assert_eq!(expected.dimension, ResourceDimension::Entities);
    assert!(
        matches!(ctx.take_record_captures(), Err(CodecError::ResourceLimit(actual)) if actual == expected)
    );
}

#[test]
fn over_limit_type_section_is_given_up_before_guid_dispatch_and_keeps_the_exact_peer() {
    let design = vec![([0x11; 16], Vec::new()); 300];
    let bytes = primary_envelope_fixture_with_design(&[], &design);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let rejected = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.pair.token.as_str() == "dc")
        .unwrap();
    assert!(matches!(rejected.meta, SegmentMetaState::Malformed { .. }));
    let crate::rse::SegmentBulkState::Framed(bulk) = &rejected.bulk else {
        panic!("independent bulk member still frames")
    };
    assert!(matches!(
        bulk.records,
        crate::rse::RecordFrameState::Partial { .. }
    ));
    let projection =
        super::super::rse_native_projection::project(&ctx, &container, &container.rse.source_parts)
            .unwrap();
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
        .unwrap();
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
    let (_, native, fidelity, losses) = output.into_parts(&ctx).unwrap().into_parts();
    let namespace = native.namespace("inventor").unwrap();
    let records = namespace.arena_as::<MetaTypeRecord>("meta_types").unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, "inventor:rse:meta-type#seg-0");
    assert_eq!(records[0].token, "seg");
    assert_eq!(records[0].index, 0);
    assert_eq!(records[0].type_id, "5c5945f6d5113313100060a6bba647b5");
    assert_eq!(records[0].fields, [(1, 2), (3, 4)]);
    let given_up = fidelity
        .parts()
        .values()
        .filter(|part| matches!(part.state(), PartState::GivenUp {}))
        .collect::<Vec<_>>();
    assert_eq!(given_up.len(), 301);
    let section = *given_up
        .iter()
        .find(|part| part.level().as_str() == "inventor.meta_forward_section")
        .unwrap();
    let mut bodies = given_up
        .iter()
        .filter(|part| part.level().as_str() == "inventor.rse_record_body")
        .copied()
        .collect::<Vec<_>>();
    bodies.sort_by_key(|part| part.offset());
    assert_eq!(bodies.len(), 300);
    for (index, body) in bodies.iter().enumerate() {
        assert_eq!(body.offset(), u64::try_from(index).unwrap() * 8);
        assert_eq!(body.byte_len(), 8);
        assert!(body.framing_bytes().is_empty());
        let parent = &fidelity.parts()[body.parent().unwrap()];
        assert_eq!(parent.level().as_str(), "inventor.bulk_body");
    }
    let trailer = fidelity
        .parts()
        .values()
        .find(|part| part.level().as_str() == "inventor.bulk_stream_trailer")
        .unwrap();
    assert_eq!(trailer.offset(), 2400);
    assert_eq!(trailer.byte_len(), 4);
    assert_eq!(trailer.framing_bytes(), &[2400..2404]);
    assert!(matches!(trailer.state(), PartState::Decoded {}));
    assert_eq!(section.level().as_str(), "inventor.meta_forward_section");
    assert_eq!(section.byte_len(), 8 + 300 * 28);
    assert_eq!(
        section.framing_bytes(),
        &[
            section.offset()..section.offset() + 4,
            section.offset() + 4 + 300 * 28..section.offset() + 8 + 300 * 28
        ]
    );
    assert_eq!(
        fidelity.parts()[section.parent().unwrap()].level().as_str(),
        "inventor.meta_body"
    );
    let unknowns = namespace
        .arena_as::<cadmpeg_ir::NativeUnknownRecord>("unknowns")
        .unwrap();
    assert_eq!(unknowns.len(), 301);
    let mut retained_bodies = unknowns
        .iter()
        .map(|unknown| fidelity.retained_record(unknown.id.as_str()).unwrap())
        .filter(|record| record.data().unwrap().len() == 8)
        .collect::<Vec<_>>();
    retained_bodies.sort_by_key(|record| record.offset());
    assert_eq!(retained_bodies.len(), 300);
    for (index, record) in retained_bodies.iter().enumerate() {
        let mut expected_body = u32::try_from(index).unwrap().to_le_bytes().to_vec();
        expected_body.extend(0_u32.to_le_bytes());
        assert_eq!(record.offset(), u64::try_from(index).unwrap() * 8);
        assert_eq!(record.stream(), bodies[index].stream());
        assert_eq!(record.data(), Some(expected_body.as_slice()));
    }
    let mut expected = 300_u32.to_le_bytes().to_vec();
    for _ in 0..300 {
        expected.extend([0x11; 16]);
        expected.extend(1_u16.to_le_bytes());
        expected.extend(2_u32.to_le_bytes());
        expected.extend(3_u16.to_le_bytes());
        expected.extend(4_u32.to_le_bytes());
    }
    expected.extend((4_u32 + 300 * 28).to_le_bytes());
    assert_eq!(
        fidelity
            .retained_record(
                unknowns
                    .iter()
                    .find(|unknown| fidelity
                        .retained_record(unknown.id.as_str())
                        .unwrap()
                        .data()
                        .unwrap()
                        .len()
                        == expected.len())
                    .unwrap()
                    .id
                    .as_str()
            )
            .unwrap()
            .data(),
        Some(expected.as_slice())
    );
    assert_eq!(losses.len(), 301);
    assert!(losses
        .iter()
        .all(|loss| loss.code.to_string() == "decode/record.unreadable"));
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.message.contains("more than 256"))
            .count(),
        1
    );
    assert_eq!(
        losses
            .iter()
            .filter(|loss| loss.message.contains("given-up metadata type table"))
            .count(),
        300
    );
    let captures = ctx.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "inventor:rse:meta-type#seg-0");
}

#[test]
fn the_exact_maximum_type_table_remains_admitted() {
    let design = vec![([0x11; 16], Vec::new()); 256];
    let bytes = primary_envelope_fixture_with_design(&[], &design);
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).unwrap();
    let container = InventorContainer::open(&ctx, root).unwrap();
    let segment = container
        .rse
        .segments
        .iter()
        .find(|segment| segment.pair.token.as_str() == "dc")
        .unwrap();
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
    let (_, native, fidelity, losses) = container
        .rse
        .source_parts
        .into_parts(&ctx)
        .unwrap()
        .into_parts();
    assert!(losses.is_empty());
    assert!(fidelity
        .parts()
        .values()
        .all(|part| matches!(part.state(), PartState::Decoded {})));
    let types = native
        .namespace("inventor")
        .unwrap()
        .arena_as::<MetaTypeRecord>("meta_types")
        .unwrap();
    assert_eq!(types.len(), 257);
    assert!(types
        .iter()
        .any(|record| record.id == "inventor:rse:meta-type#dc-255"
            && record.index == 255
            && record.type_id == "11111111111111111111111111111111"));
}
