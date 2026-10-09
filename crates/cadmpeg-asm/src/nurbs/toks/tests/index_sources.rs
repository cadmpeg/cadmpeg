// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::sab::{Record, Token};
use super::super::SubtypeTable;

fn records() -> [Record; 3] {
    std::array::from_fn(|index| Record {
        index,
        name: "spline".into(),
        tokens: vec![Token::SubtypeOpen, Token::Ident("exactcur".into()), Token::SubtypeClose].into(),
        offset: 0,
        len: 0,
    })
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
