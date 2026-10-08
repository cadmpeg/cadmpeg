// SPDX-License-Identifier: Apache-2.0

use super::super::*;
use crate::container::{Container, DirEntry, DirEntryBody, Region};
use crate::test_support::{resource_refusal_at, with_decode_context, with_decode_context_over};
use cadmpeg_core::decode::{ResourceDimension, View};

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
    with_decode_context_over(&[], |policy| policy.limits.max_work_units = 1, |ctx| {
        let mut view = View::over_retained(&bytes);
        assert!(parse_jt_f32_vector(ctx, &mut view).unwrap().is_none());
        assert_eq!(view.position(), 8);
        assert!(ctx.resource_refusal().is_none());
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
    with_decode_context_over(&[], |policy| policy.limits.max_work_units = limit.used + 1, |ctx| {
        assert!(parse_jt9_partition_node_body(ctx, &bytes).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
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
    with_decode_context_over(container.data.as_ref(), |policy| policy.limits.max_work_units = limit.used + 1, |ctx| {
        assert!(display_jt_indices(ctx, &container).unwrap().is_empty());
        assert!(ctx.resource_refusal().is_none());
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
        container.data.as_ref(), ResourceDimension::RetainedBytes, "admit DisplayJT document",
        |ctx| display_jt_documents(ctx, &container, &indices),
    ) else { panic!("enclosing document storage must refuse"); };
    assert_eq!(limit.additional, cadmpeg_core::decode::u64_from_index(4 * std::mem::size_of::<DisplayJtDocument>()));
    with_decode_context(|ctx| {
        let original = ctx.refuse_codec_limit("prior JT failure", 0, 1);
        let refused = display_jt_documents(ctx, &container, &indices).unwrap_err();
        assert!(matches!((original, refused), (CodecError::ResourceLimit(first), CodecError::ResourceLimit(second)) if first == second));
    });
}
