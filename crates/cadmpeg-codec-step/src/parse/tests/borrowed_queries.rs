// SPDX-License-Identifier: Apache-2.0
//! A single selected index list has no query scratch allocation.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const SOURCE: &[u8] = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#2=A();#1=A();#3=B();ENDSEC;END-ISO-10303-21;";

#[test]
fn single_selected_queries_need_no_collection_or_backing() {
    let (exchange, _) = crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for names in [&["A"][..], &["MISSING", "A", "MISSING"][..]] {
        let ids = exchange.entities_any(&ctx, names).unwrap()
            .map(|row| row.map(|(id, _)| id)).collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(ids, [1, 2]);
    }
    let ids = exchange.matching_entity_ids(&ctx, |name| name == "A").unwrap()
        .collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(ids, [1, 2]);
    assert_eq!(ctx.resource_refusal(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn two_selected_lists_admit_actual_reference_scratch_and_keep_refusal() {
    let (exchange, _) = crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner).unwrap();
    for matching in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = if matching {
            exchange.matching_entity_ids(&ctx, |_| true).err().unwrap()
        } else {
            exchange.entities_any(&ctx, &["A", "B"]).err().unwrap()
        };
        let CodecError::ResourceLimit(refusal) = error else { panic!("actual scratch growth must be refused"); };
        assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
        assert_eq!(refusal.used, 0);
        assert_eq!(refusal.additional, 1);
        assert_eq!(refusal.limit, 0);
        assert_eq!(refusal.operation, if matching { "STEP matching entity lists" } else { "STEP entity union lists" });
        assert_eq!(ctx.resource_refusal(), Some(refusal));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
    }
}

#[test]
fn last_matching_predicate_refusal_survives_an_empty_selection() {
    let (exchange, _) = crate::test_support::with_service_context(SOURCE, crate::parse::parse_inner).unwrap();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = exchange.matching_entity_ids(&ctx, |name| {
        if name == "B" {
            assert!(matches!(ctx.charge_work(u64::MAX, "STEP last predicate refusal"), Err(CodecError::ResourceLimit(_))));
        }
        false
    }).err().unwrap();
    let CodecError::ResourceLimit(refusal) = error else { panic!("the original predicate refusal must survive"); };
    assert_eq!(refusal.operation, "STEP last predicate refusal");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal));
}
