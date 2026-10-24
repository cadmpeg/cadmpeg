// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::sketch::{decode_lost_edge_references_from_stream, SourcePointers};

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

fn scan_references(
    bytes: &[u8],
    frame_ends: &std::collections::BTreeMap<usize, usize>,
) -> (
    Vec<crate::records::references::LostEdgeReference>,
    SourcePointers,
    Vec<cadmpeg_core::decode::RecordCapture>,
) {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .unwrap();
    let mut references = Vec::new();
    let mut pointers = SourcePointers::new();
    decode_lost_edge_references_from_stream(
        &ctx,
        "BulkStream.dat",
        root,
        frame_ends,
        &mut references,
        &mut pointers,
    )
    .expect("valid lost edge record");
    let captures = ctx.take_record_captures().unwrap();
    (references, pointers, captures)
}

#[test]
fn lost_edge_reference_scan_preserves_record_fields() {
    let bytes = lost_edge_bytes();
    let (references, ..) = scan_references(&bytes, &std::collections::BTreeMap::new());
    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0].id,
        crate::ids::native_lost_edge_reference_id("BulkStream.dat", 0)
    );
    assert_eq!(references[0].record_index, 41);
    assert_eq!(references[0].next_record_index, 42);
}

/// #225 comment 6358238990 item 4 and #214: the record is parsed inside its
/// record scope; its five offsets are `record_layout` pointers; the scope
/// reads the 48-byte record (header and marker) and the class tag and index
/// of the following header. With no primary index, the extent is the record.
#[test]
fn a_lost_edge_reference_capture_holds_its_source_offsets_and_reads_its_record() {
    let bytes = lost_edge_bytes();
    let (references, pointers, captures) =
        scan_references(&bytes, &std::collections::BTreeMap::new());
    assert_eq!(references.len(), 1);
    assert_eq!(
        pointers[&references[0].id],
        std::collections::BTreeMap::from([
            ("record_byte_offset".to_owned(), 0),
            ("class_tag_offset".to_owned(), 4),
            ("record_index_offset".to_owned(), 7),
            ("byte_offset".to_owned(), 29),
            ("next_byte_offset".to_owned(), 48),
        ])
    );
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), references[0].id);
    assert_eq!(captures[0].offset(), 0);
    assert_eq!(captures[0].byte_len(), 48);
    assert_eq!(
        captures[0].reads(),
        &std::collections::BTreeMap::from([(
            "file".to_owned(),
            vec![0..u64_from_index(bytes.len())]
        )])
    );
}

/// An indexed record's extent is its primary-index frame.
#[test]
fn an_indexed_lost_edge_reference_takes_its_primary_frame_extent() {
    let mut bytes = lost_edge_bytes();
    bytes.extend_from_slice(&[0; 5]);
    let frame_ends = std::collections::BTreeMap::from([(0, 48)]);
    let (references, _, captures) = scan_references(&bytes, &frame_ends);
    assert_eq!(references.len(), 1);
    assert_eq!(captures[0].offset(), 0);
    assert_eq!(captures[0].byte_len(), 48);
    let frame_ends = std::collections::BTreeMap::from([(0, 59)]);
    let (_, _, captures) = scan_references(&bytes, &frame_ends);
    assert_eq!(captures[0].byte_len(), 59);
}
