// SPDX-License-Identifier: Apache-2.0
//! Container parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use cadmpeg_core::container::ContainerRole;

use super::{
    identify_variant, outer_container_declarations, outer_container_for_extent,
    parse_directory_region, parse_extents, reconstruct_logical_stream, scan_bytes, summarize,
    Census, ContainerScan, Descriptor, Extent, InnerDir, EDGE_DELIMITER, OUTER_MAGIC,
};
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, Confidence};

use crate::test_support::test_container::{
    external_reference_segment, finjpl_stream, outer_body_catpart, outer_directory_catpart,
    standard_catpart, summary_preview_segment,
};
use crate::test_support::test_e5::append_e5_record;
use crate::variant::Variant;
use crate::CatiaCodec;

fn summarize_service(scan: &ContainerScan<'_>) -> cadmpeg_ir::ContainerSummary {
    crate::test_support::with_service_context(|ctx| summarize(ctx, scan))
        .expect("service budget admits container summary")
}

fn finjpl_service(body: &super::BodyExtent<'_>) -> Vec<super::FinjplSegment> {
    crate::test_support::with_service_context(|ctx| super::finjpl_segments(ctx, body))
        .expect("service budget admits FINJPL segments")
}

fn preview_service(data: &[u8]) -> Vec<super::PreviewImage> {
    crate::test_support::with_service_context(|ctx| super::preview_images(ctx, data))
        .expect("service budget admits previews")
}

fn external_refs_service(data: &[u8]) -> Vec<super::ExternalReference> {
    crate::test_support::with_service_context(|ctx| super::external_references(ctx, data))
        .expect("service budget admits external references")
}

fn last_save_version_service(data: &[u8]) -> Option<super::LastSaveVersion> {
    crate::test_support::with_service_context(|ctx| super::last_save_version(ctx, data))
        .expect("service budget admits version")
}

fn parse_extents_service(
    dirbuf: &[u8],
    o: usize,
    k: usize,
    physical_base: usize,
    file_len: usize,
) -> Option<(Vec<super::Extent>, usize)> {
    crate::test_support::with_service_context(|ctx| {
        parse_extents(ctx, dirbuf, o, k, physical_base, file_len)
    })
    .expect("service budget admits extents")
}

fn parse_directory_region_service(
    data: &[u8],
    physical_base: usize,
    dir_offset: usize,
    dir_length: usize,
) -> Option<super::InnerDir> {
    crate::test_support::with_service_context(|ctx| {
        parse_directory_region(ctx, data, physical_base, dir_offset, dir_length)
    })
    .expect("service budget admits directory")
}

fn descriptor_name_service(dirbuf: &[u8], ds: usize) -> String {
    crate::test_support::with_service_context(|ctx| super::descriptor_name(ctx, dirbuf, ds))
        .expect("service budget admits descriptor name")
}

fn reconstruct_service(data: &[u8], descriptor: &Descriptor, inner: usize) -> Vec<u8> {
    crate::test_support::with_service_context(|ctx| {
        let (stream, storage) = reconstruct_logical_stream(ctx, data, descriptor, inner)?;
        storage.commit()?;
        Ok::<_, cadmpeg_core::CodecError>(stream)
    })
    .expect("service budget admits logical stream")
}

fn brep_service(data: &[u8], dir: &InnerDir) -> Option<Vec<u8>> {
    crate::test_support::with_service_context(|ctx| super::brep_stream(ctx, data, dir))
        .expect("service budget admits BREP stream")
}

fn main_data_stream_service(data: &[u8], dir: &InnerDir) -> Option<Vec<u8>> {
    crate::test_support::with_service_context(|ctx| super::main_data_stream(ctx, data, dir))
        .expect("service budget admits main stream")
}

fn outer_declarations_service(
    data: &[u8],
    dir: &InnerDir,
) -> Vec<super::OuterContainerDeclaration> {
    crate::test_support::with_service_context(|ctx| outer_container_declarations(ctx, data, dir))
        .expect("service budget admits container declarations")
}

fn record_sources_service(
    scan: &ContainerScan<'_>,
) -> Vec<Vec<crate::wire::records::SourceExtent>> {
    crate::test_support::with_service_context(|ctx| super::consolidated_record_sources(ctx, scan))
        .expect("service budget admits record sources")
}

fn record_ranges_service(scan: &ContainerScan<'_>) -> Vec<std::ops::Range<usize>> {
    crate::test_support::with_service_context(|ctx| super::consolidated_record_ranges(ctx, scan))
        .expect("service budget admits record ranges")
}

#[test]
fn logical_stream_bytes_refuse_retained_limit() {
    let descriptor = test_descriptor("MainDataStream", 1, 3);
    let limited = crate::test_support::with_retained_limit(2, |ctx| {
        let (stream, storage) = reconstruct_logical_stream(ctx, b"01234", &descriptor, 0)?;
        storage.commit()?;
        Ok::<_, cadmpeg_core::CodecError>(stream)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_logical_stream_bytes")
    );
    assert_eq!(reconstruct_service(b"01234", &descriptor, 0), b"123");
}

#[test]
fn logical_stream_roster_refuses_collection_limit() {
    let bytes = outer_directory_catpart();
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, bytes))
        .expect("service budget admits outer directory");
    let first_len = scan.outer.as_ref().expect("outer directory").descriptors[0].logical_length();
    let limited = crate::test_support::with_collection_limit(first_len, |ctx| {
        super::logical_record_streams(ctx, &scan)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_logical_record_streams")
    );
    let streams =
        crate::test_support::with_service_context(|ctx| super::logical_record_streams(ctx, &scan))
            .expect("service budget admits logical streams");
    assert_eq!(streams.len(), 1);
}

#[test]
fn record_source_inner_extents_refuse_collection_limit() {
    let bytes = outer_directory_catpart();
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, bytes))
        .expect("service budget admits outer directory");
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::consolidated_record_sources(ctx, &scan)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_record_source_extents")
    );
    assert_eq!(record_sources_service(&scan).len(), 1);
}

#[test]
fn fbb_run_roster_refuses_collection_limit() {
    let bytes = [0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0];
    let limited =
        crate::test_support::with_collection_limit(0, |ctx| super::fbb_run_ranges(ctx, &bytes));
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_fbb_run_ranges")
    );
    let ranges =
        crate::test_support::with_service_context(|ctx| super::fbb_run_ranges(ctx, &bytes))
            .expect("service budget admits FBB runs");
    assert_eq!(ranges, vec![0..8]);
}

#[test]
fn outer_container_stream_identity_refuses_retained_limit() {
    let (bytes, _) = crate::test_support::test_container::outer_container_catpart(b"graph");
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, bytes))
        .expect("service budget admits outer container");
    let outer = scan.outer.as_ref().expect("outer directory");
    let data_descriptor = outer
        .descriptors
        .iter()
        .find(|descriptor| descriptor.name == "Data")
        .expect("Data descriptor");
    let logical = reconstruct_service(&scan.data, data_descriptor, outer.inner);
    let limited = crate::test_support::with_retained_limit(0, |ctx| {
        super::parse_outer_container_declarations(ctx, &logical, &outer.descriptors)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_container_stream_name")
    );
    assert_eq!(outer_declarations_service(&scan.data, outer).len(), 1);
}

#[test]
fn finjpl_markers_refuse_collection_limit() {
    let bytes = summary_preview_segment();
    let body = super::BodyExtent::whole(&bytes);
    let limited =
        crate::test_support::with_collection_limit(0, |ctx| super::finjpl_segments(ctx, &body));
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_finjpl_positions")
    );
    assert_eq!(finjpl_service(&body).len(), 1);
}

#[test]
fn finjpl_primary_name_refuses_retained_limit() {
    let bytes = summary_preview_segment();
    let limited = crate::test_support::with_retained_limit(0, |ctx| {
        super::finjpl_primary_name(ctx, &bytes, 0, bytes.len())
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_finjpl_name")
    );
    assert_eq!(
        finjpl_service(&super::BodyExtent::whole(&bytes))[0]
            .name
            .as_deref(),
        Some("CATSummaryInformation")
    );
}

#[test]
fn preview_rows_refuse_collection_limit() {
    let bytes = summary_preview_segment();
    let segments = finjpl_service(&super::BodyExtent::whole(&bytes));
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        super::preview_images_in_segments(ctx, &bytes, &segments)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_preview_images")
    );
    assert_eq!(preview_service(&bytes).len(), 1);
}

#[test]
fn external_reference_target_refuses_retained_limit() {
    let bytes = external_reference_segment("linked.CATPart");
    let segments = finjpl_service(&super::BodyExtent::whole(&bytes));
    let limited = crate::test_support::with_retained_limit(0, |ctx| {
        super::external_references_in_segments(ctx, &bytes, &segments)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_external_reference_target")
    );
    assert_eq!(external_refs_service(&bytes)[0].target, "linked.CATPart");
}

#[test]
fn last_save_build_date_refuses_retained_limit() {
    let bytes = summary_preview_segment();
    let limited = crate::test_support::with_retained_limit(0, |ctx| {
        super::parse_last_save_version(ctx, &bytes)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_last_save_build_date")
    );
    assert_eq!(
        last_save_version_service(&bytes).expect("version").version,
        5
    );
}

#[test]
fn summary_attribute_refuses_collection_limit() {
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, standard_catpart()))
        .expect("service resource budget");
    let limited = crate::test_support::with_collection_limit(0, |ctx| summarize(ctx, &scan));
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_summary_attribute")
    );
    assert!(!summarize_service(&scan).entries.is_empty());
}

#[test]
fn container_note_refuses_retained_limit() {
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, standard_catpart()))
        .expect("service resource budget");
    let limited = crate::test_support::with_retained_limit(0, |ctx| super::notes(ctx, &scan));
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_container_note")
    );
    let notes = crate::test_support::with_service_context(|ctx| super::notes(ctx, &scan))
        .expect("service budget admits container notes");
    assert!(!notes.is_empty());
}

#[test]
fn container_notes_refuse_collection_limit() {
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, standard_catpart()))
        .expect("service resource budget");
    let limited = crate::test_support::with_collection_limit(0, |ctx| super::notes(ctx, &scan));
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_container_notes")
    );
    let notes = crate::test_support::with_service_context(|ctx| super::notes(ctx, &scan))
        .expect("service budget admits container notes");
    assert!(!notes.is_empty());
}

fn append_e5_test_record(bytes: &mut Vec<u8>, id: u32) {
    append_e5_test_record_with_payload(bytes, id, &[]);
}

fn append_e5_test_record_with_payload(bytes: &mut Vec<u8>, id: u32, payload: &[u8]) {
    bytes.extend_from_slice(super::E5_MARKER);
    bytes.extend_from_slice(&[0xfe, 0x00]);
    bytes.extend_from_slice(
        &(u16::try_from(payload.len()).expect("fixture value fits u16")).to_le_bytes(),
    );
    bytes.extend_from_slice(&[0x00, 0x00]);
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(payload);
}

fn outer_with_preamble(body: &[u8]) -> Vec<u8> {
    let directory_length = 32usize;
    let directory_offset = 512usize;
    let mut bytes = vec![0u8; directory_length];
    bytes[..super::OUTER_MAGIC.len()].copy_from_slice(super::OUTER_MAGIC);
    bytes[8..12].copy_from_slice(
        &(u32::try_from(directory_offset).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes[12..16].copy_from_slice(
        &(u32::try_from(directory_length).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes.extend_from_slice(body);
    bytes.resize(directory_offset + directory_length, 0);
    bytes
}

fn test_descriptor(name: &str, physical_offset: u32, length: u32) -> Descriptor {
    Descriptor {
        name: name.to_string(),
        desc_offset: 0,
        extents: vec![Extent {
            phys_off: physical_offset,
            phys_len: length,
            flags: 0,
        }],
    }
}

fn fbb_only_tables_with_shared_delimiter() -> Vec<u8> {
    let mut bytes = vec![0x30, 0x04, 0x04, 0xff, 0xd2, 0xd2, 0xd2, 0xd2];
    for (kind, handles) in [(1u8, [1u8, 2]), (2, [2, 3])] {
        bytes.extend_from_slice(&[0x01, kind, 0x01, 0x02, 0x02]);
        bytes.extend_from_slice(&handles);
        bytes.extend_from_slice(super::EDGE_DELIMITER.as_slice());
    }
    bytes.extend_from_slice(&[0x01, 0x06, 0x00]);
    bytes
}

#[test]
fn nested_fbb_spine_precedes_a_coherent_e5_stream() {
    let scan = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, standard_catpart()))
        .expect("service resource budget");
    assert_eq!(
        crate::test_support::with_service_context(|ctx| identify_variant(
            ctx,
            scan.inner.as_ref(),
            scan.brep.as_deref(),
            scan.main_data_stream.as_deref(),
            &scan.census,
            true,
        ))
        .expect("service resource budget"),
        Variant::StandardNested
    );
}

#[test]
fn coherent_e5_stream_overrides_zero_entity_markers() {
    let census = Census {
        a9_records: 1,
        ..Census::default()
    };
    assert_eq!(
        crate::test_support::with_service_context(|ctx| identify_variant(
            ctx, None, None, None, &census, true
        ))
        .expect("service resource budget"),
        Variant::E5Stream
    );
}

#[test]
fn scan_selects_a_coherent_e5_walk_over_a_zero_entity_record() {
    let mut body = vec![0xa9, 0x03, 0x10, 0x00, 0, 0, 0, 0, 0, 0, 0, 0];
    for id in 0..10 {
        append_e5_test_record(&mut body, id);
    }
    let scan = crate::test_support::with_service_context(|ctx| {
        scan_bytes(ctx, outer_with_preamble(&body))
    })
    .expect("service resource budget");
    assert_eq!(scan.census.a9_records, 1);
    assert_eq!(scan.variant, Variant::E5Stream);
}

#[test]
fn coherent_e5_stream_overrides_an_inner_body_without_brep_streams() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    assert_eq!(
        crate::test_support::with_service_context(|ctx| identify_variant(
            ctx,
            Some(&inner),
            None,
            None,
            &Census::default(),
            true
        ))
        .expect("service resource budget"),
        Variant::E5Stream
    );
}

#[test]
fn fbb_only_grammar_wins_when_its_delimiter_is_shared_with_standard() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    let brep = fbb_only_tables_with_shared_delimiter();
    let census = Census {
        fbb_runs: 1,
        edge_delimiters: 2,
        ..Census::default()
    };

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::standard::fbb::standard_edge_count(ctx, &brep)
        })
        .expect("service resource budget"),
        None
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::standard::fbb::fbb_only_edge_count(ctx, &brep)
        })
        .expect("service resource budget"),
        Some(2)
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| identify_variant(
            ctx,
            Some(&inner),
            Some(&brep),
            Some(&brep),
            &census,
            false
        ))
        .expect("service resource budget"),
        Variant::FbbOnly
    );
}

#[test]
fn unadmitted_fbb_region_is_unknown_even_with_delimiter_markers() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    let mut brep = vec![0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0];
    brep.extend_from_slice(EDGE_DELIMITER.as_slice());
    let census = Census {
        fbb_runs: 1,
        edge_delimiters: 1,
        ..Census::default()
    };

    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::standard::fbb::standard_edge_count(ctx, &brep)
        })
        .expect("service resource budget"),
        None
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::families::standard::fbb::fbb_only_edge_count(ctx, &brep)
        })
        .expect("service resource budget"),
        None
    );
    assert_eq!(
        crate::test_support::with_service_context(|ctx| identify_variant(
            ctx,
            Some(&inner),
            Some(&brep),
            Some(&brep),
            &census,
            false
        ))
        .expect("service resource budget"),
        Variant::Unknown
    );

    let no_vertex_brep = vec![0x30, 0x04, 0x04, 0xff, 0, 0, 0, 0];
    let no_vertex_census = Census {
        fbb_runs: 1,
        ..Census::default()
    };
    assert_eq!(
        crate::test_support::with_service_context(|ctx| identify_variant(
            ctx,
            Some(&inner),
            Some(&no_vertex_brep),
            Some(&no_vertex_brep),
            &no_vertex_census,
            false,
        ))
        .expect("service resource budget"),
        Variant::Unknown
    );
}

#[test]
fn coherent_e5_stream_precedes_a_partial_fbb_spine() {
    let inner = InnerDir {
        inner: 0,
        descriptors: Vec::new(),
    };
    let brep = fbb_only_tables_with_shared_delimiter();
    let census = Census {
        fbb_runs: 1,
        edge_delimiters: 2,
        ..Census::default()
    };

    assert_eq!(
        crate::test_support::with_service_context(|ctx| identify_variant(
            ctx,
            Some(&inner),
            Some(&brep),
            Some(&brep),
            &census,
            true
        ))
        .expect("service resource budget"),
        Variant::E5Stream
    );
}

#[test]
fn e5_stream_requires_declared_stride_or_coordinate_rows_between_records() {
    let mut body = Vec::new();
    for id in 0..10 {
        append_e5_test_record(&mut body, id);
        body.push(0x7f);
    }
    assert!(
        crate::test_support::with_service_context(|ctx| super::e5_record_stream(
            ctx,
            &outer_with_preamble(&body)
        ))
        .expect("service work")
        .is_none()
    );

    let mut body = Vec::new();
    for id in 0..10 {
        append_e5_test_record(&mut body, id);
        if id != 9 {
            body.extend_from_slice(&[0x05, 0x08, 0x01]);
            body.extend_from_slice(&[0; 12]);
        }
    }
    assert!(
        crate::test_support::with_service_context(|ctx| super::e5_record_stream(
            ctx,
            &outer_with_preamble(&body)
        ))
        .expect("service work")
        .is_some()
    );
}

#[test]
fn e5_stream_ignores_markers_inside_framed_payloads() {
    let mut false_record = Vec::new();
    append_e5_test_record(&mut false_record, 100);
    let mut body = Vec::new();
    append_e5_test_record_with_payload(&mut body, 0, &false_record);
    for id in 1..9 {
        append_e5_test_record(&mut body, id);
    }
    assert!(
        crate::test_support::with_service_context(|ctx| super::e5_record_stream(
            ctx,
            &outer_with_preamble(&body)
        ))
        .expect("service work")
        .is_none()
    );
}

#[test]
fn e5_stream_and_finjpl_inventory_exclude_the_trailing_directory() {
    let directory_length = 192usize;
    let directory_offset = 512usize;
    let mut bytes = vec![0u8; directory_length];
    bytes[..super::OUTER_MAGIC.len()].copy_from_slice(super::OUTER_MAGIC);
    bytes[8..12].copy_from_slice(
        &(u32::try_from(directory_offset).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes[12..16].copy_from_slice(
        &(u32::try_from(directory_length).expect("fixture value fits u32")).to_be_bytes(),
    );
    bytes.resize(directory_offset, 0);

    let mut directory = vec![0u8; super::DIR_MAGIC.len()];
    directory[..super::DIR_MAGIC.len()].copy_from_slice(super::DIR_MAGIC);
    directory.extend_from_slice(super::FINJPL_MARKER);
    directory.extend_from_slice(&0x0000_008eu32.to_be_bytes());
    for id in 0..10 {
        append_e5_test_record(&mut directory, id);
    }
    directory.resize(directory_length, 0);
    bytes.extend_from_slice(&directory);

    assert!(
        crate::test_support::with_service_context(|ctx| super::e5_record_stream(ctx, &bytes))
            .expect("service work")
            .is_none()
    );
    let scan = crate::test_support::with_service_context(|ctx| super::scan_bytes(ctx, bytes))
        .expect("service resource budget");
    assert!(scan.finjpl_segments.is_empty());
    assert_eq!(scan.census.e5_markers, 0);
}

#[test]
fn all_e5_record_spans_cross_other_framed_records() {
    let mut body = Vec::new();
    append_e5_test_record(&mut body, 1);
    body.extend_from_slice(&[0xe5, 0x0d, 0x13, 0xf4, 0x01, 0x09, 0, 0, 0]);
    append_e5_test_record(&mut body, 2);
    assert_eq!(super::all_e5_record_spans(&body).count(), 2);
}

#[test]
fn equal_unpreferred_e5_segment_walks_are_ambiguous() {
    let mut body = Vec::new();
    for segment in 0..2 {
        body.extend_from_slice(super::FINJPL_MARKER);
        body.extend_from_slice(&0x0000_0080u32.to_be_bytes());
        for id in 0..10 {
            append_e5_test_record(&mut body, segment * 10 + id);
        }
    }
    assert!(
        crate::test_support::with_service_context(|ctx| super::e5_record_stream(
            ctx,
            &outer_with_preamble(&body)
        ))
        .expect("service work")
        .is_none()
    );
}

#[test]
fn brep_stream_requires_unique_canonical_descriptors() {
    let data = (0..32u8).collect::<Vec<_>>();
    let tied = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("MainDataStream", 0, 4),
            test_descriptor("MainDataStream", 4, 4),
            test_descriptor("SurfacicReps", 8, 2),
        ],
    };
    assert!(brep_service(&data, &tied).is_none());

    let noncanonical = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("MainDataStream", 0, 4),
            test_descriptor("SurfacicRepsAlias", 4, 4),
        ],
    };
    assert!(brep_service(&data, &noncanonical).is_none());

    let unique = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("MainDataStream", 0, 4),
            test_descriptor("MainDataStream", 4, 5),
            test_descriptor("SurfacicReps", 9, 2),
        ],
    };
    assert_eq!(
        brep_service(&data, &unique),
        Some(data[4..9].iter().chain(&data[9..11]).copied().collect())
    );
    assert_eq!(
        main_data_stream_service(&data, &unique),
        Some(data[4..9].to_vec())
    );
}

#[test]
fn extent_parser_retains_the_raw_flags_word() {
    let mut directory = vec![0; 24];
    for (offset, value) in [(4, 40u32), (8, 8), (12, 8), (16, 0), (20, 0xa501_0080)] {
        directory[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    let (extents, logical_length) =
        parse_extents_service(&directory, 0, 1, 0, 64).expect("complete extent");
    assert_eq!(logical_length, 8);
    assert_eq!(extents[0].flags, 0xa501_0080);
    assert!(parse_extents_service(&directory, 0, 1, usize::MAX, usize::MAX).is_none());
}

#[test]
fn extent_roster_refuses_collection_limit() {
    let mut directory = vec![0; 24];
    for (offset, value) in [(4, 40u32), (8, 8), (12, 8), (16, 0), (20, 0)] {
        directory[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    let limited = crate::test_support::with_collection_limit(0, |ctx| {
        parse_extents(ctx, &directory, 0, 1, 0, 64)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_directory_extents")
    );
    assert_eq!(
        parse_extents_service(&directory, 0, 1, 0, 64)
            .expect("valid extent")
            .0
            .len(),
        1
    );
}

#[test]
fn directory_descriptor_refuses_collection_limit() {
    let bytes = outer_directory_catpart();
    let limited = crate::test_support::with_collection_limit(1, |ctx| {
        super::parse_outer_stream_directory(ctx, &bytes)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_directory_descriptors")
    );
    let parsed = crate::test_support::with_service_context(|ctx| {
        super::parse_outer_stream_directory(ctx, &bytes)
    })
    .expect("service budget admits directory")
    .expect("valid directory");
    assert_eq!(parsed.descriptors.len(), 1);
}

#[test]
fn descriptor_name_is_anchored_to_the_descriptor_tail() {
    let mut directory = vec![0u8; 0x80];
    let ds = 0x40;
    let name = b"MainDataStream";
    let name_start = ds - 3 - name.len() * 2;
    for (index, byte) in name.iter().enumerate() {
        directory[name_start + index * 2] = *byte;
    }
    directory[ds - 3..ds].copy_from_slice(&[0, 0, 0]);

    assert_eq!(descriptor_name_service(&directory, ds), "MainDataStream");
}

#[test]
fn descriptor_name_refuses_retained_limit() {
    let mut directory = vec![0u8; 0x80];
    let ds = 0x40;
    let name = b"MainDataStream";
    let name_start = ds - 3 - name.len() * 2;
    for (index, byte) in name.iter().enumerate() {
        directory[name_start + index * 2] = *byte;
    }
    directory[ds - 3..ds].copy_from_slice(&[0, 0, 0]);
    let limited = crate::test_support::with_retained_limit(0, |ctx| {
        super::descriptor_name(ctx, &directory, ds)
    });
    assert!(
        matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_descriptor_name")
    );
    assert_eq!(descriptor_name_service(&directory, ds), "MainDataStream");
}

#[test]
fn descriptor_name_ignores_unrelated_utf16_runs_and_requires_the_tail() {
    let mut directory = vec![0u8; 0x80];
    for (index, byte) in b"UNRELATED_LONGER_RUN".iter().enumerate() {
        directory[8 + index * 2] = *byte;
    }
    let ds = 0x40;
    let name = b"Data";
    let name_start = ds - 3 - name.len() * 2;
    for (index, byte) in name.iter().enumerate() {
        directory[name_start + index * 2] = *byte;
    }
    directory[ds - 3..ds].copy_from_slice(&[0, 0, 1]);
    assert!(descriptor_name_service(&directory, ds).is_empty());

    directory[ds - 3..ds].copy_from_slice(&[0, 0, 0]);
    assert_eq!(descriptor_name_service(&directory, ds), "Data");
}

#[test]
fn descriptor_name_accepts_the_legacy_fixed_header_form() {
    let mut directory = vec![0u8; 0x80];
    let ds = 0x10;
    let name = b"RootStorage";
    let name_start = ds + 0x10;
    for (index, byte) in name.iter().enumerate() {
        directory[name_start + index * 2] = *byte;
    }
    directory[name_start + name.len() * 2..name_start + name.len() * 2 + 2]
        .copy_from_slice(&[0, 0]);

    assert_eq!(descriptor_name_service(&directory, ds), "RootStorage");
}

#[test]
fn directory_parser_accepts_a_structurally_bounded_extent_roster_above_64() {
    let descriptor_start = 16;
    let extent_count_offset = descriptor_start + 0x50;
    let extent_count = 65usize;
    let directory_end = extent_count_offset + 4 + extent_count * 20;
    let mut directory = vec![0u8; directory_end];
    directory[..super::DIR_MAGIC.len()].copy_from_slice(super::DIR_MAGIC);
    directory[descriptor_start + 0x0c..descriptor_start + 0x10].copy_from_slice(
        &(u32::try_from(extent_count).expect("fixture value fits u32")).to_be_bytes(),
    );
    directory[extent_count_offset..extent_count_offset + 4].copy_from_slice(
        &(u32::try_from(extent_count).expect("fixture value fits u32")).to_be_bytes(),
    );
    for index in 0..extent_count {
        let extent = extent_count_offset + 4 + index * 20;
        directory[extent..extent + 4].copy_from_slice(
            &(u32::try_from(index).expect("fixture value fits u32")).to_be_bytes(),
        );
        directory[extent + 4..extent + 8].copy_from_slice(&1u32.to_be_bytes());
        directory[extent + 8..extent + 12].copy_from_slice(&1u32.to_be_bytes());
        directory[extent + 12..extent + 16].copy_from_slice(
            &(u32::try_from(index).expect("fixture value fits u32")).to_be_bytes(),
        );
    }

    let parsed = parse_directory_region_service(&directory, 0, 0, directory.len())
        .expect("bounded extent roster");
    let descriptor = parsed
        .descriptors
        .iter()
        .find(|descriptor| descriptor.desc_offset == descriptor_start)
        .expect("descriptor at synthesized header");
    assert_eq!(
        descriptor.logical_length(),
        cadmpeg_core::decode::u64_from_index(extent_count)
    );
    assert_eq!(descriptor.extents.len(), extent_count);
}

#[test]
fn logical_stream_reconstruction_is_atomic_over_its_extent_roster() {
    let descriptor = Descriptor {
        name: "MAIN".to_string(),
        desc_offset: 0,
        extents: vec![
            Extent {
                phys_off: 1,
                phys_len: 2,
                flags: 0,
            },
            Extent {
                phys_off: 7,
                phys_len: 2,
                flags: 0,
            },
        ],
    };
    assert_eq!(reconstruct_service(b"0123456789", &descriptor, 0), b"1278");

    let mut outside = descriptor.clone();
    outside.extents[1].phys_off = 9;
    assert!(reconstruct_service(b"0123456789", &outside, 0).is_empty());
}

#[test]
fn logical_stream_reconstruction_rejects_overflowing_physical_offsets() {
    let descriptor = Descriptor {
        name: "MAIN".to_string(),
        desc_offset: 0,
        extents: vec![Extent {
            phys_off: 1,
            phys_len: 1,
            flags: 0,
        }],
    };
    assert!(reconstruct_service(&[0], &descriptor, usize::MAX).is_empty());
}

#[test]
fn container_summary_exposes_extent_flags_in_logical_order() {
    let scan = ContainerScan {
        data: Vec::new().into(),
        outer_dir_offset: 0,
        outer_dir_length: 0,
        outer: Some(InnerDir {
            inner: 0,
            descriptors: vec![Descriptor {
                name: "MAIN".to_string(),
                desc_offset: 16,
                extents: vec![
                    Extent {
                        phys_off: 40,
                        phys_len: 4,
                        flags: 0xa501_0080,
                    },
                    Extent {
                        phys_off: 80,
                        phys_len: 8,
                        flags: 0,
                    },
                ],
            }],
        }),
        inner: None,
        brep: None,
        main_data_stream: None,
        e5_record_range: None,
        previews: Vec::new(),
        last_save_version: None,
        external_references: Vec::new(),
        finjpl_segments: Vec::new(),
        outer_container_declarations: Vec::new(),
        census: Census::default(),
        variant: Variant::Unknown,
    };
    let summary = summarize_service(&scan);
    assert_eq!(
        summary.entries[0].attributes["extent_flags"],
        "0xa5010080,0x00000000"
    );
}

#[test]
fn outer_data_declaration_assigns_class_to_its_uuid_stream() {
    let mut data = vec![0; 40];
    data[8..12].copy_from_slice(b"\x01\x00\x03\x00");
    data[12..16].copy_from_slice(&2u32.to_le_bytes());
    data[16..24].copy_from_slice(b"\x01\x00\x6c\x00\x02\x00\x00\x00");
    data[32..36].copy_from_slice(b"\x02\x00\x81\x20");
    data.extend_from_slice(b"CATPrtCont\0CATProdCont\0\0");
    data.extend_from_slice(b"\x03\x00\xf7\x00\x03\x00\x00\x00");
    data.extend_from_slice(&0x4bbc_295cu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1048u32.to_be_bytes());
    data.extend_from_slice(&0x62eb_7b6fu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1825u32.to_be_bytes());
    let data_len = u32::try_from(data.len()).expect("bounded declaration");
    let outer = InnerDir {
        inner: 0,
        descriptors: vec![
            Descriptor {
                name: "Data".to_string(),
                desc_offset: 10,
                extents: vec![Extent {
                    phys_off: 0,
                    phys_len: data_len,
                    flags: 0,
                }],
            },
            Descriptor {
                name: "1048_62eb7b6f_1825".to_string(),
                desc_offset: 20,
                extents: vec![Extent {
                    phys_off: data_len,
                    phys_len: 1,
                    flags: 0,
                }],
            },
        ],
    };
    data.push(0);

    let declarations = outer_declarations_service(&data, &outer);

    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].data_offset, 0);
    assert_eq!(declarations[0].ordinal, 2);
    assert_eq!(declarations[0].class_name, "CATPrtCont");
    assert_eq!(declarations[0].base_class, "CATProdCont");
    assert_eq!(declarations[0].stream_name, "1048_62eb7b6f_1825");
    assert_eq!(
        outer_container_for_extent(&outer, &declarations, u64::from(data_len), 1)
            .map(|declaration| declaration.class_name.as_str()),
        Some("CATPrtCont")
    );
    assert!(
        outer_container_for_extent(&outer, &declarations, u64::from(data_len) - 1, 2).is_none()
    );

    let mut prefixed_outer = outer.clone();
    prefixed_outer.descriptors[1].name = "_1048_62eb7b6f_1825".to_string();
    let prefixed_declarations = outer_declarations_service(&data, &prefixed_outer);
    assert_eq!(prefixed_declarations.len(), 1);
    assert_eq!(prefixed_declarations[0].stream_name, "_1048_62eb7b6f_1825");
    assert_eq!(
        outer_container_for_extent(
            &prefixed_outer,
            &prefixed_declarations,
            u64::from(data_len),
            1
        )
        .map(|declaration| declaration.class_name.as_str()),
        Some("CATPrtCont")
    );

    let mut ambiguous_outer = prefixed_outer;
    ambiguous_outer.descriptors.push(Descriptor {
        name: "1048_62eb7b6f_1825".to_string(),
        desc_offset: 30,
        extents: vec![Extent {
            phys_off: data_len,
            phys_len: 1,
            flags: 0,
        }],
    });
    assert!(outer_declarations_service(&data, &ambiguous_outer).is_empty());

    let scan = ContainerScan {
        data: data.into(),
        outer_dir_offset: 0,
        outer_dir_length: 0,
        outer: Some(outer),
        inner: None,
        brep: None,
        main_data_stream: None,
        e5_record_range: None,
        previews: Vec::new(),
        last_save_version: None,
        external_references: Vec::new(),
        finjpl_segments: Vec::new(),
        outer_container_declarations: declarations,
        census: Census::default(),
        variant: Variant::Unknown,
    };
    let summary = summarize_service(&scan);
    assert_eq!(
        summary.entries[1].attributes["container_class"],
        "CATPrtCont"
    );
    assert_eq!(
        summary.entries[1].attributes["container_base_class"],
        "CATProdCont"
    );
    assert_eq!(summary.entries[1].attributes["container_ordinal"], "2");
    assert_eq!(summary.entries[1].attributes["container_data_offset"], "0");
}

#[test]
fn outer_data_declaration_uses_the_terminal_marker_after_long_class_names() {
    let long_class = "C".repeat(193);
    let mut data = vec![0; 40];
    data[8..12].copy_from_slice(b"\x01\x00\x03\x00");
    data[12..16].copy_from_slice(&2u32.to_le_bytes());
    data[16..24].copy_from_slice(b"\x01\x00\x6c\x00\x02\x00\x00\x00");
    data[32..36].copy_from_slice(b"\x02\x00\x81\x20");
    data.extend_from_slice(long_class.as_bytes());
    data.extend_from_slice(b"\0CATProdCont\0\0");
    data.extend_from_slice(b"\x03\x00\xf7\x00\x03\x00\x00\x00");
    data.extend_from_slice(&0x4bbc_295cu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1048u32.to_be_bytes());
    data.extend_from_slice(&0x62eb_7b6fu32.to_be_bytes());
    data.extend_from_slice(&0x0000_1825u32.to_be_bytes());
    let data_len = u32::try_from(data.len()).expect("bounded declaration");
    let outer = InnerDir {
        inner: 0,
        descriptors: vec![
            Descriptor {
                name: "Data".to_string(),
                desc_offset: 10,
                extents: vec![Extent {
                    phys_off: 0,
                    phys_len: data_len,
                    flags: 0,
                }],
            },
            Descriptor {
                name: "1048_62eb7b6f_1825".to_string(),
                desc_offset: 20,
                extents: vec![Extent {
                    phys_off: data_len,
                    phys_len: 1,
                    flags: 0,
                }],
            },
        ],
    };
    data.push(0);

    let declarations = outer_declarations_service(&data, &outer);

    assert_eq!(declarations.len(), 1);
    assert_eq!(declarations[0].class_name, long_class);
    assert_eq!(declarations[0].base_class, "CATProdCont");
}

#[test]
fn detect_high_on_outer_magic() {
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&CatiaCodec, OUTER_MAGIC),
        Confidence::High
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&CatiaCodec, &standard_catpart()),
        Confidence::High
    );
    assert_eq!(
        cadmpeg_test_support::detection::confidence(&CatiaCodec, b"PK\x03\x04 not catia"),
        Confidence::No
    );
}

#[test]
fn summary_preview_parser_extracts_exact_jpeg_and_dimensions() {
    let bytes = summary_preview_segment();
    let segments = finjpl_service(&crate::container::BodyExtent::whole(&bytes));
    assert_eq!(segments[0].name.as_deref(), Some("CATSummaryInformation"));
    let previews = preview_service(&bytes);
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].width, 640);
    assert_eq!(previews[0].height, 288);
    assert_eq!(previews[0].components, 1);
    assert_eq!(&bytes[previews[0].range.clone()][..2], [0xff, 0xd8]);
    assert_eq!(
        &bytes[previews[0].range.clone()][previews[0].range.len() - 2..],
        [0xff, 0xd9]
    );
    let summary = summarize_service(
        &crate::test_support::with_service_context(|ctx| {
            crate::container::scan_bytes(ctx, outer_body_catpart(&bytes))
        })
        .expect("service resource budget"),
    );
    assert!(summary.entries.iter().any(|entry| {
        entry.role == ContainerRole::FinjplSegment && entry.name == "CATSummaryInformation"
    }));

    let mut truncated = bytes;
    let eoi = truncated
        .windows(2)
        .position(|value| value == [0xff, 0xd9])
        .unwrap();
    truncated.truncate(eoi + 1);
    assert!(preview_service(&truncated).is_empty());
}

#[test]
fn summary_version_parser_requires_one_consistent_tuple() {
    let bytes = summary_preview_segment();
    let version = last_save_version_service(&bytes).unwrap();
    assert_eq!(version.version, 5);
    assert_eq!(version.release, 27);
    assert_eq!(version.service_pack, 2);
    assert_eq!(version.hot_fix, 0);
    assert_eq!(version.build_date, "03-10-2017.22.00");

    let mut conflicting = bytes;
    let mut other = summary_preview_segment();
    let release = other
        .windows(11)
        .position(|value| value == b"<Release>27")
        .unwrap();
    other[release + 9] = b'2';
    other[release + 10] = b'8';
    conflicting.extend_from_slice(&other);
    assert!(last_save_version_service(&conflicting).is_none());

    let mut non_summary = summary_preview_segment();
    non_summary[8..12].copy_from_slice(&0x0101_0002u32.to_be_bytes());
    assert!(last_save_version_service(&non_summary).is_none());
    assert!(preview_service(&non_summary).is_empty());
    let native = crate::native::CatiaNative::decode(&non_summary);
    assert!(native.preview_images.is_empty());
}

#[test]
fn storage_property_parser_enumerates_external_catia_documents() {
    let mut bytes = external_reference_segment("Support.CATPart");
    bytes.extend_from_slice(&external_reference_segment("Assembly.CATProduct"));
    bytes.extend_from_slice(&external_reference_segment("notes.txt"));
    let references = external_refs_service(&bytes);
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].target, "Support.CATPart");
    assert_eq!(references[1].target, "Assembly.CATProduct");

    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, outer_body_catpart(&bytes))
    })
    .expect("service resource budget");
    let summary = summarize_service(&scan);
    assert_eq!(
        summary
            .entries
            .iter()
            .filter(|entry| entry.role == ContainerRole::ExternalReference)
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>(),
        ["Support.CATPart", "Assembly.CATProduct"]
    );

    let native = crate::native::CatiaNative::decode(&bytes);
    assert_eq!(native.external_references.len(), 2);
    assert_eq!(native.external_references[0].target, "Support.CATPart");
    assert_eq!(
        native.external_references[0].segment,
        native.finjpl_segments[0].id
    );
    assert_eq!(
        native.external_references[1].segment,
        native.finjpl_segments[1].id
    );
    for reference in &native.external_references {
        let segment = native
            .finjpl_segments
            .iter()
            .find(|segment| segment.id == reference.segment)
            .expect("external-reference segment");
        assert!(reference.byte_offset >= segment.byte_offset);
        assert!(reference.byte_offset < segment.byte_offset + segment.byte_len);
    }
}

#[test]
fn summary_preview_requires_a_coherent_frame_header() {
    let valid = summary_preview_segment();
    let frame = valid
        .windows(2)
        .position(|bytes| bytes == [0xff, 0xc0])
        .expect("fixture SOF marker");

    let mut zero_height = valid.clone();
    zero_height[frame + 5..frame + 7].copy_from_slice(&0u16.to_be_bytes());
    assert!(preview_service(&zero_height).is_empty());

    let mut inconsistent_components = valid;
    inconsistent_components[frame + 9] = 2;
    assert!(preview_service(&inconsistent_components).is_empty());
    assert!(crate::native::CatiaNative::decode(&inconsistent_components)
        .preview_images
        .is_empty());
}

#[test]
fn summary_preview_requires_one_complete_jpeg_candidate() {
    let valid = summary_preview_segment();
    let image_start = valid
        .windows(3)
        .position(|bytes| bytes == [0xff, 0xd8, 0xff])
        .expect("fixture JPEG SOI");

    let mut malformed_prefix = valid.clone();
    malformed_prefix.splice(image_start..image_start, [0xff, 0xd8, 0xff, 0xd9]);
    let previews = preview_service(&malformed_prefix);
    let [preview] = previews.as_slice() else {
        panic!("one complete preview after malformed SOI")
    };
    assert_eq!(&malformed_prefix[preview.range.clone()][..2], [0xff, 0xd8]);

    let image_end = valid
        .windows(2)
        .enumerate()
        .skip(image_start)
        .find_map(|(at, bytes)| (bytes == [0xff, 0xd9]).then_some(at + 2))
        .expect("fixture JPEG EOI");
    let image = valid[image_start..image_end].to_vec();
    let mut duplicate = valid;
    duplicate.extend(image);
    assert!(preview_service(&duplicate).is_empty());
}

#[test]
fn scan_parses_directory_and_identifies_standard() {
    let f = standard_catpart();
    let scan =
        crate::test_support::with_service_context(|ctx| crate::container::scan_bytes(ctx, f))
            .expect("service resource budget");
    assert_eq!(scan.variant, Variant::StandardNested);
    let dir = scan.inner.expect("inner directory");
    assert!(dir.descriptors.iter().any(|d| d.name == "MainDataStream"));
    assert!(dir.descriptors.iter().any(|d| d.name == "SurfacicReps"));
    let brep = scan.brep.expect("reconstructed brep stream");
    // The BREP stream is MainDataStream followed by SurfacicReps.
    assert!(brep.windows(3).any(|w| w == [0x05, 0x08, 0x01]));
    assert!(brep.windows(3).any(|w| w == [0x00, 0x33, 0x33]));
    assert_eq!(scan.census.fbb_runs, 1);
    assert_eq!(scan.census.fbb_face_rows, 2);
    assert!(scan.census.edge_delimiters >= 1);
    assert_eq!(scan.census.vertex_markers, 3);
}

#[test]
fn scan_parses_outer_directory_with_absolute_extents() {
    let bytes = outer_directory_catpart();
    let directory_offset =
        usize::try_from(u32::from_be_bytes(bytes[8..12].try_into().unwrap())).unwrap();
    assert_eq!(
        crate::test_support::with_service_context(|ctx| {
            crate::container::outer_stream_directory_range(ctx, &bytes)
        })
        .expect("service resource budget"),
        Some(directory_offset..bytes.len())
    );
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, bytes.clone())
    })
    .expect("service resource budget");
    let outer = scan.outer.as_ref().expect("outer directory");
    assert_eq!(outer.inner, 0);
    assert_eq!(outer.descriptors.len(), 1);
    let descriptor = &outer.descriptors[0];
    assert_eq!(descriptor.name, "RootStorage");
    assert_eq!(
        reconstruct_service(&bytes, descriptor, outer.inner),
        b"outer logical stream"
    );

    let summary = summarize_service(&scan);
    let entry = summary
        .entries
        .iter()
        .find(|entry| entry.name == "RootStorage")
        .expect("outer stream summary");
    assert_eq!(entry.attributes["directory"], "outer");
}

#[test]
fn inspect_enumerates_streams_and_names_variant() {
    let f = standard_catpart();
    let mut cur = Cursor::new(f);
    let summary = CatiaCodec
        .inspect(&mut cur, &cadmpeg_core::decode::InspectOptions::default())
        .unwrap();
    assert_eq!(summary.format(), "catia");
    assert_eq!(summary.container_kind, "v5-cfv2");
    assert!(summary.entries.iter().any(|e| e.name == "MainDataStream"));
    assert!(summary.entries.iter().any(|e| e.name == "SurfacicReps"));
    assert!(summary.notes.iter().any(|n| n.contains("standard nested")));
}

#[test]
fn finjpl_parser_splits_segments_and_classifies_type_words() {
    use crate::container::FinjplKind;

    let bytes = finjpl_stream();
    let segments = finjpl_service(&crate::container::BodyExtent::whole(&bytes));
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].kind(), FinjplKind::Storage);
    assert_eq!(segments[0].type_word, 0x0000_008e);
    assert_eq!(segments[0].range, 2..17);
    assert_eq!(segments[1].kind(), FinjplKind::ProjectFlags);
}

#[test]
fn e5_stream_selection_prefers_coherent_storage_segment_over_stray_preamble_marker() {
    let mut bytes = vec![0u8; 32];
    bytes[..8].copy_from_slice(OUTER_MAGIC);
    bytes[8..12].copy_from_slice(&512u32.to_be_bytes());
    bytes[12..16].copy_from_slice(&32u32.to_be_bytes());
    append_e5_record(&mut bytes, 0xfe, 1, &[]);
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_0080u32.to_be_bytes());
    for id in 10..21 {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    bytes.extend_from_slice(b"FINJPL  ");
    bytes.extend_from_slice(&0x0000_008eu32.to_be_bytes());
    let expected_start = bytes.len() - 12;
    for id in 30..41 {
        append_e5_record(&mut bytes, 0xfe, id, &[]);
    }
    bytes.resize(544, 0);

    let range = crate::test_support::with_service_context(|ctx| {
        crate::container::e5_record_stream(ctx, &bytes)
    })
    .expect("service work")
    .expect("coherent E5 stream");
    assert_eq!(range.start, expected_start);
    assert_eq!(&bytes[range.start..range.start + 8], b"FINJPL  ");
}

#[test]
fn consolidated_record_sources_follow_physical_stream_extents() {
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, standard_catpart())
    })
    .expect("service resource budget");
    let inner = scan.inner.as_ref().expect("inner stream directory");
    let expected = inner
        .descriptors
        .iter()
        .flat_map(|descriptor| {
            descriptor.extents.iter().map(|extent| {
                let start = inner.inner + cadmpeg_core::decode::index_from_u32(extent.phys_off);
                start..start + cadmpeg_core::decode::index_from_u32(extent.phys_len)
            })
        })
        .collect::<Vec<_>>();
    let expected_sources = inner
        .descriptors
        .iter()
        .map(|descriptor| {
            descriptor
                .extents
                .iter()
                .map(|extent| {
                    let start = inner.inner + cadmpeg_core::decode::index_from_u32(extent.phys_off);
                    start..start + cadmpeg_core::decode::index_from_u32(extent.phys_len)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(record_ranges_service(&scan), expected);
    assert_eq!(
        record_sources_service(&scan)
            .into_iter()
            .map(|source| source
                .into_iter()
                .map(|extent| extent.range())
                .collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        expected_sources
    );
    assert!(record_ranges_service(&scan)
        .iter()
        .all(|range| !range.contains(&inner.inner)));
}

#[test]
fn flagged_fbb_marker_is_structural() {
    assert!(crate::container::is_fbb_row(&[
        0xb0, 0x04, 0x04, 0xff, 0x99, 0x1f, 0x1a, 0xd1,
    ]));
    assert!(!crate::container::is_fbb_row(&[
        0x20, 0x04, 0x04, 0xff, 0xff, 0xc4, 0xb2, 0xaa,
    ]));
}

#[test]
fn fbb_census_separates_groups_from_face_rows() {
    let row = [0x30, 0x04, 0x04, 0xff, 0, 1, 2, 3];
    let mut body = row.to_vec();
    body.extend_from_slice(&row);
    body.extend_from_slice(&[0xaa; 8]);
    body.extend_from_slice(&row);

    let ranges = crate::test_support::with_service_context(|ctx| {
        crate::container::fbb_run_ranges(ctx, &body)
    })
    .expect("service budget admits FBB runs");
    assert_eq!(ranges, vec![0..16, 24..32]);
    let scan = crate::test_support::with_service_context(|ctx| {
        crate::container::scan_bytes(ctx, standard_catpart())
    })
    .expect("service resource budget");
    assert_eq!(scan.census.fbb_runs, 1);
    assert_eq!(scan.census.fbb_face_rows, 2);
}

#[test]
fn marker_free_container_searches_refuse_work() {
    let bytes = [0_u8; 64];
    for operation in [
        "catia_finjpl_scan",
        "catia_fbb_scan",
        "catia_census_marker_scan",
    ] {
        crate::test_support::with_work_limit(0, |ctx| {
            let error = match operation {
                "catia_finjpl_scan" => {
                    super::finjpl_segments(ctx, &super::BodyExtent::whole(&bytes)).map(|_| ())
                }
                "catia_fbb_scan" => super::fbb_run_ranges(ctx, &bytes).map(|_| ()),
                _ => super::count_subslice(ctx, &bytes, b"marker").map(|_| ()),
            }
            .expect_err("search must consume work");
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("resource refusal required")
            };
            assert_eq!(limit.operation, operation);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }
}

#[test]
fn empty_directory_candidate_scans_refuse_work() {
    let mut bytes = super::DIR_MAGIC.to_vec();
    bytes.resize(128, 0);
    for probe in [false, true] {
        crate::test_support::with_work_limit(0, |ctx| {
            let result = if probe {
                super::directory_region_has_descriptor(ctx, &bytes, 0, 0, bytes.len()).map(|_| ())
            } else {
                super::parse_directory_region(ctx, &bytes, 0, 0, bytes.len()).map(|_| ())
            };
            let cadmpeg_core::CodecError::ResourceLimit(limit) =
                result.expect_err("candidate scan consumes work")
            else {
                panic!("resource refusal required")
            };
            assert_eq!(limit.operation, "catia_directory_candidate_scan");
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }
}

#[test]
fn outer_declarations_release_their_reconstructed_stream() {
    let data = [0_u8; 64];
    let directory = InnerDir {
        inner: 0,
        descriptors: vec![test_descriptor("Data", 0, 64)],
    };
    crate::test_support::with_retained_limit(0, |ctx| {
        assert!(super::outer_container_declarations(ctx, &data, &directory)
            .expect("discarded stream is temporary")
            .is_empty());
    });
    crate::test_support::with_materialized_limit(0, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            super::outer_container_declarations(ctx, &data, &directory)
                .expect_err("temporary stream storage must be admitted")
        else {
            panic!("resource refusal required")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn brep_surface_source_is_scoped_and_destination_is_retained_once() {
    let data = [1_u8, 2, 3, 4, 5, 6];
    let directory = InnerDir {
        inner: 0,
        descriptors: vec![
            test_descriptor("MainDataStream", 0, 4),
            test_descriptor("SurfacicReps", 4, 2),
        ],
    };
    crate::test_support::with_retained_limit(8, |ctx| {
        assert_eq!(
            super::brep_stream(ctx, &data, &directory)
                .expect("eight retained vector capacity bytes"),
            Some(data.to_vec())
        );
    });
    crate::test_support::with_materialized_limit(0, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            super::brep_stream(ctx, &data, &directory).expect_err("stream scratch is admitted")
        else {
            panic!("resource refusal required")
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes
        );
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn jpeg_candidate_suffix_walks_refuse_caller_work_limit() {
    let mut bytes = summary_preview_segment();
    let image_start = bytes
        .windows(3)
        .position(|value| value == [0xff, 0xd8, 0xff])
        .expect("SOI");
    bytes.truncate(image_start);
    for _ in 0..32 {
        bytes.extend([0xff, 0xd8, 0xff, 0xda, 0, 2]);
    }
    let segments = finjpl_service(&super::BodyExtent::whole(&bytes));
    crate::test_support::with_work_limit(u64::try_from(bytes.len() * 2).expect("work"), |ctx| {
        let error = super::preview_images_in_segments(ctx, &bytes, &segments)
            .expect_err("repeated suffix walks exceed work");
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "catia_jpeg_marker_walk");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
    assert!(preview_service(&bytes).is_empty());
}

#[test]
fn declaration_candidates_refuse_repeated_suffix_searches() {
    let mut data = vec![0u8; 1024];
    for start in (0..900).step_by(64) {
        data[start + 8..start + 12].copy_from_slice(&[1, 0, 3, 0]);
        data[start + 16..start + 24].copy_from_slice(&[1, 0, 0x6c, 0, 2, 0, 0, 0]);
        data[start + 32..start + 36].copy_from_slice(&[2, 0, 0x81, 0x20]);
    }
    crate::test_support::with_work_limit(2048, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            super::parse_outer_container_declarations(ctx, &data, &[])
                .expect_err("suffix searches require caller work")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "catia_container_terminal_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn nested_magic_absence_refuses_unadmitted_search() {
    crate::test_support::with_work_limit(0, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            super::parse_stream_directory(ctx, &[0; 256]).expect_err("magic search needs work")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "catia_nested_magic_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn e5_stride_selection_refuses_caller_work() {
    let mut body = Vec::new();
    for id in 0..10 {
        append_e5_test_record(&mut body, id);
    }
    let bytes = outer_with_preamble(&body);
    crate::test_support::with_work_limit(0, |ctx| {
        let cadmpeg_core::CodecError::ResourceLimit(limit) =
            super::e5_record_stream(ctx, &bytes).expect_err("selection needs work")
        else {
            panic!("resource refusal")
        };
        assert_eq!(limit.operation, "catia_e5_segment_scan");
        assert_eq!(ctx.resource_refusal(), Some(limit));
    });
}

#[test]
fn container_scan_rejects_wrong_magic_and_truncated_header() {
    for bytes in [&[][..], &b"garbage!"[..]] {
        let result = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, bytes));
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::WrongFormat(_))
        ));
    }
    for length in 8..16 {
        let mut bytes = super::OUTER_MAGIC.to_vec();
        bytes.resize(length, 0);
        let result = crate::test_support::with_service_context(|ctx| scan_bytes(ctx, bytes));
        assert!(matches!(
            result,
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }
}
