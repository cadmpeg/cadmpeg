// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::{
    decode_headers_for_indices_from_stream, entity_meta_scope, IndexedRecordOffsets,
};

fn indexed_header(index: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"001");
    bytes[7..].copy_from_slice(&index.to_le_bytes());
    bytes
}

#[test]
fn indexed_record_offsets_list_each_offset_per_key() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let records = IndexedRecordOffsets::build(&bytes);
    assert_eq!(records.offsets(7), &[0, 11]);
}

#[test]
fn indexed_records_iterate_in_ascending_record_order() {
    let mut bytes = Vec::new();
    for record_index in [900, 3, 57, 12, 400, 1, 88, 21] {
        bytes.extend_from_slice(&indexed_header(record_index));
    }
    let records = IndexedRecordOffsets::build(&bytes);
    let order = records
        .records()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(order, [1, 3, 12, 21, 57, 88, 400, 900]);
}

#[test]
fn record_header_stream_emits_one_identified_header() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let scope = crate::ids::native_scope("BulkStream.dat");
    let wanted = std::collections::HashSet::from([(scope.as_str(), 7)]);
    let mut out = Vec::new();
    decode_headers_for_indices_from_stream("BulkStream.dat", &scope, &bytes, &wanted, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].id,
        crate::ids::native_design_record_header_id("BulkStream.dat", 0)
    );
}

#[test]
fn entity_header_meta_scope_follows_the_bulk_stream() {
    let bulk_scope = crate::ids::native_scope("a/BulkStream.dat");
    let expected = crate::ids::native_scope("a/MetaStream.dat");
    let actual = entity_meta_scope(&bulk_scope).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn record_header_primary_index_overrides_a_header_shaped_payload() {
    use crate::test_support::streams_test::design_metastream_with_records;
    use crate::test_support::zip_test::with_scan;
    use std::io::{Cursor, Write};

    let bulk_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
    let mut bulk = Vec::new();
    crate::test_support::lp_ascii(&mut bulk, "690");
    bulk.extend_from_slice(&7_u32.to_le_bytes());
    bulk.extend_from_slice(&[0; 5]);
    let primary_offset = u64::try_from(bulk.len()).unwrap();
    crate::test_support::lp_ascii(&mut bulk, "256");
    bulk.extend_from_slice(&7_u32.to_le_bytes());
    bulk.extend_from_slice(&[0; 20]);
    let meta = design_metastream_with_records(
        &[(
            "6CCF41D5-40BE-48ED-A834-18F3EAED6C57",
            "",
            1,
            "Fusion",
            &[7],
        )],
        &[(7, primary_offset)],
    );
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(bulk_name, stored).unwrap();
    zip.write_all(&bulk).unwrap();
    zip.start_file("FusionAssetName[Active]/Design1/MetaStream.dat", stored)
        .unwrap();
    zip.write_all(&meta).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    with_scan(&archive, |scan| {
        let _ctx = cadmpeg_test_support::service_decode_context();
        let headers = crate::design::decode::sketch::decode_related_record_headers(
            scan,
            &[(crate::ids::native_scope(bulk_name), 7)],
        )
        .unwrap();
        assert_eq!(headers.len(), 1);
        assert_eq!(headers[0].byte_offset, primary_offset);
        assert_eq!(headers[0].class_tag.as_str(), "256");
        assert_eq!(headers[0].record_index, 7);
    });
}
