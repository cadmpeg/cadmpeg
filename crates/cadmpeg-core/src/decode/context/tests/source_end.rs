// SPDX-License-Identifier: Apache-2.0
//! Source exhaustion is checked against actual registered stream bounds.

use crate::decode::{ByteRange, DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn end_admission_accepts_actual_suffix_and_rejects_shorter_borrowed_window() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"prefix-tail", &arena, &DecodePolicy::service()).unwrap();
    ctx.require_source_end(root).unwrap();
    ctx.require_source_end(root.child(7, 11).unwrap()).unwrap();
    assert!(ctx.require_source_end(root.child(0, 7).unwrap()).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn registered_alias_end_uses_its_own_domain_and_rejects_foreign_session() {
    let arena = DecodeArena::new();
    let (ctx, root) =
        DecodeContext::from_root_bytes(b"xxABCyy", &arena, &DecodePolicy::service()).unwrap();
    let alias = ctx
        .register_named_slice(root, ByteRange { start: 2, end: 5 }, "member")
        .unwrap();
    ctx.require_source_end(alias).unwrap();
    assert!(ctx
        .require_source_end(alias.child(alias.start(), alias.end() - 1).unwrap())
        .is_err());
    let (foreign, foreign_root) =
        DecodeContext::from_root_bytes(b"xxABCyy", &arena, &DecodePolicy::service()).unwrap();
    assert!(foreign.require_source_end(alias).is_err());
    assert!(ctx.require_source_end(foreign_root).is_err());
    assert!(ctx.take_record_captures().unwrap().is_empty());
    assert!(foreign.take_record_captures().unwrap().is_empty());
}
