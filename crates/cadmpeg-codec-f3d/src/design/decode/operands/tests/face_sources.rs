// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::{
    decode_face_source_groups, face_source_carrier_layout, face_source_reference_headers,
    parse_face_source_carrier_prefix,
};
use crate::test_support::write_marked_reference;

#[test]
fn face_source_reference_headers_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    let (service, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let records =
        crate::design::decode::sketch::IndexedRecordOffsets::build(&service, &[]).unwrap();
    policy.limits.max_collection_items = 1;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        face_source_reference_headers(
            &ctx,
            &[],
            0,
            &crate::records::identity::ReferenceRun::unlocated(vec![7u32, 8]),
            &records,
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d face source reference headers"
    ));
}

#[test]
fn face_source_output_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let group = crate::records::topology::face::DesignFaceSourceGroup {
        id: "f3d:design:design-face-source-group#0".to_owned(),
        scope_record_index: 12,
        carrier_reference_ordinal: 0,
        carrier_record_index: 7,
        carrier_span: crate::records::identity::NonEmptyByteSpan::new(0, 11).unwrap(),
        carrier_class_tag: crate::records::references::DesignClassTag::try_from("398".to_owned())
            .unwrap(),
        paired_record_index: 7,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("462".to_owned())
            .unwrap(),
        source_members: Vec::new(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;

    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut out = Vec::new();
    assert!(matches!(
        ctx.push_vec(&mut out, group, "f3d face source group output"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "f3d face source group output"
    ));
    assert!(out.is_empty());
}

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

#[test]
fn face_source_carriers_use_generation_keyed_prefixes() {
    for (class_tag, source_count, scalar_offset, discriminator, paired_class_tag) in [
        (*b"398", 4, 80, 100, "462"),
        (*b"394", 2, 58, 109, "311"),
        (*b"356", 2, 58, 109, "309"),
    ] {
        let layout = face_source_carrier_layout(class_tag_str(&class_tag)).unwrap();
        assert_eq!(layout.source_count, source_count);
        assert_eq!(layout.scalar_offset, scalar_offset);
        assert_eq!(layout.scalar_discriminator, discriminator);
        assert_eq!(layout.paired_class_tag, paired_class_tag);

        let bytes = source_carrier(
            &class_tag,
            100,
            12,
            source_count,
            scalar_offset,
            discriminator,
        );
        let references = crate::test_support::with_decode_context(|ctx| {
            parse_face_source_carrier_prefix(ctx, &bytes, 0, 12, layout).expect("service admission")
        })
        .unwrap();
        assert_eq!(
            references,
            (0..source_count)
                .map(|ordinal| (
                    36 + ordinal * 11,
                    200 + u32::try_from(ordinal).expect("fixture value fits u32")
                ))
                .collect::<Vec<_>>()
        );
    }
}

fn class_tag_str(class_tag: &[u8; 3]) -> &str {
    std::str::from_utf8(class_tag).unwrap()
}

#[test]
fn face_source_carrier_prefix_rejects_wrong_count_and_discriminator() {
    let layout = face_source_carrier_layout("398").unwrap();
    let mut bytes = source_carrier(b"398", 100, 12, 4, 80, 100);

    bytes[32..36].copy_from_slice(&3u32.to_le_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| parse_face_source_carrier_prefix(
            ctx, &bytes, 0, 12, layout
        )
        .expect("service admission"))
        .is_none()
    );

    let layout = face_source_carrier_layout("398").unwrap();
    let mut bytes = source_carrier(b"398", 100, 12, 4, 80, 100);
    bytes[80..84].copy_from_slice(&101u32.to_le_bytes());
    assert!(
        crate::test_support::with_decode_context(|ctx| parse_face_source_carrier_prefix(
            ctx, &bytes, 0, 12, layout
        )
        .expect("service admission"))
        .is_none()
    );
}

#[test]
fn face_source_reference_storage_has_fixed_capacity() {
    for (tag, count, offset, discriminator) in [(b"398", 4, 80, 100), (b"394", 2, 58, 109)] {
        let layout = face_source_carrier_layout(class_tag_str(tag)).unwrap();
        let bytes = source_carrier(tag, 100, 12, count, offset, discriminator);
        let references = crate::test_support::with_decode_context(|ctx| {
            parse_face_source_carrier_prefix(ctx, &bytes, 0, 12, layout).expect("service admission")
        })
        .unwrap();
        assert_eq!(references.len(), count);
        assert_eq!(references.capacity(), 4);
    }
}

#[test]
fn face_source_reference_push_refuses_each_collection_item() {
    let layout = face_source_carrier_layout("398").unwrap();
    let bytes = source_carrier(b"398", 100, 12, 4, 80, 100);
    let references = crate::test_support::with_decode_context(|ctx| {
        parse_face_source_carrier_prefix(ctx, &bytes, 0, 12, layout)
            .expect("valid FaceSource carrier")
    })
    .expect("four FaceSource references");
    assert_eq!(references.len(), 4);

    for skip in 0..4 {
        let refusal = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            "collect F3D face source references",
            skip,
            |ctx| parse_face_source_carrier_prefix(ctx, &bytes, 0, 12, layout).map(|_| ()),
        );
        assert!(matches!(
            refusal,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
                    && limit.operation == "collect F3D face source references"
                    && limit.additional == 1
        ));
    }
}

#[test]
fn face_source_member_vector_refuses_collection_limit() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::identity::ReferenceRun;

    let stream_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = Vec::new();
    crate::test_support::indexed_header(&mut bulk, *b"300", 10);

    let carrier_at = bulk.len();
    crate::test_support::indexed_header(&mut bulk, *b"398", 200);
    bulk.resize(carrier_at + 96, 0);
    crate::test_support::write_marked_reference(&mut bulk, carrier_at + 21, 10);
    bulk[carrier_at + 32..carrier_at + 36].copy_from_slice(&4_u32.to_le_bytes());
    for (ordinal, record_index) in [202_u32, 203, 204, 205].into_iter().enumerate() {
        crate::test_support::write_marked_reference(
            &mut bulk,
            carrier_at + 36 + ordinal * 11,
            record_index,
        );
    }
    bulk[carrier_at + 80..carrier_at + 84].copy_from_slice(&100_u32.to_le_bytes());
    bulk[carrier_at + 84..carrier_at + 92].copy_from_slice(&0.125_f64.to_le_bytes());
    bulk[carrier_at + 92..carrier_at + 96].copy_from_slice(&100_u32.to_le_bytes());
    crate::test_support::indexed_header(&mut bulk, *b"462", 201);
    for record_index in [202_u32, 203, 204, 205] {
        let mut member = Vec::new();
        crate::test_support::indexed_header(&mut member, *b"286", record_index);
        member.extend_from_slice(&[0; 10]);
        member.extend_from_slice(&u64::from(record_index).to_le_bytes());
        crate::test_support::lp_utf16(
            &mut member,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        );
        crate::test_support::lp_utf16(
            &mut member,
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
        );
        member.extend_from_slice(&2_u32.to_le_bytes());
        member.push(0);
        member.extend_from_slice(&[0; 4]);
        bulk.extend(member);
    }

    let mut scope = DesignParameterScope::empty(
        "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Face,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![200, 201]);
            draft.layout_fixture_references();
        })
        .unwrap();
    let archive = crate::test_support::zip_test::f3d_with_configuration(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
        stream_name,
        &bulk,
    );

    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let groups = crate::test_support::with_decode_context(|ctx| {
            decode_face_source_groups(ctx, scan, std::slice::from_ref(&scope))
        })
        .expect("valid face-source group");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source_members.len(), 4);

        let refusal = crate::test_support::resource_refusal_at(
            ResourceDimension::CollectionItems,
            "collect F3D face source members",
            0,
            |ctx| {
                decode_face_source_groups(ctx, scan, std::slice::from_ref(&scope)).map(|_| ())
            },
        );
        assert!(matches!(
            refusal,
            CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "collect F3D face source members"
                    && limit.additional == 1
        ));
    });
}
