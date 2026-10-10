// SPDX-License-Identifier: Apache-2.0

use crate::nurbs::toks::{admit_subtype_references, SubtypeTable};
use crate::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn definition(index: usize, target: Option<usize>) -> Record {
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("node".into())];
    if let Some(target) = target {
        tokens.extend([Token::SubtypeOpen, Token::Ident("ref".into()),
            Token::Long(i64::try_from(target).unwrap()), Token::SubtypeClose]);
    }
    tokens.push(Token::SubtypeClose);
    crate::test_support::sab::record(index, "node".into(), tokens.into(), 0, 0)
}

#[test]
fn leaf_first_subtype_chain_preserves_admitted_reference_depth() {
    let records: Vec<_> = (0..64).map(|index| definition(index, index.checked_sub(1))).collect();
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &records).unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RecursionDepth, "follow admitted ASM subtype reference", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            admit_subtype_references(&ctx, &records, &table)
        });
    let CodecError::ResourceLimit(limit) = error else { panic!("reference depth refusal"); };
    assert_eq!(limit.operation, "follow admitted ASM subtype reference");
    assert_eq!((limit.limit, limit.used, limit.additional), (1, 1, 1));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn shared_subtype_definitions_stay_within_the_default_work_budget() {
    let records: Vec<_> = (0..4096).map(|index| definition(index, (index != 0).then_some(0))).collect();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let table = SubtypeTable::from_records(&ctx, &records).unwrap();
    admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}
