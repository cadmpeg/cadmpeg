// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::super::{NullToken, ValueKind};

fn assert_string_collection_refusal(data: &[u8], limit: u64, operation: &'static str) {
    let (persistence, parents) = super::object_fixture_parts(data);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::super::string_records(&ctx, data, &persistence.scopes, &parents)
        .expect_err("the next string collection item exceeds the limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation));
}

const ONE_STRING_ELEMENT: &[u8] = b"@labels 1 10\n0 1 [1]\n1 1 W\n";

#[test]
fn active_string_array_refuses_before_btree_node() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, 2, "creo legacy active string arrays");
}

#[test]
fn string_array_child_refuses_before_btree_node() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, 3, "creo legacy string array child nodes");
}

#[test]
fn string_array_child_rows_refuse_before_growth() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, 4, "creo legacy string array child rows");
}

#[test]
fn string_array_element_offset_refuses_before_btree_node() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, 5, "creo legacy string array element offsets");
}

#[test]
fn string_array_values_refuse_before_growth() {
    assert_string_collection_refusal(ONE_STRING_ELEMENT, 7, "creo legacy string array values");
}

#[test]
fn string_records_refuse_before_growth() {
    assert_string_collection_refusal(b"@name 1 10\n0 1 W\n", 1, "creo legacy string records");
}

fn assert_string_retained_refusal(data: &[u8], limit: u64, operation: &'static str) {
    let (persistence, parents) = super::object_fixture_parts(data);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::super::string_records(&ctx, data, &persistence.scopes, &parents)
        .expect_err("the next string copy exceeds the retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == operation));
}

#[test]
fn string_record_name_refuses_before_retained_copy() {
    assert_string_retained_refusal(b"@name 1 10\n0 1 W\n", 1, "creo legacy string record names");
}

#[test]
fn string_utf8_payload_refuses_before_retained_copy() {
    assert_string_retained_refusal(b"@name 1 10\n0 1 W\n", 0, "creo legacy string UTF-8 payload");
}

#[test]
fn string_byte_payload_refuses_before_retained_copy() {
    assert_string_retained_refusal(b"@name 1 10\n0 1 \xff\n", 0, "creo legacy string byte payload");
}

fn assert_scalar_string_refusal(
    collection_limit: Option<u64>,
    retained_limit: Option<u64>,
    operation: &'static str,
    dimension: ResourceDimension,
) {
    let data = b"@code 1 3\n0 1 TEXT\n";
    let (persistence, parents) = super::object_fixture_parts(data);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    if let Some(limit) = collection_limit {
        policy.limits.max_collection_items = limit;
    }
    if let Some(limit) = retained_limit {
        policy.limits.max_retained_bytes = limit;
    }
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::super::scalar_string_records(
        &ctx,
        data,
        &persistence.scopes,
        ValueKind::TYPE3,
        NullToken::RepresentsNull,
        &parents,
    )
    .expect_err("the next scalar string allocation exceeds the limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation));
}

#[test]
fn scalar_string_name_refuses_before_retained_copy() {
    assert_scalar_string_refusal(
        None,
        Some(4),
        "creo legacy scalar string names",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn scalar_string_records_refuse_before_growth() {
    assert_scalar_string_refusal(
        Some(1),
        None,
        "creo legacy scalar string records",
        ResourceDimension::CollectionItems,
    );
}
