// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::decode_lost_edge_references_from_stream;

fn lost_edge_bytes() -> Vec<u8> {
    let marker = b"EDGE_REFERENCE_LOST";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(
        &(u32::try_from(marker.len()).expect("fixture value fits u32")).to_le_bytes(),
    );
    bytes.extend_from_slice(marker);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&42u32.to_le_bytes());
    bytes
}

#[test]
fn lost_edge_reference_scan_preserves_record_fields() {
    let bytes = lost_edge_bytes();
    let mut references = Vec::new();
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 41,
            start: 0,
            member_end: 48,
            end: 48,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 42,
            start: 48,
            member_end: bytes.len(),
            end: bytes.len(),
        },
    ];
    decode_lost_edge_references_from_stream("BulkStream.dat", &bytes, &frames, &mut references);
    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0].id,
        crate::ids::native_lost_edge_reference_id("BulkStream.dat", 0)
    );
    assert_eq!(references[0].record_index, 41);
    assert_eq!(references[0].next_record_index, 42);
}

#[test]
fn header_shaped_payload_does_not_select_a_lost_edge() {
    let mut bytes = lost_edge_bytes();
    let second = bytes.len();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&43_u32.to_le_bytes());
    let frames = [
        crate::metastream::PrimaryRecordFrame {
            entity_id: 41,
            start: 0,
            member_end: second,
            end: second,
        },
        crate::metastream::PrimaryRecordFrame {
            entity_id: 43,
            start: second,
            member_end: bytes.len(),
            end: bytes.len(),
        },
    ];
    let mut references = Vec::new();
    decode_lost_edge_references_from_stream("BulkStream.dat", &bytes, &frames, &mut references);
    assert!(references.is_empty());
}
