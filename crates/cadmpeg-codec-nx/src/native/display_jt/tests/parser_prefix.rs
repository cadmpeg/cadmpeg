// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::container::{Container, DirEntry, DirEntryBody, Region};
use crate::test_support::{resource_refusal_at, with_decode_context, with_decode_context_over};
use cadmpeg_core::decode::{ResourceDimension, View};

const CANDIDATE_STORAGE_LIMIT: u64 = 1024 * 1024;

fn container(data: Vec<u8>) -> Container<'static> {
    let physical_size = cadmpeg_core::decode::u64_from_index(data.len());
    Container {
        data: data.into(),
        physical_size,
        layout: crate::container::test_modern_layout(6),
        entries: vec![DirEntry {
            name: "/Root/UG_PART/DisplayJT".into(),
            region: Region::Footer,
            body: DirEntryBody::File { offset: 0, len: physical_size },
        }],
        fastload_table: None,
        segment_index: None,
        segment_wrappers: Vec::new(),
        indexed_section_layouts: std::sync::OnceLock::new(),
        om_section_cache: std::sync::OnceLock::new(),
    }
}

fn row(ordinal: u32, header_offset: u32) -> DisplayJtIndexRow {
    DisplayJtIndexRow {
        id: format!("nx:display-jt:index#0-row-{ordinal}"),
        ordinal,
        header_offset,
        value: NonZeroU64::new(1).unwrap(),
        source_offset: 0,
    }
}

fn document_header(toc_count: u32) -> Vec<u8> {
    let mut bytes = format!("{:<80}", "Version +0009.004 JT").into_bytes();
    bytes.push(0);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&105_u32.to_le_bytes());
    bytes.extend_from_slice(&[1; 16]);
    bytes.extend_from_slice(&toc_count.to_le_bytes());
    bytes
}

#[test]
fn jt_range_values_stop_after_the_first_nonfinite_value() {
    let mut bytes = 4097_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&f32::INFINITY.to_le_bytes());
    bytes.extend_from_slice(&1.0_f32.to_le_bytes().repeat(4096));
    with_decode_context_over(&[], |policy| {
        policy.limits.max_work_units = 1;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = CANDIDATE_STORAGE_LIMIT;
    }, |ctx| {
        let mut view = View::over_retained(&bytes);
        assert!(parse_jt_f32_vector(ctx, &mut view).unwrap().is_none());
        assert_eq!(view.position(), 8);
        assert!(ctx.resource_refusal().is_none());
        let storage = ctx.reserve_scoped(CANDIDATE_STORAGE_LIMIT, "reuse rejected JT range storage").unwrap();
        drop(storage);
    });
}

#[test]
fn jt_partition_name_stops_after_the_first_control_character() {
    let mut bytes = 1_u16.to_le_bytes().to_vec();
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&4097_u32.to_le_bytes());
    bytes.extend_from_slice(&u16::from(b'\n').to_le_bytes());
    bytes.extend_from_slice(&u16::from(b'x').to_le_bytes().repeat(4096));
    let CodecError::ResourceLimit(limit) = resource_refusal_at(
        &[], ResourceDimension::WorkUnits, "validate DisplayJT partition name",
        |ctx| parse_jt9_partition_node_body(ctx, &bytes),
    ) else { panic!("partition name validation must refuse"); };
    assert_eq!(limit.additional, 1);
    // UTF-16 decoding and owning the name precede its control-character check.
    with_decode_context_over(&[], |policy| {
        policy.limits.max_work_units = limit.used + 1;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = CANDIDATE_STORAGE_LIMIT;
    }, |ctx| {
        assert!(parse_jt9_partition_node_body(ctx, &bytes).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
        let storage = ctx.reserve_scoped(CANDIDATE_STORAGE_LIMIT, "reuse rejected JT partition storage").unwrap();
        drop(storage);
    });
}

#[test]
fn jt_index_rows_stop_after_the_first_zero_value() {
    let mut bytes = 9_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&4097_u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 16].repeat(4097));
    let container = container(bytes);
    let CodecError::ResourceLimit(limit) = resource_refusal_at(
        container.data.as_ref(), ResourceDimension::WorkUnits, "scan DisplayJT index rows",
        |ctx| display_jt_indices(ctx, &container),
    ) else { panic!("index row visit must refuse"); };
    assert_eq!(limit.additional, 1);
    with_decode_context_over(container.data.as_ref(), |policy| {
        policy.limits.max_work_units = limit.used + 1;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = CANDIDATE_STORAGE_LIMIT;
    }, |ctx| {
        assert!(display_jt_indices(ctx, &container).unwrap().is_empty());
        assert!(ctx.resource_refusal().is_none());
        let storage = ctx.reserve_scoped(CANDIDATE_STORAGE_LIMIT, "reuse rejected JT index storage").unwrap();
        drop(storage);
    });
}

#[test]
fn jt_documents_stop_after_the_first_short_header() {
    let container = container(vec![0; 96]);
    let indices = [DisplayJtIndex::new("nx:display-jt:index#0".into(), 9, vec![row(0, 0), row(1, 48)], 0).unwrap()];
    let CodecError::ResourceLimit(limit) = resource_refusal_at(
        container.data.as_ref(), ResourceDimension::WorkUnits, "scan DisplayJT document",
        |ctx| display_jt_documents(ctx, &container, &indices),
    ) else { panic!("document visit must refuse"); };
    assert_eq!(limit.additional, 1);
    with_decode_context_over(container.data.as_ref(), |policy| policy.limits.max_work_units = limit.used + 1, |ctx| {
        assert!(display_jt_documents(ctx, &container, &indices).unwrap().is_empty());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn jt_toc_stops_after_the_first_empty_segment() {
    let mut bytes = document_header(4097);
    bytes.extend_from_slice(&[0; jt_toc::LEN].repeat(4097));
    let container = container(bytes);
    let indices = [DisplayJtIndex::new("nx:display-jt:index#0".into(), 9, vec![row(0, 0)], 0).unwrap()];
    let CodecError::ResourceLimit(limit) = resource_refusal_at(
        container.data.as_ref(), ResourceDimension::WorkUnits, "scan DisplayJT table of contents",
        |ctx| display_jt_documents(ctx, &container, &indices),
    ) else { panic!("TOC visit must refuse"); };
    assert_eq!(limit.additional, 1);
    with_decode_context_over(container.data.as_ref(), |policy| policy.limits.max_work_units = limit.used + 1, |ctx| {
        assert!(display_jt_documents(ctx, &container, &indices).unwrap().is_empty());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn jt_inline_version_still_admits_enclosing_document_storage() {
    let mut bytes = document_header(1);
    bytes.extend_from_slice(&[2; 16]);
    bytes.extend_from_slice(&137_u32.to_le_bytes());
    bytes.extend_from_slice(&24_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&[2; 16]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&24_u32.to_le_bytes());
    let container = container(bytes);
    let indices = [DisplayJtIndex::new("nx:display-jt:index#0".into(), 9, vec![row(0, 0)], 0).unwrap()];
    let documents = with_decode_context_over(container.data.as_ref(), |_| {}, |ctx| display_jt_documents(ctx, &container, &indices)).unwrap();
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].version.as_str(), format!("{:<80}", "Version +0009.004 JT"));
    let CodecError::ResourceLimit(limit) = resource_refusal_at(
        container.data.as_ref(), ResourceDimension::MaterializedBytes, "admit DisplayJT document",
        |ctx| display_jt_documents(ctx, &container, &indices),
    ) else { panic!("enclosing document storage must refuse"); };
    assert_eq!(limit.additional, cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<DisplayJtDocument>()));
    let CodecError::ResourceLimit(limit) = resource_refusal_at(
        container.data.as_ref(), ResourceDimension::RetainedBytes, "DisplayJT document candidates",
        |ctx| display_jt_documents(ctx, &container, &indices),
    ) else { panic!("accepted document storage must refuse retention"); };
    assert!(limit.additional >= cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<DisplayJtDocument>()));
    with_decode_context(|ctx| {
        let original = ctx.refuse_codec_limit("prior JT failure", 0, 1);
        let refused = display_jt_documents(ctx, &container, &indices).unwrap_err();
        assert!(matches!((original, refused), (CodecError::ResourceLimit(first), CodecError::ResourceLimit(second)) if first == second));
    });
}

#[test]
fn jt_documents_release_a_valid_prefix_when_a_later_header_is_short() {
    let mut bytes = document_header(1);
    bytes.extend_from_slice(&[2; 16]);
    bytes.extend_from_slice(&137_u32.to_le_bytes());
    bytes.extend_from_slice(&24_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_be_bytes());
    bytes.extend_from_slice(&[2; 16]);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&24_u32.to_le_bytes());
    assert_eq!(bytes.len(), 161);
    bytes.extend_from_slice(&[0; 48]);
    let container = container(bytes);
    let indices = [DisplayJtIndex::new("nx:display-jt:index#0".into(), 9, vec![row(0, 0), row(1, 161)], 0).unwrap()];
    with_decode_context_over(container.data.as_ref(), |policy| {
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = CANDIDATE_STORAGE_LIMIT;
    }, |ctx| {
        assert!(display_jt_documents(ctx, &container, &indices).unwrap().is_empty());
        let storage = ctx.reserve_scoped(CANDIDATE_STORAGE_LIMIT, "reuse rejected JT document storage").unwrap();
        drop(storage);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn jt_tessellation_identity_refusal_holds_its_input_until_final_formatting() {
    let invalid = "nx:display-jt:tessellation#7-2-path-bad path";
    let expected = format!("display-jt tessellation: identity is invalid: {invalid:?}");
    let materialized = cadmpeg_core::decode::u64_from_index(invalid.len() + 32);
    with_decode_context_over(&[], |policy| {
        policy.limits.max_materialized_bytes = materialized;
        policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(expected.len());
    }, |ctx| {
        let mut candidate_storage = ctx.reserve_scoped(0, "discarded JT mesh candidate").unwrap();
        let refusal = candidate_storage.with_storage(|| {
            let discarded = ctx.collection_vec::<u8>(32, "discarded JT mesh lane").unwrap();
            let refusal = retain_jt_tessellation_id(ctx, 7, 2, Some("bad path")).unwrap().unwrap_err();
            drop(discarded);
            Ok::<_, cadmpeg_core::CodecError>(refusal)
        }).unwrap();
        drop(candidate_storage);
        let message = ctx.format_retained(format_args!("display-jt tessellation: {}", refusal.error), "retain DisplayJT tessellation rejection").unwrap();
        assert_eq!(message, expected);
        drop(refusal);
        let storage = ctx.reserve_scoped(materialized, "reuse JT identity refusal storage").unwrap();
        drop(storage);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn jt_tri_strip_headers_release_a_valid_prefix_and_skip_an_invalid_suffix() {
    const TRI_STRIP: [u8; 16] = [0xab, 0x10, 0xdd, 0x10, 0xc8, 0x2a, 0xd1, 0x11, 0x9b, 0x6b, 0x00, 0x80, 0xc7, 0xbb, 0x59, 0x97];
    let mut body = 1_u16.to_le_bytes().to_vec();
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&0_u64.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    body.extend_from_slice(&7_u32.to_le_bytes());
    body.extend_from_slice(&1_u16.to_le_bytes());
    let mut bytes = vec![0; 25];
    bytes.extend_from_slice(&body);
    let container = container(bytes);
    let valid = DisplayJtShapeLodElement {
        id: "shape-element".into(), segment: "shape-segment".into(), ordinal: 0,
        object_type_id: TRI_STRIP, object_id: 7,
        body_byte_len: u32::try_from(body.len()).unwrap(),
        body_sha256: Sha256Digest::digest(&body), source_offset: 0,
    };
    let invalid = DisplayJtShapeLodElement { source_offset: u64::MAX, ..valid.clone() };
    with_decode_context_over(container.data.as_ref(), |policy| {
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = CANDIDATE_STORAGE_LIMIT;
    }, |ctx| {
        assert!(display_jt_tri_strip_lod_headers(ctx, &container, &[valid, invalid.clone()]).unwrap().is_empty());
        let storage = ctx.reserve_scoped(CANDIDATE_STORAGE_LIMIT, "reuse rejected tri-strip header storage").unwrap();
        drop(storage);
        assert!(ctx.resource_refusal().is_none());
    });
    let elements = vec![invalid; 4097];
    with_decode_context_over(container.data.as_ref(), |policy| policy.limits.max_work_units = 1, |ctx| {
        assert!(display_jt_tri_strip_lod_headers(ctx, &container, &elements).unwrap().is_empty());
        assert!(ctx.resource_refusal().is_none());
    });
}
