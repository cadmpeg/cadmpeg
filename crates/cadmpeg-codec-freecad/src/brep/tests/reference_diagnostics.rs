// SPDX-License-Identifier: Apache-2.0
//! Binary reference diagnostics use the caller's admitted text owner.

use crate::brep::{binary_orientation, checked_binary_reference, TextOrientation};
use crate::test_support::{refusal_at, with_service_context};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn reference_refusal(value: i32, count: usize, allow_zero: bool, expected: &str) {
    let error = refusal_at(
        ResourceDimension::RetainedBytes,
        &[],
        "FreeCAD binary reference diagnostic",
        |ctx| checked_binary_reference(ctx, value, count, allow_zero, "edge curve"),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == 0 && limit.additional == cadmpeg_core::decode::u64_from_index(expected.len())));
    with_service_context(&[], |ctx| {
        assert!(
            matches!(checked_binary_reference(ctx, value, count, allow_zero, "edge curve"),
            Err(CodecError::Malformed(message)) if message == expected)
        );
    });
}

#[test]
fn negative_binary_reference_diagnostic_refuses_before_formatting() {
    reference_refusal(-1, 12, true, "negative binary edge curve");
}

#[test]
fn binary_reference_above_table_diagnostic_refuses_before_formatting() {
    reference_refusal(
        13,
        12,
        true,
        "binary edge curve index 13 exceeds table count 12",
    );
}

#[test]
fn required_zero_binary_reference_diagnostic_refuses_before_formatting() {
    reference_refusal(
        0,
        12,
        false,
        "binary edge curve index 0 exceeds table count 12",
    );
}

#[test]
fn invalid_binary_orientation_diagnostic_refuses_before_formatting() {
    let error = refusal_at(
        ResourceDimension::RetainedBytes,
        &[],
        "FreeCAD binary orientation diagnostic",
        |ctx| binary_orientation(ctx, 255),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.used == 0 && limit.additional == cadmpeg_core::decode::u64_from_index(
            "invalid binary orientation 255".len())));
    with_service_context(&[], |ctx| {
        assert!(
            matches!(binary_orientation(ctx, 255), Err(CodecError::Malformed(message))
            if message == "invalid binary orientation 255")
        );
    });
}

#[test]
fn valid_binary_reference_and_orientation_need_no_work_or_text_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        checked_binary_reference(&ctx, 0, 0, true, "root location").unwrap(),
        0
    );
    assert_eq!(
        checked_binary_reference(&ctx, 12, 12, false, "edge curve").unwrap(),
        12
    );
    for (value, expected) in [
        (0, TextOrientation::Forward),
        (1, TextOrientation::Reversed),
        (2, TextOrientation::Internal),
        (3, TextOrientation::External),
    ] {
        assert_eq!(binary_orientation(&ctx, value).unwrap(), expected);
    }
    assert_eq!(ctx.resource_refusal(), None);
}
