// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn named_variable_row_vec_refuses_before_growth() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    assert_eq!(
        variable_row_with_limits(u64::MAX, u64::MAX)
            .expect("one row admitted")
            .rows
            .len(),
        1
    );
    let error = variable_row_with_limits(
        crate::test_support::allocation_limit_at(
            ResourceDimension::CollectionItems,
            Some("creo variable rows"),
            |cap| variable_row_with_limits(cap, u64::MAX),
        ),
        u64::MAX,
    )
    .expect_err("one row needs one item");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "creo variable rows"));
}

#[test]
fn named_variable_value_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = variable_row_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo variable value body"),
            |cap| variable_row_with_limits(u64::MAX, cap),
        ),
    )
    .expect_err("value needs one byte");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable value body"));
}

#[test]
fn named_variable_guess_body_refuses_before_retention() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = variable_row_with_limits(
        u64::MAX,
        crate::test_support::allocation_limit_at(
            ResourceDimension::RetainedBytes,
            Some("creo variable guess body"),
            |cap| variable_row_with_limits(u64::MAX, cap),
        ),
    )
    .expect_err("guess needs three bytes");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes
            && limit.operation == "creo variable guess body"));
}
