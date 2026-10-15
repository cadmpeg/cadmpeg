// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::io::Cursor;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::directory::{DirectoryEntry, SourceStatus};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use crate::test_support::test_owned::{owned_test_file_with_directory_fields, OwnedTestEntity};
use crate::IgesCodec;

fn associativity_at_work_cap(
    form: i64,
    values: &[TokenValue],
    parameter_end: usize,
    status: [u8; 4],
    cap: u64,
) -> Result<bool, cadmpeg_core::CodecError> {
    let tokens = values
        .iter()
        .cloned()
        .map(|value| Token { value, span: 0..0 })
        .collect();
    let record =
        ParameterRecord::from_test_tokens(3, 0..0, Vec::new(), parameter_end, tokens, Vec::new());
    let entry = DirectoryEntry {
        source_offset: 0,
        sequence: 3,
        entity_type: 402,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: SourceStatus::from_codes(status),
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form,
        reserved: [[0; 8]; 2],
        label: [0; 8],
        subscript: 0,
    };
    let target = DirectoryEntry {
        sequence: 1,
        entity_type: 202,
        ..entry
    };
    let entries = [(1, &target)].into_iter().collect();
    let records = std::collections::BTreeMap::new();
    let association_owners = std::collections::BTreeMap::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    let result = super::super::predefined_associativity_valid(
        &entry,
        &record,
        &entries,
        &records,
        &association_owners,
        &ctx,
    );
    if let Err(cadmpeg_core::CodecError::ResourceLimit(first)) = &result {
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(last)) if last == *first)
        );
    } else {
        ctx.finish_session().unwrap();
    }
    result
}

fn assert_associativity_rejects_without_pointer_lookup(
    form: i64,
    values: &[TokenValue],
    parameter_end: usize,
    status: [u8; 4],
) {
    assert!(!associativity_at_work_cap(form, values, parameter_end, status, 0).unwrap());
}

fn assert_associativity_visits_before_rejection(
    form: i64,
    values: &[TokenValue],
    parameter_end: usize,
    status: [u8; 4],
    operation: &'static str,
    additional: u64,
) {
    assert!(!associativity_at_work_cap(form, values, parameter_end, status, u64::MAX).unwrap());
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        |cap| associativity_at_work_cap(form, values, parameter_end, status, cap),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && limit.operation == operation && limit.additional == additional)
    );
}

fn integer_values(values: &[i64]) -> Vec<TokenValue> {
    values.iter().copied().map(TokenValue::Integer).collect()
}

#[test]
fn invalid_form_13_header_skips_dimension_pointer_lookup() {
    assert_associativity_rejects_without_pointer_lookup(
        13,
        &integer_values(&[99, 0, 1, 1]),
        4,
        [0, 0, 0, 0],
    );
}

#[test]
fn form_6_invalid_header_visits_view_before_rejection() {
    assert_associativity_visits_before_rejection(
        6,
        &integer_values(&[99, 99, 1, 1, 1]),
        5,
        [0, 0, 0, 0],
        "iges structure pointer lookup",
        4,
    );
}

#[test]
fn form_6_invalid_count_skips_view_lookup() {
    assert_associativity_rejects_without_pointer_lookup(
        6,
        &integer_values(&[99, 1, -1, 1]),
        4,
        [0, 0, 0, 0],
    );
}

#[test]
fn form_6_invalid_end_visits_view_before_rejection() {
    assert_associativity_visits_before_rejection(
        6,
        &integer_values(&[99, 1, 1, 1, 1, 0]),
        6,
        [0, 0, 0, 0],
        "iges structure pointer lookup",
        4,
    );
}

#[test]
fn form_6_zero_visible_count_remains_valid() {
    let record = ParameterRecord::from_test_tokens(
        3,
        0..0,
        Vec::new(),
        4,
        integer_values(&[99, 1, 0, 1])
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect(),
        Vec::new(),
    );
    let entry = DirectoryEntry {
        source_offset: 0,
        sequence: 3,
        entity_type: 402,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: SourceStatus::from_codes([0, 0, 0, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form: 6,
        reserved: [[0; 8]; 2],
        label: [0; 8],
        subscript: 0,
    };
    let view_entry = DirectoryEntry {
        sequence: 1,
        entity_type: 410,
        ..entry
    };
    let view_record =
        ParameterRecord::from_test_tokens(1, 0..0, Vec::new(), 0, Vec::new(), Vec::new());
    let entries = [(1, &view_entry)].into_iter().collect();
    let records = [(1, &view_record)].into_iter().collect();
    let association_owners = [(3, [1].into_iter().collect())].into_iter().collect();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();

    assert!(super::super::predefined_associativity_valid(
        &entry,
        &record,
        &entries,
        &records,
        &association_owners,
        &ctx,
    )
    .unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn form_9_invalid_header_visits_member_before_rejection() {
    assert_associativity_visits_before_rejection(
        9,
        &integer_values(&[99, 99, 1, 1, 1]),
        5,
        [0, 0, 0, 0],
        "iges predefined associativity fields",
        1,
    );
}

#[test]
fn form_9_zero_child_count_skips_member_lookups() {
    assert_associativity_rejects_without_pointer_lookup(
        9,
        &integer_values(&[99, 1, 0, 1]),
        4,
        [0, 0, 0, 0],
    );
}

#[test]
fn form_9_invalid_end_visits_member_before_rejection() {
    assert_associativity_visits_before_rejection(
        9,
        &integer_values(&[99, 1, 1, 1]),
        4,
        [0, 0, 0, 0],
        "iges predefined associativity fields",
        1,
    );
}

#[test]
fn form_13_zero_geometry_count_skips_dimension_lookup() {
    assert_associativity_rejects_without_pointer_lookup(
        13,
        &integer_values(&[99, 1, 0, 1]),
        4,
        [0, 0, 0, 0],
    );
}

#[test]
fn form_13_invalid_end_skips_dimension_lookup() {
    assert_associativity_rejects_without_pointer_lookup(
        13,
        &integer_values(&[99, 1, 1, 1]),
        4,
        [0, 0, 0, 0],
    );
}

#[test]
fn form_16_invalid_header_skips_transform_lookup() {
    assert_associativity_rejects_without_pointer_lookup(
        16,
        &integer_values(&[99, 99, 1, 1, 1]),
        5,
        [0, 0, 0, 0],
    );
}

#[test]
fn form_16_zero_member_count_skips_transform_lookup() {
    assert_associativity_rejects_without_pointer_lookup(
        16,
        &integer_values(&[99, 1, 0, 1, 1]),
        5,
        [0, 0, 0, 0],
    );
}

#[test]
fn form_16_invalid_end_skips_transform_and_member_lookups() {
    assert_associativity_rejects_without_pointer_lookup(
        16,
        &integer_values(&[99, 1, 1, 1, 1, 0]),
        6,
        [0, 0, 0, 0],
    );
}

fn valid_form_21_values() -> Vec<TokenValue> {
    vec![
        TokenValue::Integer(99),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(1),
        TokenValue::Integer(0),
        TokenValue::real(0.0),
        TokenValue::Integer(0),
        TokenValue::Integer(0),
        TokenValue::real(0.0),
        TokenValue::real(0.0),
        TokenValue::real(0.0),
    ]
}

#[test]
fn form_21_invalid_header_skips_dimension_lookup() {
    let mut values = valid_form_21_values();
    values[1] = TokenValue::Integer(99);
    assert_associativity_rejects_without_pointer_lookup(21, &values, 11, [0, 1, 0, 0]);
}

#[test]
fn form_21_zero_geometry_count_skips_dimension_lookup() {
    let mut values = valid_form_21_values();
    values[2] = TokenValue::Integer(0);
    values.truncate(6);
    assert_associativity_rejects_without_pointer_lookup(21, &values, 6, [0, 1, 0, 0]);
}

#[test]
fn form_21_invalid_end_skips_dimension_lookup() {
    let mut values = valid_form_21_values();
    values.push(TokenValue::Integer(0));
    assert_associativity_rejects_without_pointer_lookup(21, &values, 12, [0, 1, 0, 0]);
}

#[test]
fn form_21_invalid_orientation_scalar_skips_dimension_lookup() {
    let mut values = valid_form_21_values();
    values[4] = TokenValue::Integer(8);
    assert_associativity_rejects_without_pointer_lookup(21, &values, 11, [0, 1, 0, 0]);
}

#[test]
fn form_21_non_numeric_angle_skips_dimension_lookup() {
    let mut values = valid_form_21_values();
    values[5] = TokenValue::String(Vec::new());
    assert_associativity_rejects_without_pointer_lookup(21, &values, 11, [0, 1, 0, 0]);
}

#[test]
fn form_21_independent_status_skips_dimension_lookup() {
    assert_associativity_rejects_without_pointer_lookup(
        21,
        &valid_form_21_values(),
        11,
        [0, 0, 0, 0],
    );
}

#[test]
fn associativity_definition_ignores_unrelated_directory_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_directory_fields(
                &[OwnedTestEntity {
                    entity_type: 302,
                    form: 5001,
                    label: "DEFIN".into(),
                    status: "00000200",
                    parameters: "302,1,1,1,1,1;".into(),
                }],
                &[(1, 9)],
                &[(1, 7)],
                &[(1, 4)],
                &[(1, 3)],
                &[(1, 8)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind()
            && loss.message.contains("associativity definition")
    }));
}

#[test]
fn units_data_ignores_unrelated_directory_fields() {
    let result = IgesCodec
        .decode(
            &mut Cursor::new(owned_test_file_with_directory_fields(
                &[OwnedTestEntity {
                    entity_type: 316,
                    form: 0,
                    label: "UNITS".into(),
                    status: "00000200",
                    parameters: "316,1,6HLENGTH,2HKN,1852;".into(),
                }],
                &[(1, 9)],
                &[(1, 7)],
                &[(1, 4)],
                &[(1, 3)],
                &[(1, 8)],
            )),
            &DecodeOptions::default(),
        )
        .unwrap();

    assert!(!result.report().losses.iter().any(|loss| {
        loss.code == IgesLossCode::EntityNotProjected.kind() && loss.message.contains("units")
    }));
}
