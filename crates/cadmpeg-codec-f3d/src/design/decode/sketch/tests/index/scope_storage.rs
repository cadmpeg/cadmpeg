// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::sketch::native_scope_scoped;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn scoped_native_scope_tuple_holds_actual_text_storage_without_retained_output() {
    for (name, expected) in [
        ("a:b", "f3d:a%3Ab"),
        ("α\u{2003}β", "f3d:α%E2%80%83β"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = u64::try_from(expected.len()).unwrap();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let owner = native_scope_scoped(&ctx, name).unwrap();
        assert_eq!(owner.0, expected);
        assert_eq!(owner.0.capacity(), expected.len());
        let Err(CodecError::ResourceLimit(original)) = ctx.reserve_scoped(1, "while scope tuple lives") else {
            panic!("the actual returned text backing must stay held");
        };
        assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!((original.used, original.additional), (policy.limits.max_materialized_bytes, 1));
        drop(owner);
        let Err(CodecError::ResourceLimit(later)) = native_scope_scoped(&ctx, "") else {
            panic!("dropping scratch must preserve the original refusal");
        };
        assert_eq!(later, original);
    }
}

#[test]
fn scoped_native_scope_tuple_releases_direct_and_rejected_return_storage() {
    for rejected in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 9;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        if rejected {
            let result = (|| {
                let candidate = native_scope_scoped(&ctx, "a:b")?;
                assert_eq!(candidate.0, "f3d:a%3Ab");
                Ok::<_, CodecError>(None::<()>)
            })().unwrap();
            assert_eq!(result, None);
        } else {
            let owner = native_scope_scoped(&ctx, "a:b").unwrap();
            assert_eq!(owner.0, "f3d:a%3Ab");
            drop(owner);
        }
        drop(ctx.reserve_scoped(9, "after scope tuple destruction").unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn scoped_native_scope_tuple_preserves_exact_materialized_and_original_refusal() {
    for materialized in [8, 9] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = materialized;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match native_scope_scoped(&ctx, "a:b") {
            Ok(owner) => {
                assert_eq!(materialized, 9);
                assert_eq!(owner.0, "f3d:a%3Ab");
                drop(owner);
            }
            Err(CodecError::ResourceLimit(original)) => {
                assert_eq!(materialized, 8);
                assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(original.operation, "f3d scoped native stream key");
                assert_eq!(original.additional, 9);
                assert_eq!(ctx.resource_refusal(), Some(original.clone()));
                let Err(CodecError::ResourceLimit(later)) = native_scope_scoped(&ctx, "a") else {
                    panic!("later native scope must return the same first refusal");
                };
                assert_eq!(later, original);
            }
            Err(error) => panic!("unexpected native scope error: {error}"),
        };
        if materialized == 9 {
            ctx.finish_session().unwrap();
        }
    }
}
