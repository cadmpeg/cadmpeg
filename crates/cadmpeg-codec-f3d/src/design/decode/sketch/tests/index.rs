// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::{
    decode_headers_for_indices_from_stream, entity_meta_scope, IndexedRecordOffsets,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn indexed_header(index: u32) -> [u8; 11] {
    let mut bytes = [0; 11];
    bytes[..4].copy_from_slice(&3_u32.to_le_bytes());
    bytes[4..7].copy_from_slice(b"001");
    bytes[7..].copy_from_slice(&index.to_le_bytes());
    bytes
}

#[test]
fn indexed_record_offsets_charge_each_key_and_offset() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &bytes).unwrap();
    assert_eq!(records.offsets(7), &[0, 11]);
}

#[test]
fn record_header_stream_charges_emitted_index_output_and_id() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(7));
    let scope = crate::ids::native_scope("BulkStream.dat");
    let wanted = std::collections::HashSet::from([(scope.as_str(), 7)]);
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut out = Vec::new();
    decode_headers_for_indices_from_stream(
        &ctx,
        "BulkStream.dat",
        &scope,
        &bytes,
        &wanted,
        &mut out,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].id,
        crate::ids::native_design_record_header_id("BulkStream.dat", 0)
    );
}

#[test]
fn entity_header_meta_scope_and_module_refuse_byte_limits() {
    let bulk_scope = crate::ids::native_scope("a/BulkStream.dat");
    let expected = crate::ids::native_scope("a/MetaStream.dat");
    let actual = entity_meta_scope(&bulk_scope).unwrap();
    assert_eq!(actual, expected);
}
