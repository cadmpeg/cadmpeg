// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::parameter::{Token, TokenValue};

fn visit_bounds(copy_tokens: bool) {
    let operation = if copy_tokens { "iges native token scan" } else { "iges native visit control" };
    for count in [1_usize, 64] {
        let tokens: Vec<_> = (0..count).map(|index| Token {
            value: TokenValue::Integer(i64::try_from(index).unwrap()), span: 0..0,
        }).collect();
        let total = u64::try_from(count).unwrap();
        for (work, accepts) in [(0, false), (total - 1, false), (total, true)] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            policy.limits.max_materialized_bytes = 0;
            policy.limits.max_collection_items = if copy_tokens { total } else { 0 };
            policy.limits.max_retained_bytes = if copy_tokens {
                u64::try_from(count * std::mem::size_of::<Token>()).unwrap()
            } else { 0 };
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            // Scalar token clones and absent link mappings allocate no payload.
            // The token vector reserves exactly count slots before the visits.
            let result = if copy_tokens {
                super::super::copy_native_tokens(&ctx, &tokens).map(|copies| assert_eq!(copies, tokens))
            } else {
                super::super::native_entity_ids(&ctx, 0..count, operation,
                    "iges native visit control slots", |_| None).map(|ids| assert!(ids.is_empty()))
            };
            if accepts {
                result.unwrap();
                ctx.finish_session().unwrap();
            } else {
                let first = match result {
                    Err(CodecError::ResourceLimit(first)) => first,
                    Err(error) => panic!("unexpected native source error: {error:?}"),
                    Ok(()) => panic!("expected actual native source refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(first.operation, operation);
                assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
                let replay = if copy_tokens {
                    super::super::copy_native_tokens(&ctx, &tokens).map(|_| ())
                } else {
                    super::super::native_entity_ids(&ctx, 0..count, operation,
                        "iges native visit control slots", |_| None).map(|_| ())
                };
                assert!(matches!(replay, Err(CodecError::ResourceLimit(last)) if last == first));
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

#[test]
fn unmapped_native_link_sources_charge_only_actual_first_and_last_visits() {
    visit_bounds(false);
}
#[test]
fn scalar_native_token_sources_charge_only_actual_first_and_last_visits() {
    visit_bounds(true);
}

#[test]
fn empty_native_source_helpers_need_no_work_or_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let tokens = super::super::copy_native_tokens(&ctx, &[]).unwrap();
    assert!(tokens.is_empty());
    let links = super::super::native_entity_ids(&ctx, std::iter::empty::<u32>(),
        "iges native visit control", "iges native visit control slots", Some).unwrap();
    assert!(links.is_empty());
    ctx.finish_session().unwrap();
}
