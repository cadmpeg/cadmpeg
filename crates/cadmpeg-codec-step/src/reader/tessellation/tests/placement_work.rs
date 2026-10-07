// SPDX-License-Identifier: Apache-2.0
//! Admission of placement comparison work.

use super::{CodecError, DecodePolicy};
use crate::test_support::with_service_context;

#[test]
fn distinct_placement_comparisons_refuse_before_keyed_insertion() {
    let placements = [cadmpeg_ir::transform::Transform::identity(); 32];
    // Each matrix is visited once; distinct keys pay keyed insertion work.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        "step_tessellation_distinct_placements",
        |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
                let result = super::super::distinct_placement_count(&placements, ctx);
                if let Err(CodecError::ResourceLimit(limit)) = &result {
                    assert_eq!(Some(*limit), ctx.resource_refusal());
                }
                result
            })
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == "step_tessellation_distinct_placements"));
    with_service_context(&[], |_, ctx| {
        assert_eq!(
            super::super::distinct_placement_count(&placements, ctx).expect("admitted comparisons"),
            1
        );
    });
}

#[test]
fn distinct_placements_preserve_signed_zero_equality() {
    with_service_context(&[], |_, ctx| {
        let identity = cadmpeg_ir::transform::Transform::identity();
        let mut negative_zero = identity.affine_rows();
        negative_zero[0][1] = -0.0;
        let negative_zero =
            cadmpeg_ir::transform::Transform::affine(negative_zero).expect("affine");
        let mut shifted = identity.affine_rows();
        shifted[0][3] = 1.0;
        let shifted = cadmpeg_ir::transform::Transform::affine(shifted).expect("affine");
        assert_eq!(
            super::super::distinct_placement_count(&[identity, negative_zero, shifted], ctx)
                .expect("count"),
            2
        );
    });
}
