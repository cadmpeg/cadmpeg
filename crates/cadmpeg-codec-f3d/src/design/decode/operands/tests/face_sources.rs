// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{
    face_source_carrier_spec, face_source_record_is_registered, parse_face_source_carrier_prefix,
    FaceSourceType,
};
use crate::test_support::write_marked_reference;

fn indexed_header(bytes: &mut Vec<u8>, class_tag: &[u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
}

fn source_carrier(
    class_tag: &[u8; 3],
    record_index: u32,
    scope_record_index: u32,
    source_count: usize,
    scalar_offset: usize,
    discriminator: u32,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, class_tag, record_index);
    bytes.resize(scalar_offset + 16, 0);
    write_marked_reference(&mut bytes, 21, scope_record_index);
    bytes[32..36].copy_from_slice(
        &(u32::try_from(source_count).expect("fixture value fits u32")).to_le_bytes(),
    );
    for ordinal in 0..source_count {
        let offset = 36 + ordinal * 11;
        write_marked_reference(
            &mut bytes,
            offset,
            200 + u32::try_from(ordinal).expect("fixture value fits u32"),
        );
    }
    bytes[scalar_offset..scalar_offset + 4].copy_from_slice(&discriminator.to_le_bytes());
    bytes[scalar_offset + 4..scalar_offset + 12].copy_from_slice(&0.125f64.to_le_bytes());
    bytes[scalar_offset + 12..scalar_offset + 16].copy_from_slice(&discriminator.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<Vec<(usize, u32)>> {
    crate::test_support::with_decode_context(|ctx| {
        parse_face_source_carrier_prefix(ctx, bytes, 0, bytes.len(), 12).unwrap()
    })
}

#[test]
fn face_source_carriers_follow_counts_and_repeat_the_opaque_discriminator() {
    for (class_tag, source_count, scalar_offset, discriminator) in [
        (*b"398", 4, 80, 100),
        (*b"394", 2, 58, 109),
        (*b"356", 2, 58, 109),
        (*b"414", 1, 47, 110),
        (*b"777", 3, 69, 252),
    ] {
        let layout = face_source_carrier_spec(source_count).unwrap();
        assert_eq!(layout.source_count, source_count);
        assert_eq!(layout.scalar_offset, scalar_offset);
        let bytes = source_carrier(
            &class_tag,
            100,
            12,
            source_count,
            scalar_offset,
            discriminator,
        );
        let references = parse(&bytes).unwrap();
        assert_eq!(
            references,
            (0..source_count)
                .map(|ordinal| (
                    36 + ordinal * 11,
                    200 + u32::try_from(ordinal).expect("fixture value fits u32"),
                ))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn face_source_carrier_prefix_rejects_unbounded_counts_and_inconsistent_lanes() {
    let bytes = source_carrier(b"398", 100, 12, 4, 80, 100);
    for count in [0u32, 3, u32::MAX] {
        let mut invalid = bytes.clone();
        invalid[32..36].copy_from_slice(&count.to_le_bytes());
        assert!(parse(&invalid).is_none());
    }
    let mut invalid = bytes.clone();
    invalid[80..84].copy_from_slice(&101u32.to_le_bytes());
    assert!(parse(&invalid).is_none());
    let mut invalid = bytes.clone();
    invalid[84..92].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(parse(&invalid).is_none());
    for offset in [11, 26, 41] {
        let mut invalid = bytes.clone();
        invalid[offset] = 1;
        assert!(parse(&invalid).is_none());
    }
    let mut invalid = bytes;
    invalid[37..41].fill(0);
    assert!(parse(&invalid).is_none());
}

#[test]
fn damaged_face_source_carrier_keeps_its_indexed_sibling() {
    let layout = face_source_carrier_spec(2).unwrap();
    let mut bytes = source_carrier(b"394", 100, 12, 2, layout.scalar_offset, 109);
    let scalar = layout.scalar_offset + std::mem::size_of::<u32>();
    bytes[scalar..scalar + std::mem::size_of::<f64>()].copy_from_slice(&f64::NAN.to_le_bytes());
    let second = bytes.len();
    bytes.extend_from_slice(&source_carrier(
        b"394",
        101,
        12,
        2,
        layout.scalar_offset,
        109,
    ));
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 100,
            start: 0,
            member_end: second,
            end: second,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 101,
            start: second,
            member_end: bytes.len(),
            end: bytes.len(),
        },
    ];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, source) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let decoded: Vec<_> = frames
        .iter()
        .filter_map(|frame| {
            crate::design::decode::operands::decode_face_source_carrier_part(
                &ctx,
                &mut parts,
                source,
                "Design/BulkStream.dat",
                frame,
                frame.end,
                12,
            )
            .unwrap()
        })
        .collect();
    assert_eq!(decoded.len(), 1);
    assert_eq!(
        decoded[0]
            .iter()
            .map(|(_, index)| *index)
            .collect::<Vec<_>>(),
        [200, 201]
    );
    let (_, native, fidelity, losses) = parts.into_parts();
    assert_eq!(
        native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert!(fidelity
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
}

#[test]
fn face_source_identity_uses_registration_and_segment_instead_of_class_literals() {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    use crate::records::identity::ReferenceRun;
    let mut types = vec![SegmentType::try_new(
        "f3d:test/Design1/MetaStream.dat:design-type#10".into(),
        SegmentTypeData {
            class_tag: "256".to_owned().try_into().unwrap(),
            byte_offset: Some(10),
            type_guid: "CAA55628-037B-4D01-9C74-26118F70B63E"
                .to_owned()
                .try_into()
                .unwrap(),
            type_guid_offset: Some(14),
            base_type_guid: BaseTypeGuid::Absent,
            version: 0,
            version_offset: Some(50),
            module: "Fusion".into(),
            entities: ReferenceRun::unlocated(vec![100]),
        },
    )
    .unwrap()];
    let source = "f3d:test/Design1/BulkStream.dat:design-parameter-scope#0";
    let matches = |types: &[SegmentType], source, class| {
        face_source_record_is_registered(types, source, 100, class, FaceSourceType::Carrier)
    };
    assert!(matches(&types, source, "256"));
    assert!(!matches(&types, source, "398"));
    assert!(!matches(
        &types,
        "f3d:other/Design1/BulkStream.dat:scope#0",
        "256"
    ));
    types[0].set_type_guid(
        "F0130424-8B7E-4092-93C9-1CA807482534"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    assert!(!matches(&types, source, "256"));
}

#[test]
fn damaged_face_source_identity_keeps_its_indexed_sibling() {
    fn identity(index: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, b"290", index);
        bytes.extend_from_slice(&586u64.to_le_bytes());
        crate::test_support::lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
        crate::test_support::lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&[0; 5]);
        bytes
    }
    let mut bytes = identity(200);
    bytes[11] = 1;
    let second = bytes.len();
    bytes.extend_from_slice(&identity(201));
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 200,
            start: 0,
            member_end: second,
            end: second,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 201,
            start: second,
            member_end: bytes.len(),
            end: bytes.len(),
        },
    ];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, source) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let decoded: Vec<_> = frames
        .iter()
        .filter_map(|frame| {
            crate::design::decode::operands::decode_face_source_identity_part(
                &ctx,
                &mut parts,
                source,
                "Design/BulkStream.dat",
                frame,
                (frame.entity_id == 200).then_some(201),
            )
            .unwrap()
        })
        .collect();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].local_id, 586);
    let (_, native, fidelity, losses) = parts.into_parts();
    assert_eq!(
        native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert!(fidelity
        .unrepresented()
        .contains(&cadmpeg_ir::document::ArenaClass::Design));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code.local_code(), "record.unreadable");
}

#[test]
fn face_source_identity_survives_a_damaged_following_header() {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, b"290", 200);
    bytes.extend_from_slice(&586u64.to_le_bytes());
    crate::test_support::lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    crate::test_support::lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    let end = bytes.len();
    bytes.extend_from_slice(&[0xff; 11]);
    let frame = crate::metastream::PrimaryRecordFrame {
        entity_id: 200,
        start: 0,
        member_end: end,
        end,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, source) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut parts =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let member = crate::design::decode::operands::decode_face_source_identity_part(
        &ctx,
        &mut parts,
        source,
        "Design/BulkStream.dat",
        &frame,
        Some(201),
    )
    .unwrap()
    .unwrap();
    assert_eq!(member.local_id, 586);
    assert_eq!(member.next_record_index, 201);
    assert_eq!(member.next_byte_offset, u64::try_from(end).unwrap());
    let (_, _, _, losses) = parts.into_parts();
    assert!(losses.is_empty());
}
