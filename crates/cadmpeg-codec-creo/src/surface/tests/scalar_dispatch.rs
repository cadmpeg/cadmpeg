// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::scalar::ScalarCache;
use crate::surface::{scalar_tokens, SurfaceKind, SurfaceParameterScalar};

#[test]
fn surface_scalar_dispatch_admits_only_visited_unknown_offsets_and_no_terminal_probe() {
    for code in [0x22, 0x24, 0x25, 0x26, 0x28, 0x29, 0x2a, 0x2c] {
        let kind = SurfaceKind::from_byte(code).expect("surface family");
        for count in [0, 1, 7, 17, 257] {
            let body = vec![0xed; count]; // No row-lane scalar or layout opener.
            let work = cadmpeg_core::decode::u64_from_index(count);
            for cap in 0..=work + 1 {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = cap;
                policy.limits.max_materialized_bytes = 0;
                policy.limits.max_retained_bytes = 0;
                policy.limits.max_collection_items = 0;
                policy.limits.max_entities = 0;
                policy.limits.max_recursion_depth = 0;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
                let run = || scalar_tokens(&ctx, kind, &body, &ScalarCache::default());
                let original = if cap < work {
                    let CodecError::ResourceLimit(refusal) = run().expect_err("dispatch Work cap") else {
                        panic!("resource refusal");
                    };
                    assert_eq!(refusal.operation, "creo surface scalar token dispatch");
                    assert_eq!((refusal.used, refusal.additional), (cap, 1));
                    refusal
                } else {
                    assert!(run().expect("every visited offset fits").is_empty());
                    let refusal = ctx.charge_work_limit(cap - work + 1, "after scalar dispatch")
                        .expect_err("exact remaining Work proves no terminal probe");
                    assert_eq!((refusal.used, refusal.additional), (work, cap - work + 1));
                    refusal
                };
                assert_eq!(original.dimension, ResourceDimension::WorkUnits);
                assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            }
        }
    }
}

#[test]
fn surface_scalar_dispatch_skips_scalar_interior_bytes_and_keeps_raw_identity() {
    // Head46 replaces the high IEEE byte 0x40. The low seven bytes belong
    // to this scalar even when they look like scalar heads or delimiters.
    let raw = [0x46, 0x00, 0xe3, 0xe0, 0xf7, 0xe4, 0x0f, 0x18];
    let expected = f64::from_bits(0x4000_e3e0_f7e4_0f18);
    for prefix in [0, 1, 7, 17, 257] {
        for suffix in [0, 1, 7, 17, 257] {
            let mut body = vec![0xed; prefix];
            body.extend_from_slice(&raw);
            body.extend(std::iter::repeat_n(0xed, suffix));
            let dispatches = cadmpeg_core::decode::u64_from_index(prefix + 1 + suffix);
            let work = dispatches + 8; // One retained eight-byte copy.
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            policy.limits.max_collection_items = 1;
            policy.limits.max_retained_bytes = 8 + cadmpeg_core::decode::u64_from_index(
                4 * std::mem::size_of::<SurfaceParameterScalar>(),
            ); // Initial token backing capacity is four; no existing bytes move.
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_entities = 0;
            policy.limits.max_recursion_depth = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let run = || scalar_tokens(&ctx, SurfaceKind::Cylinder, &body, &ScalarCache::default());
            let tokens = run().expect("exact dispatch, copy and backing costs");
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].value, Some(expected));
            assert_eq!(tokens[0].raw, raw);
            assert_eq!(tokens[0].offset, prefix);
            let original = ctx.charge_work_limit(1, "after scalar-owned interior")
                .expect_err("only dispatches and the copy consumed Work");
            assert_eq!((original.dimension, original.used, original.additional),
                (ResourceDimension::WorkUnits, work, 1));
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn surface_scalar_dispatch_refuses_before_copy_or_token_storage() {
    let raw = [0x46, 0, 0, 0, 0, 0, 0, 0];
    for prefix in [0, 1, 7, 17, 257] {
        let mut body = vec![0xed; prefix];
        body.extend_from_slice(&raw);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let cap = cadmpeg_core::decode::u64_from_index(prefix);
        policy.limits.max_work_units = cap;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || scalar_tokens(&ctx, SurfaceKind::Cylinder, &body, &ScalarCache::default());
        let CodecError::ResourceLimit(original) = run().expect_err("scalar dispatch precedes copying") else {
            panic!("resource refusal");
        };
        assert_eq!((original.dimension, original.used, original.additional),
            (ResourceDimension::WorkUnits, cap, 1));
        assert_eq!(original.operation, "creo surface scalar token dispatch");
        assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}
