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
