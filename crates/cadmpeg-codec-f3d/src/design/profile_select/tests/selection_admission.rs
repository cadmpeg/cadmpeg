// SPDX-License-Identifier: Apache-2.0

use crate::design::profile_select::{
    ordered_unique_profile_selections, same_member_point_bits, unique_resolved_selection,
    ResolvedProfileSelection,
};
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::Point3;

#[test]
fn unique_selection_refuses_original_empty_slots_and_value_comparison() {
    let values = [None, Some(vec![1_u32, 2, 3]), None, Some(vec![1, 2, 3])];
    for operation in [
        "f3d unique selection first search",
        "f3d unique selection agreement search",
        "f3d test selected vector comparison",
    ] {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            |limit| {
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = limit;
                crate::test_support::with_decode_policy(&policy, |ctx| {
                    unique_resolved_selection(
                        values.iter().map(Option::as_ref),
                        ctx,
                        |left, right| {
                            ctx.equal(*left, *right, "f3d test selected vector comparison")
                        },
                    )
                    .map(|selection| selection.is_some())
                })
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.dimension == ResourceDimension::WorkUnits && failure.operation == operation));
    }
}

#[test]
fn historical_point_bits_preserve_signed_zero_and_nan_payloads() {
    let point = Point3::new(0.0, f64::from_bits(0x7ff8_0000_0000_0001), 2.0);
    let points = [vec![point]];
    crate::test_support::with_decode_context(|ctx| {
        assert!(same_member_point_bits(&points, &points, ctx)?);
        assert!(!same_member_point_bits(
            &points,
            &[vec![Point3::new(-0.0, point.y, 2.0)]],
            ctx,
        )?);
        assert!(!same_member_point_bits(
            &points,
            &[vec![Point3::new(
                0.0,
                f64::from_bits(0x7ff8_0000_0000_0002),
                2.0
            )]],
            ctx,
        )?);
        Ok::<_, CodecError>(())
    })
    .unwrap();
}

#[test]
fn ambiguous_ordered_selection_drops_candidate_storage() {
    let values = [Some(ResolvedProfileSelection::Loops(vec![0])), None];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1_048_576;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(ordered_unique_profile_selections(&values, ctx)?, None);
        ctx.reserve_scoped(
            policy.limits.max_materialized_bytes,
            "selection test released storage",
        )?;
        Ok::<_, CodecError>(())
    })
    .unwrap();
}
