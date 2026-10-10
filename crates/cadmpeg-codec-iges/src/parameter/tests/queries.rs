// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::global::GlobalTable;
use crate::parameter::{PointerGroupCandidate, TrailingPointerAnalysis};
use crate::test_support::directory_target;

use super::integer_parameter_record;

#[test]
fn macro_analysis_resolves_its_directory_entry_once() {
    let entry = directory_target(1, 306);
    let directory = BTreeMap::from([(1, &entry)]);
    let record = integer_parameter_record(1, &[306]);
    for with_records in [false, true] {
        for work in [3, 4] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = if with_records {
                crate::parameter::analyze_trailing_pointer_groups_with_records_for_global_table(
                    &record,
                    &directory,
                    &BTreeMap::new(),
                    GlobalTable::V5Later,
                    &mut crate::parameter::AttributeDefinitionWidths::new(&ctx).unwrap(),
                    &ctx,
                )
            } else {
                crate::parameter::analyze_trailing_pointer_groups_for_global_table_with_context(
                    &record,
                    &directory,
                    GlobalTable::V5Later,
                    &ctx,
                )
            };
            if work == 4 {
                assert!(matches!(result.unwrap(), TrailingPointerAnalysis::Macro));
                ctx.finish_session().unwrap();
            } else {
                let CodecError::ResourceLimit(first) = result.unwrap_err() else {
                    panic!("expected Directory query refusal");
                };
                assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                assert_eq!(
                    first.operation,
                    "iges parameter primary layout directory lookup"
                );
                assert_eq!(first.used, 0);
                // One stored u32 key bounds this query to one four-byte comparison.
                assert_eq!(first.additional, 4);
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(actual)) if actual == first
                ));
            }
        }
    }
}

#[test]
fn attribute_primary_layout_admits_definition_queries_after_type_gates() {
    let mut instance = directory_target(1, 422);
    instance.structure = -3;
    let definition = directory_target(3, 322);
    let directory = BTreeMap::from([(1, &instance), (3, &definition)]);
    let record = integer_parameter_record(1, &[422, 12]);
    let definition_record = integer_parameter_record(3, &[322, 0, 0, 1, 1, 1, 1]);
    let records = BTreeMap::from([(3, &definition_record)]);
    for work in [16, 21] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::parameter::entity_primary_end_with_records_for_global_table(
            &record,
            &directory,
            &records,
            GlobalTable::V5Later,
            &ctx,
        );
        if work == 21 {
            // Two two-key Directory queries (8 each), one one-key record query
            // (4), and one attribute visit (1).
            assert_eq!(result.unwrap(), Some(2));
            ctx.finish_session().unwrap();
        } else {
            let CodecError::ResourceLimit(first) = result.unwrap_err() else {
                panic!("expected definition record query refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "iges attribute definition record lookup");
            assert_eq!(first.used, 16);
            assert_eq!(first.additional, 4);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == first
            ));
        }
    }

    let wrong_definition = directory_target(3, 314);
    let directory = BTreeMap::from([(1, &instance), (3, &wrong_definition)]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 16;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        crate::parameter::entity_primary_end_with_records_for_global_table(
            &record,
            &directory,
            &records,
            GlobalTable::V5Later,
            &ctx,
        )
        .unwrap(),
        Some(record.tokens().len())
    );
    ctx.finish_session().unwrap();
}

#[test]
fn first_trailing_pointer_query_refusal_does_not_previsit_copy_tail() {
    const POINTERS: usize = 128;
    let target = directory_target(1, 212);
    let directory = BTreeMap::from([(1, &target)]);
    let mut values = vec![999, i64::try_from(POINTERS).unwrap()];
    values.extend(std::iter::repeat_n(1, POINTERS));
    values.push(0);
    let record = integer_parameter_record(3, &values);
    let candidate = PointerGroupCandidate {
        token_start: 1,
        association_start: 2,
        property_count_index: 2 + POINTERS,
        property_count: 0,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Integer validation must finish first. Copying then visits only pointer 1.
    policy.limits.max_work_units = u64::try_from(POINTERS + 1 + 1).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error =
        crate::parameter::groups_for_candidate_with_context(&record, &directory, candidate, &ctx)
            .unwrap_err();
    let CodecError::ResourceLimit(first) = error else {
        panic!("expected first pointer query refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges trailing pointer directory lookup");
    assert_eq!(first.used, u64::try_from(POINTERS + 2).unwrap());
    assert_eq!(first.additional, 4);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == first
    ));
}

#[test]
fn invalid_first_real_byte_stops_before_numeric_tail() {
    let mut text = vec![b'x'];
    text.extend(std::iter::repeat_n(b'0', 512));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(crate::parameter::decimal_shape(&text, &ctx).unwrap(), None);
    ctx.finish_session().unwrap();
}

#[test]
fn valid_real_shape_admits_each_actual_byte() {
    for work in 0..=4 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = crate::parameter::decimal_shape(b"1E2", &ctx);
        if work >= 3 {
            assert_eq!(
                result.unwrap(),
                Some(crate::parameter::DecimalShape {
                    magnitude: crate::parameter::Magnitude::Order(2),
                    double_precision: false,
                })
            );
            ctx.finish_session().unwrap();
        } else {
            let CodecError::ResourceLimit(first) = result.unwrap_err() else {
                panic!("expected byte visit refusal");
            };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "iges numeric real shape");
            assert_eq!(first.used, work);
            assert_eq!(first.additional, 1);
            assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(actual)) if actual == first
            ));
        }
    }
}
