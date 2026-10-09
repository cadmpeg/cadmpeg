// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::{RecordOffsetCache, ScopeNumbers};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::HashMap;

#[test]
fn scratch_cache_original_session_preserves_offsets_and_releases_storage() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&super::indexed_header(7));
    bytes.extend_from_slice(&super::indexed_header(7));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = RecordOffsetCache::new(&ctx).unwrap();
    assert_eq!(cache.get("first", &bytes).unwrap().offsets(7), &[0, 11]);
    // First construction owns this stream. A later supplied payload is unread.
    assert_eq!(cache.get("first", &[0xff]).unwrap().offsets(7), &[0, 11]);
    assert_eq!(cache.get("second", &bytes).unwrap().offsets(7), &[0, 11]);
    drop(cache);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "original cache released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn scratch_cache_original_session_fuse_gates_vacant_and_occupied_queries() {
    let bytes = super::indexed_header(7);
    for occupied in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 1024;
        let (original_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let other_arena = DecodeArena::new();
        let (other_ctx, _) = DecodeContext::from_root_bytes(&[], &other_arena, &policy).unwrap();
        let mut cache = RecordOffsetCache::new(&original_ctx).unwrap();
        if occupied {
            assert_eq!(cache.get("stream", &bytes).unwrap().offsets(7), &[0]);
        }
        let Err(CodecError::ResourceLimit(original)) = original_ctx.charge_retained(1, "original cache refusal") else {
            panic!("original zero retained ceiling must refuse");
        };
        let Err(CodecError::ResourceLimit(later)) = cache.get("stream", &bytes) else {
            panic!("cache query must use its original context");
        };
        assert_eq!(later, original);
        assert_eq!(later.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(later.operation, "original cache refusal");
        assert!(other_ctx.resource_refusal().is_none());
        other_ctx.finish_session().unwrap();
    }
}

#[test]
fn scratch_scope_original_session_preserves_first_seen_numbers_and_lifetime() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut scopes = ScopeNumbers {
        numbers: HashMap::new(),
        storage: ctx.reserve_scoped(0, "original scope test").unwrap(),
        ctx: &ctx,
    };
    for (id, expected) in [
        ("first:point#7", Some(0)),
        ("second:point#8", Some(1)),
        ("first:curve#9", Some(0)),
        ("plain", None),
        ("", None),
    ] {
        assert_eq!(scopes.of(id).unwrap(), expected);
    }
    assert_eq!(scopes.numbers.len(), 2);
    drop(scopes);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "original scopes released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn scratch_scope_original_session_fuse_gates_cached_new_and_empty_ids() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1024;
    let (original_ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let other_arena = DecodeArena::new();
    let (other_ctx, _) = DecodeContext::from_root_bytes(&[], &other_arena, &policy).unwrap();
    let mut scopes = ScopeNumbers {
        numbers: HashMap::new(),
        storage: original_ctx.reserve_scoped(0, "original scope test").unwrap(),
        ctx: &original_ctx,
    };
    assert_eq!(scopes.of("first:point#7").unwrap(), Some(0));
    let Err(CodecError::ResourceLimit(original)) = original_ctx.charge_retained(1, "original scope refusal") else {
        panic!("original zero retained ceiling must refuse");
    };
    for id in ["first:curve#8", "second:point#9", "plain", ""] {
        let Err(CodecError::ResourceLimit(later)) = scopes.of(id) else {
            panic!("every scope query must preserve its original fuse");
        };
        assert_eq!(later, original);
    }
    assert_eq!(scopes.numbers.len(), 1);
    assert!(other_ctx.resource_refusal().is_none());
    other_ctx.finish_session().unwrap();
}
