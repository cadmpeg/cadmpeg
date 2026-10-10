// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::sab::{Record, Token};
use super::super::SubtypeTable;

fn records() -> [Record; 3] {
    std::array::from_fn(|index| crate::test_support::sab::record(
index,
"spline".into(),
vec![Token::SubtypeOpen, Token::Ident("exactcur".into()), Token::SubtypeClose].into(),
0,
0
))
}

fn source_boundary(cap: u64, operation: &'static str, dimension: ResourceDimension) {
    let records = records();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let first = match SubtypeTable::from_records(&ctx, &records) {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected actual subtype source/allocation refusal"),
    };
    assert_eq!(first.dimension, dimension);
    assert_eq!(first.operation, operation);
    assert_eq!(first.additional, 1);
    if dimension == ResourceDimension::WorkUnits {
        assert_eq!((first.limit, first.used), (cap, cap));
    } else {
        assert_eq!((first.limit, first.used), (0, 0));
    }
    for replay in [&records[..], &[][..]] {
        assert!(matches!(SubtypeTable::from_records(&ctx, replay),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn subtype_record_index_refuses_one_visit_before_the_record_tail() {
    source_boundary(0, "index ASM subtype records", ResourceDimension::WorkUnits);
}

#[test]
fn subtype_token_index_refuses_one_visit_after_the_record_step() {
    source_boundary(1, "index ASM subtype tokens", ResourceDimension::WorkUnits);
}

#[test]
fn subtype_definition_refuses_after_only_its_open_token_is_visited() {
    source_boundary(2, "index ASM subtype definitions", ResourceDimension::CollectionItems);
}

#[test]
fn empty_subtype_table_executes_no_source_steps() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(SubtypeTable::from_records(&ctx, &[]).unwrap().defs.is_empty());
    ctx.finish_session().unwrap();
}

#[test]
fn manual_subtype_reference_walk_does_not_scan_exhausted_stack_frames() {
    let table = SubtypeTable::from_records(&cadmpeg_test_support::service_decode_context(), &[]).unwrap();
    for count in [0_usize, 1, 64] {
        let record = crate::test_support::sab::record(
0,
"x".into(),
vec![Token::False; count].into(),
0,
0
);
        let required = u64::try_from(1 + count).unwrap();
        for cap in [1, required - 1, required] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = super::super::admit_subtype_references(&ctx, std::slice::from_ref(&record), &table);
            if cap == required {
                result.unwrap();
                assert_eq!(record.tokens.len(), count);
                assert!(record.tokens.iter().all(|token| *token == Token::False));
                ctx.finish_session().unwrap();
            } else {
                let Err(CodecError::ResourceLimit(first)) = result else { panic!("actual record or token refusal"); };
                assert_eq!(first.operation, if cap == 0 { "walk ASM subtype records" } else { "scan ASM subtype references" });
                assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
                for records in [std::slice::from_ref(&record), &[]] {
                    assert!(matches!(super::super::admit_subtype_references(&ctx, records, &table),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                }
                assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            }
        }
    }
}

mod shared_definitions;
