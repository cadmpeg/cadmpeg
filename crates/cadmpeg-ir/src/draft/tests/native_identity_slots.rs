// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::draft::CommitSession;
use crate::ids::Identity;
use crate::native::NativeRecord;
use crate::CadIr;

#[test]
fn committed_native_identity_cache_borrows_text_from_document() {
    let id = format!("test:native:record#{}", "n".repeat(1024));
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("test").arenas_mut().insert(
        "records".into(), vec![NativeRecord::new(Identity::new(&id).unwrap(), serde_json::Map::new()).unwrap()],
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 512;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(&mut ir, &ctx).unwrap();
    assert!(session.contains(&id).unwrap());
    assert!(session.contains(&id).unwrap());
    assert!(!session.contains("test:native:record#missing").unwrap());
    drop(session);
    let reservation = ctx.reserve_scoped(512, "committed native cache released").unwrap();
    drop(reservation);
    ctx.finish_session().unwrap();
}

#[test]
fn committed_native_identity_positions_cover_namespaces_arenas_and_rows() {
    let mut ir = CadIr::empty();
    for namespace in ["alpha", "beta"] {
        for arena in ["first", "second"] {
            let records = (0..3).map(|row| {
                let id = format!("test:{namespace}:{arena}#{row}");
                NativeRecord::new(Identity::new(id).unwrap(), serde_json::Map::new()).unwrap()
            }).collect();
            ir.native.namespace_mut(namespace).arenas_mut().insert(arena.into(), records);
        }
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut session = CommitSession::new(&mut ir, &ctx).unwrap();
    for namespace in ["alpha", "beta"] {
        for arena in ["first", "second"] {
            for row in 0..3 {
                assert!(session.contains(&format!("test:{namespace}:{arena}#{row}")).unwrap());
            }
        }
    }
    assert!(!session.contains("test:beta:second#3").unwrap());
}

#[test]
fn committed_native_cache_keeps_paths_when_earlier_map_keys_are_inserted() {
    let target = "test:native:record#existing";
    let record = || NativeRecord::new(Identity::new(target).unwrap(), serde_json::Map::new()).unwrap();
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("zeta").arenas_mut().insert("last".into(), vec![record()]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(ir, &ctx).unwrap();
    assert!(session.contains(target).unwrap());
    // Exercise the cache owner directly: map-key insertion does not reorder rows.
    session.state.base.native.namespace_mut("alpha");
    session.state.base.native.namespace_mut("zeta").arenas_mut().insert("first".into(), Vec::new());
    assert!(session.contains(target).unwrap());
    assert!(session.contains(target).unwrap());
    assert!(!session.contains("test:native:record#missing").unwrap());
    drop(session);
    ctx.finish_session().unwrap();
}

#[test]
fn committed_native_cache_checks_identity_after_path_hash_matches() {
    let stored = "test:native:record#stored";
    let missing = "test:native:record#absent";
    let mut ir = CadIr::empty();
    ir.native.namespace_mut("native").arenas_mut().insert("records".into(), vec![
        NativeRecord::new(Identity::new(stored).unwrap(), serde_json::Map::new()).unwrap(),
    ]);
    let index = std::collections::HashMap::from([(
        crate::index::identity_hash(missing),
        vec![crate::draft::CommittedIdentity::Native {
            namespace_hash: crate::index::identity_hash("native"),
            arena_hash: crate::index::identity_hash("records"),
            record: 0,
        }],
    )]);
    let ctx = cadmpeg_test_support::service_decode_context();
    assert!(!crate::draft::committed_identity_contains(&ir, &[], &index, missing, &ctx).unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn committed_native_cache_admits_map_key_hashes_before_lookup() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let target = "test:native:record#stored";
    let namespace = "native";
    let arena_name = "records";
    let mut ir = CadIr::empty();
    ir.native.namespace_mut(namespace).arenas_mut().insert(arena_name.into(), vec![
        NativeRecord::new(Identity::new(target).unwrap(), serde_json::Map::new()).unwrap(),
    ]);
    let index = std::collections::HashMap::from([(
        crate::index::identity_hash(target),
        vec![crate::draft::CommittedIdentity::Native {
            namespace_hash: crate::index::identity_hash(namespace),
            arena_hash: crate::index::identity_hash(arena_name),
            record: 0,
        }],
    )]);
    for arena_lookup in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let mut admitted = cadmpeg_core::decode::u64_from_index(target.len()) + 2;
        if arena_lookup { admitted += cadmpeg_core::decode::u64_from_index(namespace.len()) + 1; }
        policy.limits.max_work_units = admitted;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = crate::draft::committed_identity_contains(&ir, &[], &index, target, &ctx) else { panic!("native map-key hash must refuse"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.used, admitted);
        assert_eq!(limit.operation, if arena_lookup { "find committed native arena" } else { "find committed native namespace" });
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
}
