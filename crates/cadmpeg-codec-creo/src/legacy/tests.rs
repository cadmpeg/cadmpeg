// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use cadmpeg_test_support::wire;

use crate::test_support::build_prt;
use crate::test_support::build_prt_raw;
use crate::test_support::visibgeom_payload;
use std::io::Cursor;
use std::ops::Range;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::container::{self, Layout, UnknownLayout};
use crate::loss::CreoLossCode;
use crate::CreoCodec;

use super::type_code::LegacyTypeCode;
use super::{
    object_node_id, parse_declaration, IntegerPayload, IntegerRun, NumericPayload, NumericRun,
    ObjectPayload, PrincipalUnitSystem, Real, RealPayload, RealRun, StringPayload, StringValue,
    UnsignedPayload, ValueKind,
};

#[test]
fn serialized_offset_ids_preserve_legacy_wire_text() {
    for (id, expected) in [
        (
            super::serialized_object_node_id(123),
            "\"creo:legacy_ascii:object#123\"",
        ),
        (
            super::SerializedOffsetId {
                namespace: "legacy_ascii",
                kind: "integer",
                offset: 123,
            },
            "\"creo:legacy_ascii:integer#123\"",
        ),
        (
            super::SerializedOffsetId {
                namespace: "legacy_family",
                kind: "driver_table",
                offset: 123,
            },
            "\"creo:legacy_family:driver_table#123\"",
        ),
    ] {
        assert_eq!(
            serde_json::to_string(&id).expect("serialize offset identity"),
            expected
        );
    }
}

#[test]
fn legacy_scope_bounds_error_refuses_retained_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = super::scan_scope(&ctx, &[0], 0..2).expect_err("scope end exceeds source length");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo legacy scope bounds error")
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        let error =
            super::scan_scope(ctx, &[0], 0..2).expect_err("scope end exceeds source length");
        assert!(error.to_string().contains("past the file length"));
        Ok::<(), cadmpeg_core::CodecError>(())
    })
    .expect("service error text admitted");
}

mod numeric_admission;
mod string_admission;

fn principal_unit_system(persistence: &super::Persistence) -> Option<PrincipalUnitSystem> {
    crate::decode::with_test_decode_ctx(|ctx| persistence.principal_unit_system(ctx))
        .expect("unit selection fits service limits")
}

fn scan(
    data: &[u8],
    ranges: impl IntoIterator<Item = Range<usize>>,
) -> Result<super::Persistence, cadmpeg_core::CodecError> {
    crate::decode::with_test_decode_ctx(|ctx| super::scan(ctx, data, ranges))
}

fn assert_scope_collection_refusal(data: &[u8], limit: u64, operation: &'static str) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::scan(&ctx, data, std::iter::once(0..data.len()))
        .expect_err("the next collection item exceeds the limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation)
    );
}

#[test]
fn legacy_scope_vec_refuses_before_first_scope() {
    assert_scope_collection_refusal(b"@size 1 1\n0 1 9\n", 0, "creo legacy parsed scopes");
}

#[test]
fn legacy_declaration_index_refuses_before_new_node() {
    assert_scope_collection_refusal(
        b"@size 1 1\n0 1 9\n",
        1,
        "creo legacy declaration index nodes",
    );
}

#[test]
fn legacy_declaration_vec_refuses_before_row() {
    assert_scope_collection_refusal(b"@size 1 1\n0 1 9\n", 2, "creo legacy declarations");
}

#[test]
fn legacy_scope_candidate_vec_refuses_before_value_row() {
    assert_scope_collection_refusal(
        b"@size 1 1\n0 1 9\n",
        3,
        "creo legacy scope value candidates",
    );
}

#[test]
fn legacy_conflicting_id_set_refuses_before_new_node() {
    assert_scope_collection_refusal(
        b"@size 1 1\n@size 1 2\n",
        3,
        "creo legacy conflicting declaration IDs",
    );
}

#[test]
fn legacy_declaration_name_refuses_before_retained_copy() {
    let data = b"@size 1 1\n0 1 9\n";
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        "creo legacy declaration names",
        |ctx| super::scan(ctx, data, std::iter::once(0..data.len())),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo legacy declaration names")
    );
}

fn assert_parent_lookup_refusal(limit: u64, operation: &'static str) {
    let data = b"@root 1 0\n@child 2 0\n0 1 ->\n1 2 ->\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::parent_object_offsets(&ctx, &persistence.scopes)
        .expect_err("the next lookup node exceeds the limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation)
    );
}

#[test]
fn legacy_declaration_lookup_refuses_before_btree_node() {
    assert_parent_lookup_refusal(0, "creo legacy declaration lookup nodes");
}

#[test]
fn legacy_active_object_refuses_before_btree_node() {
    assert_parent_lookup_refusal(2, "creo legacy active object nodes");
}

#[test]
fn legacy_parent_offset_refuses_before_btree_node() {
    assert_parent_lookup_refusal(3, "creo legacy parent offset nodes");
}

#[test]
fn legacy_array_dimension_refuses_before_vec_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::array_dimensions(&ctx, b"[2][3]")
        .expect_err("one dimension exceeds the collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo legacy array dimensions")
    );
}

#[test]
fn legacy_continuation_run_refuses_before_vec_growth() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::continuation_numeric_runs(&ctx, b"$1,2", super::signed_integer)
        .expect_err("one run exceeds the collection limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo legacy continuation numeric runs")
    );
}

fn object_fixture_parts(
    data: &[u8],
) -> (super::Persistence, std::collections::BTreeMap<usize, usize>) {
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let parents = crate::decode::with_test_decode_ctx(|ctx| {
        super::parent_object_offsets(ctx, &persistence.scopes)
    })
    .expect("parent lookup fits service limits");
    (persistence, parents)
}

fn assert_object_collection_refusal(data: &[u8], limit: u64, operation: &'static str) {
    let (persistence, parents) = object_fixture_parts(data);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::object_records(&ctx, data, &persistence.scopes, &parents)
        .expect_err("the next object item exceeds the limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation)
    );
}

#[test]
fn legacy_object_value_attribute_index_refuses_before_node() {
    assert_object_collection_refusal(
        b"@root 1 0\n0 1 ->\n",
        1,
        "creo legacy object value attribute nodes",
    );
}

#[test]
fn legacy_object_array_index_refuses_before_node() {
    assert_object_collection_refusal(
        b"@arr 1 0\n0 1 [1]\n1 1 ->\n",
        3,
        "creo legacy object array index nodes",
    );
}

#[test]
fn legacy_object_array_index_rows_refuse_before_growth() {
    assert_object_collection_refusal(
        b"@arr 1 0\n0 1 [1]\n1 1 ->\n",
        4,
        "creo legacy object array index rows",
    );
}

#[test]
fn legacy_object_array_elements_refuse_before_growth() {
    assert_object_collection_refusal(
        b"@arr 1 0\n0 1 [1]\n1 1 ->\n",
        6,
        "creo legacy object array elements",
    );
}

#[test]
fn legacy_object_records_refuse_before_growth() {
    assert_object_collection_refusal(b"@root 1 0\n0 1 ->\n", 2, "creo legacy object records");
}

fn assert_object_retained_refusal(data: &[u8], operation: &'static str) {
    let (persistence, parents) = object_fixture_parts(data);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some(operation),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            super::object_records(&trial_ctx, data, &persistence.scopes, &parents)
        },
    );
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let error = super::object_records(&ctx, data, &persistence.scopes, &parents)
        .expect_err("object output needs retained bytes");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == operation)
    );
}

#[test]
fn legacy_object_array_id_refuses_before_string_growth() {
    assert_object_retained_refusal(
        b"@arr 1 0\n0 1 [1]\n1 1 ->\n",
        "creo legacy object array element IDs",
    );
}

#[test]
fn legacy_opaque_object_refuses_before_byte_copy() {
    assert_object_retained_refusal(
        b"@root 1 0\n0 1 unknown\n",
        "creo legacy opaque object bytes",
    );
}

#[test]
fn legacy_object_record_name_refuses_before_string_copy() {
    assert_object_retained_refusal(b"@root 1 0\n0 1 ->\n", "creo legacy object record names");
}

mod identity;

mod values;

#[test]
fn scan_withholds_ambiguous_and_undeclared_values() {
    let data = b"#P_OBJECT 6\n$orphan\n@field 7 1\n@other 7 2\n1 7 4\n2 99 5\n";

    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let scope = &persistence.scopes[0];

    assert!(scope.values.is_empty());
    assert_eq!(scope.declarations.len(), 1);
    assert_eq!(persistence.conflicting_declaration_count(), 1);
    assert_eq!(persistence.unresolved_value_count(), 2);
}

mod codec;

#[test]
fn typed_value_results_keep_grammar_and_unresolved_counts_together() {
    let data = b"@nullable 1 3\n@bytes 2 4\n@unsigned 3 5\n@real 4 6\n\
        0 1 NULL\n0 1 text\n$continued\n0 1 other\n$continued\n\
        0 2 NULL\n0 3 7\n0 3 -1\n0 4 3FF\n";
    let persistence = scan(data, std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    assert_eq!(persistence.type_3_values.rows[0].payload, StringValue::Null);
    assert_eq!(persistence.type_3_values.unresolved_count, 2);
    assert_eq!(
        persistence.type_4_values.rows[0].payload,
        StringValue::Utf8 {
            text: "NULL".to_owned()
        }
    );
    assert_eq!(persistence.type_4_values.unresolved_count, 0);
    assert_eq!(
        persistence.type_5_values.rows[0].payload,
        NumericPayload::Scalar { value: 7 }
    );
    assert_eq!(persistence.type_5_values.unresolved_count, 1);
    assert_eq!(
        persistence.type_6_values.rows[0].payload,
        NumericPayload::Scalar {
            value: Real::from_bits(1.0f64.to_bits())
        }
    );
    assert_eq!(persistence.type_6_values.unresolved_count, 0);
}

#[test]
fn value_kind_types_are_distinct_and_preserve_identity_tokens() {
    let token = |code: crate::legacy::type_code::LegacyTypeCode| code.identity_token();
    assert_eq!(
        token(super::declaration_code(ValueKind::INTEGER)),
        "integer"
    );
    assert_eq!(token(super::declaration_code(ValueKind::REAL)), "real");
    assert_eq!(token(super::declaration_code(ValueKind::TYPE3)), "type_3");
    assert_eq!(token(super::declaration_code(ValueKind::TYPE4)), "type_4");
    assert_eq!(token(super::declaration_code(ValueKind::TYPE5)), "type_5");
    assert_eq!(token(super::declaration_code(ValueKind::TYPE6)), "type_6");
    assert_eq!(token(super::declaration_code(ValueKind::TYPE7)), "type_7");
    assert_eq!(token(super::declaration_code(ValueKind::TYPE9)), "type_9");
    assert_eq!(token(super::declaration_code(ValueKind::TYPE11)), "type_11");
}

#[test]
fn numeric_array_withholds_child_at_maximum_depth() {
    let data = b"@value 1 1\n4294967295 1 [1]\n4294967295 1 7\n";
    let persistence =
        scan(data, std::iter::once(0..data.len())).expect("maximum-depth fixture is admitted");
    assert_eq!(persistence.integer_values.rows.len(), 1);
    assert!(matches!(
        persistence.integer_values.rows[0].payload,
        IntegerPayload::Scalar { value: 7 }
    ));
    assert_eq!(persistence.integer_values.unresolved_count, 1);
}

#[test]
fn legacy_declaration_id_parse_refuses_before_invalid_text() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo scalar text parsing",
        |ctx| super::parse_declaration(ctx, b"@name x 1"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo scalar text parsing")
    );
}

#[test]
fn legacy_declaration_type_parse_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo scalar text parsing",
        |ctx| super::parse_declaration(ctx, b"@name 1 123"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo scalar text parsing")
    );
}

#[test]
fn legacy_signed_integer_parse_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo scalar text parsing",
        |ctx| super::signed_integer(ctx, b"-2147483648"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo scalar text parsing")
    );
}

#[test]
fn legacy_unsigned_integer_parse_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo scalar text parsing",
        |ctx| super::unsigned_integer(ctx, b"4294967295"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo scalar text parsing")
    );
}

#[test]
fn legacy_compact_real_radix_parse_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo compact real hexadecimal parsing",
        |ctx| super::compact_real(ctx, b"3FF0000000000000"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo compact real hexadecimal parsing")
    );
}

#[test]
fn legacy_declaration_utf8_refuses_before_malformed_input() {
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::WorkUnits,
        "creo legacy declaration UTF-8 validation",
        |ctx| super::parse_declaration(ctx, b"@name 1 1\xff"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "creo legacy declaration UTF-8 validation")
    );
}

#[test]
fn legacy_signed_utf8_refuses_before_invalid_scalar() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::signed_integer(ctx, b"1\xff"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn legacy_unsigned_utf8_refuses_before_invalid_scalar() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::unsigned_integer(ctx, b"1\xff"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn legacy_compact_real_utf8_refuses_work() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::compact_real(ctx, b"3FF0000000000000"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn legacy_string_utf8_refuses_before_binary_fallback() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo UTF-8 validation",
        |ctx| super::byte_string_value(ctx, b"a\xff", super::NullToken::RepresentsBytes),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo UTF-8 validation")
    );
}

#[test]
fn legacy_candidate_retain_refuses_work() {
    let payload = b"@foo 1 1\n0 1 7\n";
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo legacy candidate retain",
        |ctx| super::scan_scope(ctx, payload, 0..payload.len()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo legacy candidate retain")
    );
}

#[test]
fn legacy_model_trim_refuses_work() {
    let data = b"@Solid 1 0\n@model_name 2 10\n0 1 ->\n1 2 ROOT\n";
    let persistence = scan(data, std::iter::once(0..data.len())).expect("valid model fixture");
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo legacy model name trim",
        |ctx| persistence.model_name(ctx),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo legacy model name trim")
    );
}

#[test]
fn legacy_source_model_trim_refuses_work() {
    let data = b"@Solid 1 0\n@model_name 2 10\n0 1 ->\n1 2 ROOT\n";
    let persistence = scan(data, std::iter::once(0..data.len())).expect("valid model fixture");
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo legacy source model name trim",
        |ctx| persistence.first_source_model_name(ctx),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo legacy source model name trim")
    );
}

#[test]
fn signed_integer_sign_prefix_refuses_before_invalid_digits() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo signed integer sign prefix",
        |ctx| super::signed_integer(ctx, b"-x"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo signed integer sign prefix")
    );
}

#[test]
fn legacy_declaration_prefix_refuses_before_missing_fields() {
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo legacy declaration name prefix",
        |ctx| super::parse_declaration(ctx, b"name"),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo legacy declaration name prefix")
    );
}

#[test]
fn legacy_unit_object_prefix_refuses_work() {
    let factor = 0.393_700_787_401_574_8_f64;
    let data = format!(
        "@Solid 1 0\n@unit_arr 2 0\n@type 3 1\n@unit_type 4 1\n@factor 5 2\n@name 6 10\n0 1 ->\n1 2 [1]\n2 2 ->\n3 3 11\n3 4 0\n3 5 {factor_bits:016X}\n3 6 CM\n",
        factor_bits = factor.to_bits()
    );
    let persistence = scan(data.as_bytes(), std::iter::once(0..data.len()))
        .expect("the fixture states every scope inside its own bytes");
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "creo legacy unit object prefix",
        |ctx| persistence.principal_unit_system(ctx),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
            && resource.operation == "creo legacy unit object prefix")
    );
}
