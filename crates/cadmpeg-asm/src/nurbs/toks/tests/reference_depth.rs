// SPDX-License-Identifier: Apache-2.0

use crate::nurbs::toks::{admit_subtype_references, SubtypeTable};
use crate::sab::{Record, Token};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn definition(index: usize, target: Option<usize>) -> Record<'static> {
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("node".into())];
    if let Some(target) = target {
        tokens.extend([
            Token::SubtypeOpen,
            Token::Ident("ref".into()),
            Token::Long(i64::try_from(target).unwrap()),
            Token::SubtypeClose,
        ]);
    }
    tokens.push(Token::SubtypeClose);
    crate::test_support::sab::record(index, "node".into(), tokens.into(), 0, 0)
        .expect("indexed record fixture")
}

#[test]
fn leaf_first_subtype_chain_preserves_admitted_reference_depth() {
    let records: Vec<_> = (0..64)
        .map(|index| definition(index, index.checked_sub(1)))
        .collect();
    let table =
        SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &records)
            .unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RecursionDepth,
        "follow admitted ASM subtype reference",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            admit_subtype_references(&ctx, &records, &table)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("reference depth refusal");
    };
    assert_eq!(limit.operation, "follow admitted ASM subtype reference");
    assert_eq!((limit.limit, limit.used, limit.additional), (1, 1, 1));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn shared_subtype_definitions_stay_within_the_default_work_budget() {
    let records: Vec<_> = (0..4096)
        .map(|index| definition(index, (index != 0).then_some(0)))
        .collect();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let table = SubtypeTable::from_records(&ctx, &records).unwrap();
    admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn cyclic_subtype_paths_preserve_depth_after_a_cached_rotation() {
    // The cycle has three definitions. The last record adds a reference to
    // a definition that enters the cycle through a different member.
    let records: Vec<_> = [1, 2, 0, 2, 3]
        .into_iter()
        .enumerate()
        .map(|(index, target)| definition(index, Some(target)))
        .collect();
    let table =
        SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &records)
            .unwrap();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RecursionDepth,
        "follow cyclic ASM subtype reference",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_recursion_depth = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            admit_subtype_references(&ctx, &records, &table)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("cyclic path depth refusal");
    };
    assert_eq!(limit.operation, "follow cyclic ASM subtype reference");
    assert_eq!((limit.limit, limit.used, limit.additional), (3, 3, 1));
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn cyclic_subtype_paths_stop_at_the_current_path() {
    let records: Vec<_> = [1, 2, 0]
        .into_iter()
        .enumerate()
        .map(|(index, target)| definition(index, Some(target)))
        .collect();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let table = SubtypeTable::from_records(&ctx, &records).unwrap();
    admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}

#[test]
fn cyclic_subtype_walk_scans_each_definition_payload_once() {
    let count = 4096;
    let mut tokens = vec![Token::SubtypeOpen, Token::Ident("node".into())];
    tokens.extend((0..count).map(|_| Token::False));
    tokens.extend([
        Token::SubtypeOpen,
        Token::Ident("ref".into()),
        Token::Long(0),
        Token::SubtypeClose,
        Token::SubtypeClose,
    ]);
    let mut records = vec![
        crate::test_support::sab::record(0, "node".into(), tokens.into(), 0, 0)
            .expect("indexed record fixture"),
    ];
    for index in 1..=count {
        records.push(
            crate::test_support::sab::record(
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
            )
            .expect("indexed record fixture"),
        );
    }
    let table =
        SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &records)
            .unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 128 * u64::try_from(2 * count).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    admit_subtype_references(&ctx, &records, &table).unwrap();
    ctx.finish_session().unwrap();
}
