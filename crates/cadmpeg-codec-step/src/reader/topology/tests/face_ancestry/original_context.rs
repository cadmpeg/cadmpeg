// SPDX-License-Identifier: Apache-2.0
//! Face cache queries and claims preserve the constructor's session refusal.

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::{claim_face_ancestors, face_attributes, FaceAttributeCache, FaceResolution};

const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=FACE('',(#2));#2=FACE_BOUND('',#3,.T.);#3=EDGE_LOOP('',());#4=DUMMY();ENDSEC;END-ISO-10303-21;";

fn cache_refusal(id: u64, populated: bool) {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid face references");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::service())
        .expect("source fits policy");
    let mut cache = FaceAttributeCache::new(&ctx).expect("empty face cache");
    let mut active = BTreeSet::new();
    let record = exchange.records().get(&id).expect("query record");
    if populated {
        let result = face_attributes(id, record, &exchange, &mut active, &mut cache)
            .expect("initial cache resolution");
        match id {
            1 => assert!(matches!(result, FaceResolution::Resolved(_))),
            4 => assert!(matches!(result, FaceResolution::Unrecognized)),
            _ => panic!("unexpected fixture query"),
        }
    }
    assert!(active.is_empty());
    assert_eq!(cache.completed.len(), usize::from(populated));
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(u64::MAX, "test original face query refusal")
        .expect_err("original context refuses")
    else {
        panic!("resource refusal");
    };
    assert!(matches!(
        face_attributes(id, record, &exchange, &mut active, &mut cache),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original
    ));
    assert!(active.is_empty());
    assert_eq!(cache.completed.len(), usize::from(populated));
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(active);
    drop(cache);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}

#[test]
fn face_cache_miss_preserves_original_session_refusal() {
    cache_refusal(1, false);
}

#[test]
fn face_cache_hit_preserves_original_session_refusal() {
    cache_refusal(1, true);
}

#[test]
fn unrecognized_face_cache_miss_preserves_original_session_refusal() {
    cache_refusal(4, false);
}

#[test]
fn unrecognized_face_cache_hit_preserves_original_session_refusal() {
    cache_refusal(4, true);
}

#[test]
fn face_ancestor_claim_preserves_original_session_refusal() {
    let (exchange, _) =
        crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner)
            .expect("valid face references");
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(SOURCE, &arena, &DecodePolicy::service())
        .expect("source fits policy");
    let mut cache = FaceAttributeCache::new(&ctx).expect("empty face cache");
    assert!(matches!(face_attributes(1, exchange.records().get(&1).expect("face"),
        &exchange, &mut BTreeSet::new(), &mut cache).expect("resolve ancestor"),
        FaceResolution::Resolved(_)));
    let mut claim_storage = ctx.reserve_scoped(0, "test face claims").expect("claim scope");
    let mut seen_storage = ctx.reserve_scoped(0, "test seen ancestors").expect("seen scope");
    let mut claims = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let CodecError::ResourceLimit(original) = ctx
        .charge_work(u64::MAX, "test original face claim refusal")
        .expect_err("original context refuses")
    else {
        panic!("resource refusal");
    };
    assert!(matches!(claim_face_ancestors(Some(1), &cache,
        (&mut claims, &mut claim_storage), (&mut seen, &mut seen_storage)),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
    assert!(claims.is_empty());
    assert!(seen.is_empty());
    assert_eq!(cache.completed.len(), 1);
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(claims);
    drop(seen);
    drop(claim_storage);
    drop(seen_storage);
    drop(cache);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(refusal)) if refusal == original));
}
