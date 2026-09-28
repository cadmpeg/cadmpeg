// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::decode_lost_edge_references_from_stream;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn lost_edge_bytes() -> Vec<u8> {
    let marker = b"EDGE_REFERENCE_LOST";
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&41u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 14]);
    bytes.extend_from_slice(&(marker.len() as u32).to_le_bytes());
    bytes.extend_from_slice(marker);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&42u32.to_le_bytes());
    bytes
}

#[test]
fn lost_edge_reference_output_refuses_collection_limit() {
    let bytes = lost_edge_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut references = Vec::new();
    let error = decode_lost_edge_references_from_stream(
        &ctx,
        "BulkStream.dat",
        &bytes,
        &mut references,
    )
    .expect_err("collection limit must refuse lost edge reference");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::CollectionItems
            && failure.operation == "f3d lost edge reference output"));
}

#[test]
fn lost_edge_reference_id_refuses_retained_limit() {
    let bytes = lost_edge_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = crate::ids::native_scope("BulkStream.dat").len() as u64;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut references = Vec::new();
    let error = decode_lost_edge_references_from_stream(
        &ctx,
        "BulkStream.dat",
        &bytes,
        &mut references,
    )
    .expect_err("retained limit must refuse lost edge ID");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(failure)
        if failure.dimension == ResourceDimension::RetainedBytes
            && failure.operation == "f3d lost edge reference ID"));
}

#[test]
fn lost_edge_reference_scan_preserves_record_fields() {
    let bytes = lost_edge_bytes();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut references = Vec::new();
    decode_lost_edge_references_from_stream(&ctx, "BulkStream.dat", &bytes, &mut references)
        .expect("valid lost edge record");
    assert_eq!(references.len(), 1);
    assert_eq!(references[0].id, crate::ids::native_lost_edge_reference_id("BulkStream.dat", 0));
    assert_eq!(references[0].record_index, 41);
    assert_eq!(references[0].next_record_index, 42);
}
