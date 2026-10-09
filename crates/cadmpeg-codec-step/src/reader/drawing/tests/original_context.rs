// SPDX-License-Identifier: Apache-2.0
//! Wrapper queries and their target context use the original decode session.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::super::{TargetContext, WrapperCache};

fn exchange() -> crate::parse::Exchange {
    crate::test_support::with_service_context(
        b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM();ENDSEC;END-ISO-10303-21;",
        crate::parse::parse_inner,
    ).expect("wrapper exchange").0
}

fn cache_refusal(cached: bool) {
    let exchange = exchange();
    let identities = BTreeMap::new();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("empty root");
    let cache = WrapperCache::new(&ctx).expect("original cache context");
    if cached {
        assert!(cache.resolve(1, &identities, &exchange).expect("initial query").is_none());
    }
    let prior_entries = usize::from(cached);
    assert_eq!(cache.values.borrow().len(), prior_entries);
    let CodecError::ResourceLimit(original) = ctx.charge_work(u64::MAX, "test original wrapper refusal")
        .expect_err("original context refuses") else { panic!("resource refusal"); };
    assert!(matches!(cache.resolve(1, &identities, &exchange),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert_eq!(cache.values.borrow().len(), prior_entries);
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(cache);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn drawing_wrapper_cache_miss_preserves_original_session_refusal() {
    cache_refusal(false);
}

#[test]
fn drawing_wrapper_cache_hit_preserves_original_session_refusal() {
    cache_refusal(true);
}

#[test]
fn drawing_target_context_preserves_original_session_refusal() {
    let exchange = exchange();
    let identities = BTreeMap::new();
    let typed = HashSet::new();
    let documents = BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("empty root");
    let target_context = TargetContext {
        target_identities: &identities,
        known_typed: &typed,
        exchange: &exchange,
        external_documents: &documents,
        wrappers: WrapperCache::new(&ctx).expect("original target context"),
    };
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "test original drawing target refusal")
        .expect_err("original context refuses") else { panic!("resource refusal"); };
    assert!(matches!(target_context.resolve(1),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(target_context.wrappers.values.borrow().is_empty());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(target_context);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}
