// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::extrude_feature_definition;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::BooleanOp;

#[test]
fn selection_index_separator_refusal_propagates() {
    use crate::native::attach::feature_projection::selection_indices_native;
    let error = crate::test_support::resource_refusal_at(
        &[], ResourceDimension::WorkUnits, "NX body selection separator",
        |ctx| selection_indices_native(ctx, Some(&0), &[12, 99], |value| *value),
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "NX body selection separator" && limit.additional == 1));
    crate::test_support::with_decode_context(|ctx| {
        assert_eq!(selection_indices_native(ctx, Some(&0), &[12, 99], |value| *value).unwrap(), "nx:om-object-indices#0,12,99");
        assert_eq!(selection_indices_native(ctx, None, &[0], |value| *value).unwrap(), "nx:om-object-indices#0");
        assert_eq!(selection_indices_native(ctx, None, &[] as &[u32], |value| *value).unwrap(), "nx:om-object-indices#");
    });
}

fn extrude_profile_format_refusal(dimension: ResourceDimension) {
    crate::test_support::with_decode_context_over(
        &[],
        |policy| match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            _ => panic!("profile formatting uses work and retained bytes"),
        },
        |ctx| {
            let CodecError::ResourceLimit(limit) = extrude_feature_definition(
                ctx,
                Some("nx:profile#1"),
                None,
                BooleanOp::NewBody,
                &[],
            )
            .unwrap_err() else {
                panic!("profile formatting must propagate refusal");
            };
            assert_eq!(limit.operation, "NX extrude construction profile");
            assert_eq!(limit.dimension, dimension);
            assert_eq!(ctx.resource_refusal(), Some(limit));
        },
    );
}

#[test]
fn extrude_profile_format_refuses_work() {
    extrude_profile_format_refusal(ResourceDimension::WorkUnits);
}

#[test]
fn extrude_profile_format_refuses_retained_bytes() {
    extrude_profile_format_refusal(ResourceDimension::RetainedBytes);
}
