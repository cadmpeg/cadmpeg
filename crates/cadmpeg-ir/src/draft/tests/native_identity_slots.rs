// SPDX-License-Identifier: Apache-2.0

use crate::draft::CommitSession;
use crate::ids::Identity;
use crate::native::NativeRecord;
use crate::CadIr;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn committed_native_identity_cache_borrows_text_from_document() {
    let id = format!("test:native:record#{}", "n".repeat(1024));
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut(crate::codec_id!("test"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("records"),
            vec![NativeRecord::new(Identity::new(&id).unwrap(), serde_json::Map::new()).unwrap()],
        );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();
    assert!(session.contains(&id).unwrap());
    assert!(session.contains(&id).unwrap());
    assert!(!session.contains("test:native:record#missing").unwrap());
    drop(session);
    ctx.finish_session().unwrap();
}

#[test]
fn committed_native_identity_positions_cover_namespaces_arenas_and_rows() {
    let mut ir = CadIr::empty();
    for namespace in ["alpha", "beta"] {
        for arena in ["first", "second"] {
            let records = (0..3)
                .map(|row| {
                    let id = format!("test:{namespace}:{arena}#{row}");
                    NativeRecord::new(Identity::new(id).unwrap(), serde_json::Map::new()).unwrap()
                })
                .collect();
            ir.native
                .namespace_mut(crate::design::names::CodecId::new(namespace).expect("a codec id"))
                .arenas_mut()
                .insert(
                    cadmpeg_core::text::NonBlankString::new(arena).expect("an arena name"),
                    records,
                );
        }
    }
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let mut session = CommitSession::new(&mut ir, &ctx, None).unwrap();
    for namespace in ["alpha", "beta"] {
        for arena in ["first", "second"] {
            for row in 0..3 {
                assert!(session
                    .contains(&format!("test:{namespace}:{arena}#{row}"))
                    .unwrap());
            }
        }
    }
    assert!(!session.contains("test:beta:second#3").unwrap());
}

#[test]
fn committed_native_cache_keeps_paths_when_earlier_map_keys_are_inserted() {
    let target = "test:native:record#existing";
    let record =
        || NativeRecord::new(Identity::new(target).unwrap(), serde_json::Map::new()).unwrap();
    let mut ir = CadIr::empty();
    ir.native
        .namespace_mut(crate::codec_id!("zeta"))
        .arenas_mut()
        .insert(cadmpeg_core::nonblank_literal!("last"), vec![record()]);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut session = CommitSession::new(ir, &ctx, None).unwrap();
    assert!(session.contains(target).unwrap());
    // Exercise the cache owner directly: map-key insertion does not reorder rows.
    session
        .state
        .base
        .native
        .namespace_mut(crate::codec_id!("alpha"));
    session
        .state
        .base
        .native
        .namespace_mut(crate::codec_id!("zeta"))
        .arenas_mut()
        .insert(cadmpeg_core::nonblank_literal!("first"), Vec::new());
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
    ir.native
        .namespace_mut(crate::codec_id!("native"))
        .arenas_mut()
        .insert(
            cadmpeg_core::nonblank_literal!("records"),
            vec![
                NativeRecord::new(Identity::new(stored).unwrap(), serde_json::Map::new()).unwrap(),
            ],
        );
    let mut index = crate::draft::CommittedIdentityIndex::default();
    index.insert(
        crate::draft::identity_hash(missing),
        vec![crate::draft::CommittedIdentity::Native {
            namespace_hash: crate::draft::identity_hash("native"),
            arena_hash: crate::draft::identity_hash("records"),
            record: 0,
        }],
    );
    assert!(!crate::draft::committed_identity_contains(
        &ir,
        &[],
        &index,
        missing
    ));
}
