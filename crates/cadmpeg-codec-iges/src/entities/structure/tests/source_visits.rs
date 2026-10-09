// SPDX-License-Identifier: Apache-2.0

use super::super::attribute_definition_valid_and_shape;
use crate::directory::{DirectoryEntry, SourceStatus};
use crate::global::GlobalTable;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn entry(form: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0, sequence: 1, entity_type: 322, parameter_start: 0,
        structure: 0, line_font: 0, level: 0, view: 0, transform: 0, label_display: 0,
        status: SourceStatus::from_codes([0, 0, 0, 0]), line_weight: 0, color: 0,
        parameter_line_count: 0, form, reserved: [[b' '; 8]; 2], label: [b' '; 8], subscript: 0,
    }
}

fn record(count: i64, fields: &[i64]) -> ParameterRecord {
    let mut values = vec![TokenValue::Integer(322), TokenValue::Omitted,
        TokenValue::Integer(1), TokenValue::Integer(count)];
    values.extend(fields.iter().copied().map(TokenValue::Integer));
    let parameter_end = values.len();
    let tokens = values.into_iter().map(|value| Token { value, span: 0..0 }).collect();
    ParameterRecord::from_test_tokens(1, 0..0, Vec::new(), parameter_end, tokens, Vec::new())
}

fn source_refusal(input: &ParameterRecord, form: i64, work: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let directory = entry(form);
    let entries = BTreeMap::new();
    let Err(CodecError::ResourceLimit(first)) = attribute_definition_valid_and_shape(
        &directory, input, &entries, GlobalTable::V5Later, &ctx,
    ) else {
        panic!("expected attribute definition source refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, "iges structure list traversal");
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    for replay in [input, &record(0, &[])] {
        assert!(matches!(attribute_definition_valid_and_shape(
            &directory, replay, &entries, GlobalTable::V5Later, &ctx,
        ), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn attribute_descriptor_source_refuses_one_visit_before_any_type_or_shape_allocation() {
    source_refusal(&record(3, &[-1, 7, 0, -1, 7, 0, -1, 7, 0]), 0, 0);
}

#[test]
fn attribute_value_source_refuses_one_visit_after_one_descriptor_without_admitting_the_tail() {
    // The invalid type prevents type-index storage; the three integer values
    // still belong to the supplied form1 descriptor and are checked in order.
    source_refusal(&record(1, &[-1, 1, 3, 7, 11, 19]), 1, 1);
}

#[test]
fn attribute_shape_allocation_refuses_after_one_descriptor_without_admitting_the_tail() {
    let input = record(3, &[-1, 1, 1, -1, 1, 1, -1, 1, 1]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let directory = entry(0);
    let entries = BTreeMap::new();
    let Err(CodecError::ResourceLimit(first)) = attribute_definition_valid_and_shape(
        &directory, &input, &entries, GlobalTable::V5Later, &ctx,
    ) else {
        panic!("expected the first actual attribute descriptor allocation to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.operation, "iges attribute shape descriptors");
    assert_eq!((first.limit, first.used, first.additional), (0, 0, 1));
    for replay in [&input, &record(0, &[])] {
        assert!(matches!(attribute_definition_valid_and_shape(
            &directory, replay, &entries, GlobalTable::V5Later, &ctx,
        ), Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

fn empty_shape_acceptance(input: &ParameterRecord, form: i64, work: u64) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (valid, shape) = attribute_definition_valid_and_shape(
        &entry(form), input, &BTreeMap::new(), GlobalTable::V5Later, &ctx,
    ).unwrap();
    assert!(!valid);
    assert!(shape.descriptors.is_empty());
    drop(shape);
    ctx.finish_session().unwrap();
}

#[test]
fn attribute_descriptor_source_visits_all_rejected_descriptors_and_no_empty_end_probe() {
    empty_shape_acceptance(&record(3, &[-1, 7, 0, -1, 7, 0, -1, 7, 0]), 0, 3);
    empty_shape_acceptance(&record(0, &[]), 0, 0);
}

#[test]
fn attribute_value_source_visits_exactly_three_values_and_preserves_rejection() {
    empty_shape_acceptance(&record(1, &[-1, 1, 3, 7, 11, 19]), 1, 1 + 3);
    empty_shape_acceptance(&record(0, &[]), 1, 0);
}
