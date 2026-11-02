// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use zip::CompressionMethod;

use super::super::decode_parameters;
use super::typed_parameter_record;
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::test_support::parameter_record;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::zip_test::with_scan;

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
    assert_eq!(unparsed.count, 1);
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
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    assert_eq!(decoded.parameters.len(), 1);
    let [unparsed] = decoded.unparsed.as_slice() else {
        panic!("expected one stream with unparsed untyped parameter frames");
    };
    assert_eq!(unparsed.stream, stream);
    assert_eq!(unparsed.count, 1);
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
    let (bulk, valid_at) = bulk_with_leading_unmatched_frame();
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file(stream, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let decoded = with_scan(&archive, decode_parameters).unwrap();
    let [parameter] = decoded.parameters.as_slice() else {
        panic!("expected the parameter after the unmatched frame");
    };
    assert_eq!(parameter.record_index, 71);
    assert_eq!(
        parameter.byte_offset(),
        Some(u64::try_from(valid_at).unwrap())
    );
    assert_eq!(parameter.expression(), "Width / 2");
    assert!(decoded.unparsed.is_empty());
}

#[test]
fn parameter_scan_reaches_each_header_within_a_length_budget() {
    let (bulk, valid_at) = bulk_with_leading_unmatched_frame();
    let mut position = 0usize;
    let mut visited = Vec::new();
    while let Some(at) = next_indexed_record_offset(&bulk, position) {
        visited.push(at);
        assert!(
            visited.len() <= bulk.len() + 1,
            "parameter scan exceeded its step budget at offset {at}"
        );
        position = at + 1;
    }
    assert_eq!(visited.first(), Some(&0));
    assert!(visited.contains(&valid_at));
    assert_eq!(
        visited.len(),
        visited.iter().collect::<std::collections::HashSet<_>>().len()
    );
}
