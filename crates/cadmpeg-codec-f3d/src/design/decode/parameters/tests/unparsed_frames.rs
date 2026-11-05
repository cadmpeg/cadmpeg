// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use super::typed_parameter_record;
use crate::design::test_support::parameter_record;
use crate::test_support::manifest_test::write_synthetic_manifests;

#[test]
fn decode_reports_typed_parameter_frames_that_match_no_grammar() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = typed_parameter_record(537, Some(536), "AlongDistance");
    bulk[4..7].copy_from_slice(b"256");
    let mut rejected = typed_parameter_record(541, Some(540), "AlongDistance");
    rejected[4..7].copy_from_slice(b"256");
    rejected[81 + 4 + 2 * "3 mm".len()] = 7;
    let rejected_at = bulk.len();
    bulk.extend_from_slice(&rejected);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            "7A6A3D31-BE74-4E19-9115-20642944C2E4",
            "",
            1,
            "Fusion",
            &[537, 541],
        )],
        &[(537, 0), (541, u64::try_from(rejected_at).unwrap())],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = super::decode_archive(&archive).unwrap();
    let [parameter] = decoded.parameters.as_slice() else {
        panic!("expected the one well-formed typed parameter");
    };
    assert_eq!(parameter.record_index, 537);
    let [loss] = decoded.losses.as_slice() else {
        panic!("expected one unreadable parameter part");
    };
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert!(loss.message.contains("541"));
    let provenance = loss.provenance.as_ref().expect("source location");
    assert_eq!(provenance.stream_name(), Some(stream));
    assert_eq!(provenance.offset, u64::try_from(rejected_at).unwrap());
    assert_eq!(
        provenance.byte_len(),
        Some(u64::try_from(rejected.len()).unwrap())
    );
    assert_eq!(decoded.unknowns, 1);
}

#[test]
fn owner_of_given_up_parameter_is_named_in_the_parameter_loss() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut parameter = typed_parameter_record(45, Some(44), "AlongDistance");
    parameter[4..7].copy_from_slice(b"256");
    parameter[81 + 4 + 2 * "3 mm".len()] = 7;
    let mut owner = crate::design::test_support::parameter_owner_frame();
    owner[4..7].copy_from_slice(b"257");
    let owner_at = parameter.len();
    parameter.extend_from_slice(&owner);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&parameter).unwrap();
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "7A6A3D31-BE74-4E19-9115-20642944C2E4",
                "",
                1,
                "Fusion",
                &[45],
            ),
            (
                "D91D429C-1BB5-4640-B49F-69AF14F463E9",
                "",
                1,
                "Fusion",
                &[44],
            ),
        ],
        &[(45, 0), (44, u64::try_from(owner_at).unwrap())],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&archive, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut output =
        cadmpeg_ir::source_fidelity::parts::PartOutput::new(cadmpeg_ir::CodecFormat::F3d, &ctx);
    let mut parameters =
        super::super::decode_parameters(&ctx, &scan, &mut output, &mut Default::default()).unwrap();
    let owners =
        super::super::decode_parameter_owners(&ctx, &scan, &mut parameters, &mut output).unwrap();
    assert!(parameters.is_empty());
    assert!(owners.is_empty());
    let (_, native, _, losses) = output.into_parts();
    assert_eq!(
        native.namespace("f3d").unwrap().arenas()["unknowns"].len(),
        1
    );
    assert_eq!(losses.len(), 1);
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("design-parameter-owner#")));
}

#[test]
fn decode_reports_untyped_parameter_frames_that_match_no_grammar() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = parameter_record(
        Some(44),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d12",
        3.0,
    );
    bulk[4..7].copy_from_slice(b"256");
    let mut rejected = parameter_record(
        Some(48),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d13",
        3.0,
    );
    rejected[4..7].copy_from_slice(b"256");
    rejected[7..11].copy_from_slice(&49u32.to_le_bytes());
    rejected[22..30].copy_from_slice(&9u64.to_le_bytes());
    let rejected_at = bulk.len();
    bulk.extend_from_slice(&rejected);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "00000000-0000-0000-0000-000000000001",
                "",
                1,
                "Fusion",
                &[71, 49],
            ),
            (
                "B0556F58-EE09-46A7-B18F-E296F48E5B13",
                "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
                3,
                "CommonData",
                &[19],
            ),
        ],
        &[(71, 0), (49, u64::try_from(rejected_at).unwrap())],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = super::decode_archive(&archive).unwrap();
    assert_eq!(decoded.parameters.len(), 1);
    let [loss] = decoded.losses.as_slice() else {
        panic!("expected one unreadable parameter part");
    };
    assert_eq!(loss.code.local_code(), "record.unreadable");
    assert!(loss.message.contains("entity "));
    assert!(loss.provenance.is_some());
}

/// Bulk bytes holding an unmatched indexed header before one valid parameter.
fn bulk_with_leading_unmatched_frame() -> (Vec<u8>, usize) {
    let mut bulk = Vec::new();
    bulk.extend_from_slice(&3u32.to_le_bytes());
    bulk.extend_from_slice(b"999");
    bulk.extend_from_slice(&4242u32.to_le_bytes());
    bulk.extend_from_slice(&[7; 11]);
    let valid_at = bulk.len();
    bulk.extend_from_slice(&parameter_record(
        Some(48),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d13",
        3.0,
    ));
    (bulk, valid_at)
}

#[test]
fn unmatched_frame_before_a_parameter_advances_the_scan() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let (mut bulk, valid_at) = bulk_with_leading_unmatched_frame();
    bulk[4..7].copy_from_slice(b"256");
    bulk[valid_at + 4..valid_at + 7].copy_from_slice(b"257");
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[
            (
                "00000000-0000-0000-0000-000000000001",
                "",
                1,
                "Fusion",
                &[4242],
            ),
            (
                "7A6A3D31-BE74-4E19-9115-20642944C2E4",
                "",
                1,
                "Fusion",
                &[71],
            ),
        ],
        &[(4242, 0), (71, u64::try_from(valid_at).unwrap())],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = super::decode_archive(&archive).unwrap();
    let [parameter] = decoded.parameters.as_slice() else {
        panic!("expected the parameter after the unmatched frame");
    };
    assert_eq!(parameter.record_index, 71);
    assert_eq!(
        parameter.byte_offset(),
        Some(u64::try_from(valid_at).unwrap())
    );
    assert_eq!(parameter.expression(), "Width / 2");
    assert!(decoded.losses.is_empty());
}

#[test]
fn parameter_primary_index_bounds_each_record() {
    let (bulk, valid_at) = bulk_with_leading_unmatched_frame();
    let meta = crate::metastream::MetaStream {
        types: Vec::new(),
        records: vec![
            crate::metastream::RecordIndexEntry {
                entity_id: 4242,
                bulk_offset: 0,
            },
            crate::metastream::RecordIndexEntry {
                entity_id: 71,
                bulk_offset: u64::try_from(valid_at).unwrap(),
            },
        ],
        secondary_records: Vec::new(),
    };
    let frames = crate::metastream::primary_record_frames(&meta, bulk.len()).unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!((frames[0].start, frames[0].end), (0, valid_at));
    assert_eq!((frames[1].start, frames[1].end), (valid_at, bulk.len()));
}
