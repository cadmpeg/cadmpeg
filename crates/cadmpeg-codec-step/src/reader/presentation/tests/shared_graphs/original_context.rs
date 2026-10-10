// SPDX-License-Identifier: Apache-2.0
//! Shared color queries and claims preserve their original session refusal.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::super::super::{StyleColors, StyleDomain};

fn cache_refusal(cached: bool, references: &[u64]) {
    let exchange = super::exchange("#1=COLOUR_RGB('red',1.,0.,0.);");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("empty root");
    let mut shared = StyleColors {
                exchange: &exchange,
        prefixes: BTreeMap::new(),
        values: BTreeMap::new(),
        queries: Vec::new(),
        frames: Vec::new(),
        frame_storage: ctx.reserve_scoped(0, "STEP shared color frames").expect("frame scope"),
                    completed: super::super::super::ColorCompletions::new(&ctx).expect("completion cache"),
        ctx: &ctx,
        storage: ctx.reserve_scoped(0, "test original shared color").expect("cache scope"),
    };
    let query_storage = RefCell::new(ctx.reserve_scoped(0, "test color query").expect("query scope"));
    let report_storage = RefCell::new(ctx.reserve_scoped(0, "test color report").expect("report scope"));
    let mut losses = Vec::new();
    if cached {
        let (_, value) = shared.resolve(references, StyleDomain::Surface,
            &query_storage, (&mut losses, &report_storage)).expect("initial color query");
        assert!(value.color.is_some());
    }
    let prior_prefixes = shared.prefixes.len();
    let prior_queries = shared.values.len();
    assert_eq!(prior_queries, usize::from(cached));
    let CodecError::ResourceLimit(original) = ctx.charge_work(u64::MAX, "test original color query refusal")
        .expect_err("original context refuses") else { panic!("resource refusal"); };
    assert!(matches!(shared.resolve(references, StyleDomain::Surface,
        &query_storage, (&mut losses, &report_storage)),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert_eq!(shared.prefixes.len(), prior_prefixes);
    assert_eq!(shared.values.len(), prior_queries);
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(losses);
    drop(report_storage);
    drop(query_storage);
    drop(shared);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn shared_color_cache_miss_preserves_original_session_refusal() {
    cache_refusal(false, &[1]);
}

#[test]
fn shared_color_cache_hit_preserves_original_session_refusal() {
    cache_refusal(true, &[1]);
}

#[test]
fn shared_color_empty_query_preserves_original_session_refusal() {
    cache_refusal(false, &[]);
}

#[test]
fn shared_color_claim_preserves_original_session_refusal() {
    let exchange = super::exchange("#1=COLOUR_RGB('red',1.,0.,0.);");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("empty root");
    let mut shared = StyleColors {
                exchange: &exchange,
        prefixes: BTreeMap::new(),
        values: BTreeMap::new(),
        queries: Vec::new(),
        frames: Vec::new(),
        frame_storage: ctx.reserve_scoped(0, "STEP shared color frames").expect("frame scope"),
                    completed: super::super::super::ColorCompletions::new(&ctx).expect("completion cache"),
        ctx: &ctx,
        storage: ctx.reserve_scoped(0, "test original shared color").expect("cache scope"),
    };
    let query_storage = RefCell::new(ctx.reserve_scoped(0, "test color query").expect("query scope"));
    let report_storage = RefCell::new(ctx.reserve_scoped(0, "test color report").expect("report scope"));
    let mut losses = Vec::new();
    let (query, cached) = shared.resolve(&[1], StyleDomain::Surface,
        &query_storage, (&mut losses, &report_storage)).expect("initial color query");
    assert_eq!(cached.claims.iter().map(|(key, _)| key.0).collect::<Vec<_>>(), [1]);
    assert!(!cached.claims_applied);
    let mut claim_storage = ctx.reserve_scoped(0, "test color claims").expect("claim scope");
    let mut claims = BTreeSet::new();
    let CodecError::ResourceLimit(original) = ctx.charge_work(u64::MAX, "test original color claim refusal")
        .expect_err("original context refuses") else { panic!("resource refusal"); };
    assert!(matches!(shared.claim(query, StyleDomain::Surface, (&mut claims, &mut claim_storage)),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(claims.is_empty());
    assert!(!shared.queries[*shared.values.get(&(query, StyleDomain::Surface)).expect("original cached query")].claims_applied);
    assert!(losses.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(claims);
    drop(claim_storage);
    drop(losses);
    drop(report_storage);
    drop(query_storage);
    drop(shared);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}
