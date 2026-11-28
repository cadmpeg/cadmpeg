// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use super::super::decode_parameters;
use super::compact_null_owner::compact_null_owner_parameter_record;
use super::typed_parameter_record;
use crate::design::test_support::parameter_record;
use crate::test_support::manifest_fixture::write_synthetic_manifests;
use crate::test_support::zip_fixture::with_scan;

#[test]
fn compact_owned_parameter_keeps_offset_with_opaque_trailer_flag() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let fixture = |owner, kind, flag| {
        let expression = "2 mm";
        let mut bytes = super::compact_owned_parameter_record(
            owner,
            2,
            expression,
            kind,
            Some("mm"),
            "d2",
            0.2,
        );
        let trailer = 41 + 4 + 2 * expression.encode_utf16().count();
        bytes[trailer] = flag;
        bytes
    };
    let mut bulk = fixture(31, "Side1Offset", 1);
    bulk.extend_from_slice(&fixture(34, "Side1Offset", 2));
    bulk.extend_from_slice(&fixture(37, "TangencyWeight", 1));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();

    let [offset] = decoded.parameters.as_slice() else {
        panic!("one well-formed offset");
    };
    assert_eq!(offset.owner_record_index(), Some(31));
    assert_eq!(offset.source_kind(), "Side1Offset");
    assert_eq!(offset.expression(), "2 mm");
    assert!(!offset.flags().driven);
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with unparsed compact parameter frames");
    };
    assert_eq!(unparsed.stream, stream);
    assert_eq!(unparsed.frames.len(), 2);
    let [flagged] = decoded.trailer_flags.as_slice() else {
        panic!("one compact trailer flag report");
    };
    assert_eq!(flagged.count, 1);
}

#[test]
fn decode_reports_typed_parameter_frames_that_match_no_grammar() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = typed_parameter_record(537, Some(536), "AlongDistance");
    let mut rejected = typed_parameter_record(541, Some(540), "AlongDistance");
    rejected[81 + 4 + 2 * "3 mm".len()] = 7;
    bulk.extend_from_slice(&rejected);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    let [parameter] = decoded.parameters.as_slice() else {
        panic!("expected the one well-formed typed parameter");
    };
    assert_eq!(parameter.record_index, 537);
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with unparsed typed parameter frames");
    };
    assert_eq!(unparsed.stream, stream);
    assert_eq!(unparsed.frames.len(), 1);
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
    let mut rejected = parameter_record(
        Some(48),
        "Width / 2",
        "AlongDistance",
        Some("mm"),
        "d13",
        3.0,
    );
    rejected[7..11].copy_from_slice(&49u32.to_le_bytes());
    rejected[22..30].copy_from_slice(&9u64.to_le_bytes());
    bulk.extend_from_slice(&rejected);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(
        &[(
            "B0556F58-EE09-46A7-B18F-E296F48E5B13",
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[19],
        )],
        &[],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    assert_eq!(decoded.parameters.len(), 1);
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with unparsed untyped parameter frames");
    };
    assert_eq!(unparsed.stream, stream);
    assert_eq!(unparsed.frames.len(), 1);
}

#[test]
fn decode_reads_compact_null_owner_frames_and_reports_those_that_do_not_close() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = compact_null_owner_parameter_record("d87", [0, 0, 0, 1, 0]);
    let mut rejected = compact_null_owner_parameter_record("d87", [0, 0, 0, 2, 0]);
    rejected[7..11].copy_from_slice(&902u32.to_le_bytes());
    bulk.extend_from_slice(&rejected);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(
        &[(
            "B0556F58-EE09-46A7-B18F-E296F48E5B13",
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[19],
        )],
        &[],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    let [parameter] = decoded.parameters.as_slice() else {
        panic!("expected the one closing null-owner parameter");
    };
    assert_eq!(parameter.record_index, 901);
    assert_eq!(parameter.owner_record_index(), None);
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with an unparsed parameter frame");
    };
    assert_eq!(unparsed.frames.len(), 1);
    let [flagged] = decoded.trailer_flags.as_slice() else {
        panic!("expected one stream with a set trailer flag");
    };
    assert_eq!((flagged.stream.as_str(), flagged.count), (stream, 1));
    let [unstored] = decoded.null_owners.as_slice() else {
        panic!("expected one stream with a null-owner parameter");
    };
    assert_eq!((unstored.stream.as_str(), unstored.count), (stream, 1));
}

#[test]
fn decode_reports_a_compact_null_owner_frame_whose_tail_names_another_type() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let bulk = compact_null_owner_parameter_record("d87", [0, 0, 0, 1, 0]);
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let metadata = crate::test_support::streams_fixture::design_metastream_with_records(
        &[
            (
                "B0556F58-EE09-46A7-B18F-E296F48E5B13",
                "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
                3,
                "CommonData",
                &[20],
            ),
            (
                "00000000-0000-4000-8000-000000000019",
                "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
                3,
                "CommonData",
                &[19],
            ),
        ],
        &[],
    );
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&metadata).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    assert!(decoded.parameters.is_empty());
    assert!(decoded.trailer_flags.is_empty());
    assert!(decoded.null_owners.is_empty());
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with an unparsed parameter frame");
    };
    assert_eq!(unparsed.frames.len(), 1);
}

#[test]
fn decode_reports_discriminated_trailers_that_set_an_uninterpreted_flag() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let fixture = |record_index: u32, trailer: &[u8]| {
        let mut frame = parameter_record(Some(0x5d), "1", "TangencyWeight", None, "d4", 1.0);
        frame[7..11].copy_from_slice(&record_index.to_le_bytes());
        let expression_end = 46 + 4 + 2;
        frame.splice(expression_end..expression_end + 9, trailer.iter().copied());
        frame
    };
    let mut bulk = fixture(71, &[0, 0, 0, 0, 0, 1, 0, 0, 0, 0]);
    bulk.extend_from_slice(&fixture(72, &[0; 10]));
    bulk.extend_from_slice(&fixture(73, &[0, 0, 0, 0, 1, 0, 0, 0, 0]));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();

    assert_eq!(decoded.parameters.len(), 3);
    assert!(decoded.unparsed.is_empty());
    let [flagged] = decoded.trailer_flags.as_slice() else {
        panic!("one trailer flag report");
    };
    assert_eq!(flagged.stream, stream);
    assert_eq!(flagged.count, 2);
}

#[test]
fn decode_reports_discriminated_trailers_that_set_the_driven_flag() {
    let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let fixture = |record_index: u32, trailer: &[u8]| {
        let mut frame = parameter_record(Some(0x5d), "1", "Linear Dimension-2", None, "d4", 1.0);
        frame[7..11].copy_from_slice(&record_index.to_le_bytes());
        let expression_end = 46 + 4 + 2;
        frame.splice(expression_end..expression_end + 9, trailer.iter().copied());
        frame
    };
    let mut bulk = fixture(71, &[0, 0, 0, 0, 0, 0, 1, 0, 0, 0]);
    bulk.extend_from_slice(&fixture(72, &[0; 10]));
    bulk.extend_from_slice(&fixture(73, &[0, 0, 0, 0, 0, 1, 0, 0, 0]));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();

    let driven = decoded
        .parameters
        .iter()
        .map(|parameter| (parameter.record_index, parameter.flags().driven))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        driven,
        std::collections::BTreeMap::from([(71, true), (72, false), (73, true)])
    );
    assert!(decoded.trailer_flags.is_empty());
    let [flagged] = decoded.driven_flags.as_slice() else {
        panic!("one driven flag report");
    };
    assert_eq!(flagged.stream, stream);
    assert_eq!(flagged.count, 2);
}
