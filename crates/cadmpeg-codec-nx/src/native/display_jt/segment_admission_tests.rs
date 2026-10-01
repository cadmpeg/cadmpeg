// SPDX-License-Identifier: Apache-2.0
//! Segment and shape-element allocation admission.

use cadmpeg_core::decode::{ResourceDimension, View};
use cadmpeg_core::CodecError;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

use crate::container::{Container, DirEntry, DirEntryBody, Region};

fn compressed_member() -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder.write_all(b"DisplayJT payload").unwrap();
    encoder.finish().unwrap()
}

#[test]
fn display_jt_inflate_propagates_expansion_limit() {
    let member = compressed_member();
    let source = View::over_retained(&member);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_decompressed_bytes_total = 16;
        },
        |ctx| {
            let error = super::inflate_display_jt(ctx, source).unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::DecompressedBytes));
            crate::test_support::with_decode_context(|service| {
                assert_eq!(
                    super::inflate_display_jt(service, source)
                        .unwrap()
                        .as_deref(),
                    Some(b"DisplayJT payload".as_slice())
                );
            });
        },
    );
}

#[test]
fn display_jt_inflate_propagates_retained_copy_limit() {
    let member = compressed_member();
    let source = View::over_retained(&member);

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_retained_bytes = 16;
        },
        |ctx| {
            let error = super::inflate_display_jt(ctx, source).unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "retain inflated DisplayJT payload"));
            crate::test_support::with_decode_context(|service| {
                assert_eq!(
                    super::inflate_display_jt(service, source)
                        .unwrap()
                        .as_deref(),
                    Some(b"DisplayJT payload".as_slice())
                );
            });
        },
    );
}

#[test]
fn display_jt_owned_inflate_omits_payload_copy_work() {
    let member = compressed_member();
    let work = cadmpeg_core::decode::u64_from_index(member.len() + 8192 + b"DisplayJT payload".len());
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_work_units = work;
    }, |ctx| {
        let buffer = super::inflate_display_jt(ctx, View::over_retained(&member)).unwrap().unwrap();
        assert_eq!(buffer, b"DisplayJT payload");
        assert!(ctx.resource_refusal().is_none());
        let error = ctx.charge_work(1, "verify expansion work").unwrap_err();
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::WorkUnits && limit.used == work && limit.additional == 1));
    });
}

#[test]
fn display_jt_segment_entity_refuses_before_identity_and_record_allocation() {
    let mut container = super::document_admission_tests::one_document();
    let data = container.data.to_mut();
    data[165..181].copy_from_slice(&[2; 16]);
    data[181..185].copy_from_slice(&1_u32.to_le_bytes());
    data[185..189].copy_from_slice(&24_u32.to_le_bytes());

    crate::test_support::with_decode_context(|index_ctx| {
        let indices = super::display_jt_indices(index_ctx, &container).unwrap();
        let documents = super::display_jt_documents(index_ctx, &container, &indices).unwrap();

        crate::test_support::with_decode_context_over(
            &[],
            |policy| {
                policy.limits.max_entities = 0;
            },
            |ctx| {
                let source = View::over_retained(container.data.as_ref());
                let error =
                    super::display_jt_segments((ctx, source), &container, &documents).unwrap_err();
                assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::Entities
            && limit.operation == "store DisplayJT segment"));
                crate::test_support::with_decode_context(|service| {
                    let segments =
                        super::display_jt_segments((service, source), &container, &documents)
                            .unwrap();
                    assert_eq!(segments.len(), 1);
                });
            },
        );
    });
}

#[test]
fn display_jt_shape_element_entity_refuses_before_identity_and_record_allocation() {
    let mut data = Vec::new();
    data.extend_from_slice(&[1; 16]);
    data.extend_from_slice(&7_u32.to_le_bytes());
    data.extend_from_slice(&78_u32.to_le_bytes());
    data.extend_from_slice(&24_u32.to_le_bytes());
    data.extend_from_slice(&[0x5a; 16]);
    data.push(4);
    data.extend_from_slice(&42_u32.to_le_bytes());
    data.extend_from_slice(&[9, 8, 7]);
    data.extend_from_slice(&16_u32.to_le_bytes());
    data.extend_from_slice(&[0xff; 16]);
    data.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    let data_len = cadmpeg_core::decode::u64_from_index(data.len());
    let container = Container {
        data: data.into(),
        physical_size: data_len,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".to_owned(),
            region: Region::Header,
            body: DirEntryBody::File {
                offset: 0,
                len: data_len,
            },
        }],
        fastload_table: None,
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    };
    let segment = super::DisplayJtSegment {
        id: "nx:display-jt:segment#0-0".to_owned(),
        document: "document".to_owned(),
        toc_entry: "entry".to_owned(),
        segment_id: [1; 16],
        segment_type: 7,
        segment_byte_len: 78,
        payload_sha256: cadmpeg_ir::hash::digest::Sha256Digest::digest(&[]),
        compression: None,
        source_offset: 0,
    };

    crate::test_support::with_decode_context_over(
        &[],
        |policy| {
            policy.limits.max_entities = 0;
        },
        |ctx| {
            let source = View::over_retained(container.data.as_ref());
            let error = super::display_jt_shape_lod_elements(
                (ctx, source),
                &container,
                std::slice::from_ref(&segment),
            )
            .unwrap_err();
            assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::Entities
            && limit.operation == "store DisplayJT shape element"));
            crate::test_support::with_decode_context(|service| {
                let elements =
                    super::display_jt_shape_lod_elements((service, source), &container, &[segment])
                        .unwrap();
                assert_eq!(elements.len(), 1);
            });
        },
    );
}
