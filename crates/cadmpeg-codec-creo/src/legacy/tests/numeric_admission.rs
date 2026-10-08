// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::ResourceDimension;

use super::super::ValueKind;

fn assert_numeric_refusal(
    data: &[u8],
    dimension: ResourceDimension,
    operation: &'static str,
) {
    let (scopes, parents) = super::object_fixture_parts(data);
    let error = crate::test_support::last_refusal_at(&[], dimension, operation, |ctx| {
        super::super::numeric_records(ctx, data, &scopes, ValueKind::INTEGER,
            super::super::signed_integer, &parents)
    });

    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == dimension && resource.operation == operation)
    );
}

#[test]
fn numeric_child_runs_refuse_before_vec_growth() {
    assert_numeric_refusal(
        b"@arr 1 1\n0 1 [1]\n1 1 9\n",
        ResourceDimension::CollectionItems,
        "creo legacy numeric child runs",
    );
}

#[test]
fn numeric_record_name_refuses_before_retained_copy() {
    assert_numeric_refusal(
        b"@count 1 1\n0 1 9\n",
        ResourceDimension::RetainedBytes,
        "creo legacy numeric record names",
    );
}

#[test]
fn numeric_records_refuse_before_vec_growth() {
    assert_numeric_refusal(
        b"@count 1 1\n0 1 9\n",
        ResourceDimension::CollectionItems,
        "creo legacy numeric records",
    );
}
