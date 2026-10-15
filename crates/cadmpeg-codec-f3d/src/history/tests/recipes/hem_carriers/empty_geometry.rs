// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::DecodePolicy;

#[test]
fn hem_empty_geometry_gate_is_free() {
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#42",
        crate::records::feature::scope::DesignFeatureKind::Hem,
        42,
    );
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let empty = crate::history::hem_geometry_semantics(ctx, &scope, 7, &[]).unwrap();
        assert_eq!(empty.direction, None);
        assert_eq!(empty.gap_length_form, None);
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn hem_empty_gap_carriers_are_free() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        assert_eq!(
            crate::history::hem_gap_length_form(ctx, &[], &[], None).unwrap(),
            None
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}
