// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;

use super::super::{NullToken, ValueKind};

fn assert_string_collection_refusal(data: &[u8], operation: &'static str) {
    let (scopes, parents) = super::object_fixture_parts(data);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::CollectionItems,
        operation,
        |ctx| super::super::string_records(ctx, data, &scopes, &parents),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation)
    );
}

const ONE_STRING_ELEMENT: &[u8] = b"@labels 1 10\n0 1 [1]\n1 1 W\n";

#[test]
fn active_string_array_refuses_before_growth() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, "creo legacy active string arrays");
}

#[test]
fn string_array_child_refuses_before_hash_node() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, "creo legacy string array child nodes");
}

#[test]
fn string_array_child_rows_refuse_before_growth() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, "creo legacy string array child rows");
}

#[test]
fn string_array_element_offset_refuses_before_hash_node() {
    assert_string_collection_refusal(
        ONE_STRING_ELEMENT,
        "creo legacy string array element offsets",
    );
}

#[test]
fn string_array_values_refuse_before_growth() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, "creo legacy string array values");
}

#[test]
fn string_records_refuse_before_growth() {
    assert_string_collection_refusal(b"@name 1 10\n0 1 W\n", "creo legacy string records");
}

fn assert_string_retained_refusal(data: &[u8], operation: &'static str) {
    let (scopes, parents) = super::object_fixture_parts(data);
    let error = crate::test_support::last_refusal_at(
        &[],
        ResourceDimension::RetainedBytes,
        operation,
        |ctx| super::super::string_records(ctx, data, &scopes, &parents),
    );

    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == operation)
    );
}

#[test]
fn string_record_name_refuses_before_retained_copy() {
    assert_string_retained_refusal(b"@name 1 10\n0 1 W\n", "creo legacy string record names");
}

#[test]
fn string_utf8_payload_refuses_before_retained_copy() {
    assert_string_retained_refusal(b"@name 1 10\n0 1 W\n", "creo legacy string UTF-8 payload");
}

#[test]
fn string_byte_payload_refuses_before_retained_copy() {
    assert_string_retained_refusal(b"@name 1 10\n0 1 \xff\n", "creo legacy string byte payload");
}

fn assert_scalar_string_refusal(operation: &'static str, dimension: ResourceDimension) {
    let data = b"@code 1 3\n0 1 TEXT\n";
    let (scopes, parents) = super::object_fixture_parts(data);
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        super::super::scalar_string_records(
            ctx,
            data,
            &scopes,
            ValueKind::TYPE3,
            NullToken::RepresentsNull,
            &parents,
        )
    });

    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation)
    );
}

#[test]
fn scalar_string_name_refuses_before_retained_copy() {
    assert_scalar_string_refusal(
        "creo legacy scalar string names",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn scalar_string_records_refuse_before_growth() {
    assert_scalar_string_refusal(
        "creo legacy scalar string records",
        ResourceDimension::CollectionItems,
    );
}
