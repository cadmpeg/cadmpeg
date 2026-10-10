// SPDX-License-Identifier: Apache-2.0
//! Reuse the original to-object and edge fixture for actual query refusal.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_refusal(operation: &'static str) {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
        let Err(CodecError::ResourceLimit(original)) = super::edge_flange_to_object_fixture(ctx, false, false) else {
            panic!("original to-object fixture must reach actual source query");
        };
        assert_eq!(original.operation, operation);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(ctx.charge_work(0, "later EdgeFlange query boundary"), Err(CodecError::ResourceLimit(later)) if later == original));
    });
}

#[test]
fn direct_unique_edge_flange_target_group_source_preserves_original_refusal() {
    assert_refusal("visit F3D EdgeFlange target group source");
}

#[test]
fn direct_unique_edge_flange_target_operand_source_preserves_original_refusal() {
    assert_refusal("visit F3D EdgeFlange target operand source");
}

#[test]
fn direct_unique_edge_flange_edge_group_source_preserves_original_refusal() {
    assert_refusal("visit F3D EdgeFlange edge group source");
}
