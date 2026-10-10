// SPDX-License-Identifier: Apache-2.0

use crate::sab::Token;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn subtype_admission_walks_shared_definition_once_per_stream() {
    let count = 4096;
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("node".into())];
    tokens.extend((0..count).map(|_| Token::False));
    tokens.push(Token::SubtypeClose);
    let mut records = vec![crate::test_support::sab::record(
        0,
        "node".into(),
        tokens.into(),
        0,
        0,
    )];
    for index in 1..=count {
        records.push(crate::test_support::sab::record(
            index,
            "ref".into(),
            vec![
                Token::SubtypeOpen,
                Token::Ident("ref".into()),
                Token::Long(0),
                Token::SubtypeClose,
            ]
            .into(),
            0,
            0,
        ));
    }
    let table = super::super::super::SubtypeTable::from_records(
        &cadmpeg_test_support::service_decode_context(),
        &records,
    )
    .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 128 * u64::try_from(2 * count).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    super::super::super::admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}
