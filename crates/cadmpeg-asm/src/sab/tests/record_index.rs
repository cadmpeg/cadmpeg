// SPDX-License-Identifier: Apache-2.0

use crate::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn record_index_preserves_value_positions_and_exact_iterator_length() {
    let mut tokens = Vec::new();
    for index in 0..4096 {
        tokens.push(Token::Ident("payload".into()));
        tokens.push(Token::SubIdent("part".into()));
        tokens.push(Token::Ref(index));
    }
    let record = crate::test_support::sab::record(7, "cone-surface".into(), tokens.into(), 3, 9);
    assert_eq!(record.head(), "cone");
    assert_eq!(record.chunk_len(), 4096);
    assert_eq!(record.chunks().len(), 4096);
    assert_eq!(record.chunks().next_back(), Some(&Token::Ref(4095)));
    for index in 0..4096 {
        assert_eq!(record.chunk(index), Some(&Token::Ref(i64::try_from(index).unwrap())));
        assert_eq!(record.ref_at(index), Some(i64::try_from(index).unwrap()));
    }
    assert!(record.chunk(4096).is_none());
}

#[test]
fn record_index_refuses_before_visiting_each_token() {
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, "index ASM record chunks", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            Record::new(&ctx, 0, "x".into(), vec![Token::False; 4096].into(), 0, 0)
        });
    let CodecError::ResourceLimit(limit) = error else { panic!("index refusal"); };
    assert_eq!(limit.operation, "index ASM record chunks");
    assert_eq!(limit.additional, 1);
}

#[test]
fn record_clone_shares_the_value_index_without_visiting_payload() {
    let record = crate::test_support::sab::record(0, String::new(),
        vec![Token::False; 4096].into(), 0, 0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let copy = record.try_clone_for_decode(&ctx, "test indexed record clone").unwrap();
    assert!(std::sync::Arc::ptr_eq(&record.tokens, &copy.tokens));
    assert!(std::sync::Arc::ptr_eq(&record.chunk_positions, &copy.chunk_positions));
    assert_eq!(copy.chunk_len(), 4096);
    assert_eq!(copy.chunk(4095), Some(&Token::False));
    ctx.finish_session().unwrap();
}
