// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

use super::super::ValueKind;

fn assert_numeric_refusal(
    data: &[u8],
    collection_limit: Option<u64>,
    retained_limit: Option<u64>,
    dimension: ResourceDimension,
    operation: &'static str,
) {
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
    let error = super::super::numeric_records(
        &ctx,
        data,
        &persistence.scopes,
        ValueKind::INTEGER,
        super::super::signed_integer,
        &parents,
    )
    .expect_err("the next numeric allocation exceeds the limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation));
}

#[test]
fn numeric_child_runs_refuse_before_vec_growth() {
    assert_numeric_refusal(
        b"@arr 1 1\n0 1 [1]\n1 1 9\n",
        Some(2),
        None,
        ResourceDimension::CollectionItems,
        "creo legacy numeric child runs",
    );
}

#[test]
fn numeric_record_name_refuses_before_retained_copy() {
    assert_numeric_refusal(
        b"@count 1 1\n0 1 9\n",
        None,
        Some(0),
        ResourceDimension::RetainedBytes,
        "creo legacy numeric record names",
    );
}

#[test]
fn numeric_records_refuse_before_vec_growth() {
    assert_numeric_refusal(
        b"@count 1 1\n0 1 9\n",
        Some(1),
        None,
        ResourceDimension::CollectionItems,
        "creo legacy numeric records",
    );
}
