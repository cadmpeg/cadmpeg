// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::parameter::{Token, TokenValue};

fn tokens() -> Vec<Token> {
    let mut tokens = (0..1024)
        .map(|index| Token {
            value: TokenValue::Omitted,
            span: index..index + 1,
        })
        .collect::<Vec<_>>();
    tokens[0] = Token {
        value: TokenValue::String(b"abc".to_vec()),
        span: 0..3,
    };
    tokens
}

#[test]
fn native_token_copy_first_payload_refusal_does_not_admit_the_tail() {
    let tokens = tokens();
    let slots = u64::try_from(tokens.len() * std::mem::size_of::<Token>()).unwrap();
    for (dimension, work_limit) in [
        (ResourceDimension::WorkUnits, 1),
        (ResourceDimension::RetainedBytes, 4),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work_limit;
        policy.limits.max_retained_bytes = slots;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = super::super::copy_native_tokens(&ctx, &tokens).unwrap_err();
        let CodecError::ResourceLimit(first) = error else {
            panic!("expected resource refusal: {error:?}");
        };
        assert_eq!(first.dimension, dimension);
        assert_eq!(first.operation, "iges native token bytes");
        assert_eq!(first.used, if dimension == ResourceDimension::WorkUnits { 1 } else { slots });
        assert_eq!(first.additional, 3);
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(actual)) if actual == first
        ));
    }
}

#[test]
fn native_token_copy_admits_each_visit_and_the_end_probe_once() {
    let tokens = tokens();
    // 1,024 yielded tokens, one end probe, and three copied string bytes.
    const WORK: u64 = 1024 + 1 + 3;
    for limit in [WORK - 1, WORK] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::super::copy_native_tokens(&ctx, &tokens);
        if limit == WORK {
            assert_eq!(result.unwrap(), tokens);
            ctx.finish_session().unwrap();
        } else {
            let CodecError::ResourceLimit(first) = result.unwrap_err() else {
                panic!("expected work refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "iges native token scan");
            assert_eq!(first.used, WORK - 1);
            assert_eq!(first.additional, 1);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == first
            ));
        }
    }
}

#[test]
fn native_fem_scan_pays_skipped_rows_before_first_slot_refusal() {
    let mut directory = (0..512)
        .map(|index| crate::test_support::directory_target(2 * index + 1, 116))
        .collect::<Vec<_>>();
    directory[3] = crate::test_support::directory_target(7, 134);
    let resolver_arena = DecodeArena::new();
    let (resolver_ctx, _) =
        DecodeContext::from_root_bytes(&[], &resolver_arena, &DecodePolicy::service()).unwrap();
    let resolver = crate::graph::ParameterResolver::new(&directory, &resolver_ctx).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::fem::build(&directory, &[], &resolver, &ctx).unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected collection refusal: {error:?}");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges FEM native entities");
    assert_eq!(first.used, 0);
    assert_eq!(first.additional, 1);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
}
