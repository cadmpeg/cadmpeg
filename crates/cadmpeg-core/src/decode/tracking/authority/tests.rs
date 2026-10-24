// SPDX-License-Identifier: Apache-2.0
//! Session authority supplies identity without source or read authority.

use crate::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;

#[test]
fn actual_session_tokens_are_stable_and_distinct_without_creating_captures() {
    let arena = DecodeArena::new();
    let (first, _) =
        DecodeContext::from_root_bytes(b"same", &arena, &DecodePolicy::service()).unwrap();
    let (second, _) =
        DecodeContext::from_root_bytes(b"same", &arena, &DecodePolicy::service()).unwrap();
    let token = first.session_token().unwrap();
    assert_eq!(first.session_token().unwrap(), token);
    assert_ne!(second.session_token().unwrap(), token);
    assert!(first.take_record_captures().unwrap().is_empty());
    assert!(second.take_record_captures().unwrap().is_empty());
}

#[test]
fn current_batch_tracks_actual_nested_scope_and_rollback() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service()).unwrap();
    assert_eq!(ctx.current_record_batch().unwrap(), None);
    let ((outer, inner), actual_outer) = ctx
        .record_batch(|| {
            let outer = ctx.current_record_batch().unwrap().expect("outer batch");
            let (inner, actual_inner) =
                ctx.record_batch(|| Ok(ctx.current_record_batch().unwrap().expect("inner batch")))?;
            assert_eq!(inner, actual_inner);
            assert_ne!(inner, outer);
            assert_eq!(ctx.current_record_batch().unwrap(), Some(outer));
            let failed = ctx.record_batch(|| -> Result<(), CodecError> {
                assert_ne!(ctx.current_record_batch().unwrap(), Some(outer));
                Err(CodecError::malformed("bounded batch rollback"))
            });
            assert!(failed.is_err());
            assert_eq!(ctx.current_record_batch().unwrap(), Some(outer));
            Ok((outer, inner))
        })
        .unwrap();
    assert_eq!(outer, actual_outer);
    assert_ne!(inner, outer);
    assert_eq!(ctx.current_record_batch().unwrap(), None);
    assert!(ctx.take_record_captures().unwrap().is_empty());
}

#[test]
fn authority_access_propagates_actual_session_refusal() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let requested = policy.limits.max_retained_bytes.checked_add(1).unwrap();
    let error = ctx
        .retain_bytes(requested, "authority control refusal")
        .unwrap_err();
    let CodecError::ResourceLimit(expected) = error else {
        panic!("actual resource refusal")
    };
    assert_eq!(expected.dimension, ResourceDimension::RetainedBytes);
    assert!(
        matches!(ctx.session_token(), Err(CodecError::ResourceLimit(actual)) if actual == expected)
    );
    assert!(
        matches!(ctx.current_record_batch(), Err(CodecError::ResourceLimit(actual)) if actual == expected)
    );
}
