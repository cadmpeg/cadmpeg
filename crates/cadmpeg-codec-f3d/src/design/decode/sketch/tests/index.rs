// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::IndexedRecordOffsets;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

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
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &bytes).unwrap();
    assert_eq!(records.offsets(7), &[0, 11]);

    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(matches!(
        IndexedRecordOffsets::build(&ctx, &bytes),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn indexed_record_offsets_charge_distinct_keys() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&indexed_header(7));
    bytes.extend_from_slice(&indexed_header(8));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    assert!(matches!(
        IndexedRecordOffsets::build(&ctx, &bytes),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn borrowed_record_cache_refuses_map_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = std::collections::HashMap::new();
    assert!(matches!(
        crate::design::decode::sketch::cached_borrowed_record_offsets(
            &ctx, &mut cache, "stream", &[]
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::CollectionItems
    ));
}

#[test]
fn owned_record_cache_refuses_retained_key() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = std::collections::HashMap::new();
    assert!(matches!(
        crate::design::decode::sketch::cached_owned_record_offsets(
            &ctx, &mut cache, "stream", &[]
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}

#[test]
fn owned_record_cache_refuses_temporary_lookup() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = std::collections::HashMap::new();
    assert!(matches!(
        crate::design::decode::sketch::cached_owned_record_offsets(
            &ctx, &mut cache, "stream", &[]
        ),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn scoped_stream_copy_refuses_materialized_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_materialized_bytes = 5;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::sketch::copy_scoped_stream(&ctx, "stream"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::MaterializedBytes
    ));
}

#[test]
fn charged_native_scope_matches_identity_encoding() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for name in ["Design/BulkStream.dat", "a:b#c%d e", "α\u{2003}β"] {
        assert_eq!(
            crate::design::decode::sketch::native_scope_charged(&ctx, name).unwrap(),
            crate::ids::native_scope(name)
        );
    }
}

#[test]
fn native_scope_key_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        crate::design::decode::sketch::native_scope_charged(&ctx, "a"),
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::RetainedBytes
    ));
}
