// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::{decode_headers_for_indices_from_stream, entity_meta_scope};
use crate::metastream::PrimaryFrames;

fn indexed_header(index: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"256");
    bytes[7..].copy_from_slice(&index.to_le_bytes());
    bytes
}

#[test]
fn indexed_record_offsets_list_each_offset_per_key() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let records = crate::design::test_support::test_primary_frames(&bytes);
    assert_eq!(records.offsets(7), &[0, 11]);
}

#[test]
fn primary_frames_do_not_select_a_header_shaped_payload() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(42));
    bytes.extend_from_slice(&indexed_header(8));
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(&bytes, &[7, 8], &[]);
    let metadata = crate::metastream::parse(&metadata, "MetaStream.dat").unwrap();
    let frames = crate::design::decode::meta::design_primary_frames(&bytes, &metadata).unwrap();
    let records = crate::design::decode::meta::primary_frame_extents(&frames);
    assert!(records.offsets(42).is_empty());
    assert_eq!(records.next_at_or_after(11), Some(22));
}

#[test]
fn record_header_stream_emits_one_identified_header() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let scope = crate::ids::native_scope("BulkStream.dat");
    let wanted = std::collections::HashSet::from([(scope.as_str(), 7)]);
    let mut out = Vec::new();
    let metadata = crate::test_support::zip_test::indexed_fixture_metastream(&bytes, &[7], &[]);
    let metadata = crate::metastream::parse(&metadata, "MetaStream.dat").unwrap();
    let frames = crate::design::decode::meta::design_primary_frames(&bytes, &metadata).unwrap();
    decode_headers_for_indices_from_stream("BulkStream.dat", &scope, &frames, &wanted, &mut out);
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
