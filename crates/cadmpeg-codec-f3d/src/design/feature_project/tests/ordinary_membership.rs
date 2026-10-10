// SPDX-License-Identifier: Apache-2.0
//! Actual feature membership preserves source order and the original refusal.

use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

mod faces;
mod fixed;
mod fixtures;

fn assert_original_refusal<T>(
    operation: &'static str,
    project: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, operation, Some(1));
        let Err(CodecError::ResourceLimit(original)) = project(ctx) else {
            panic!("actual membership phase must refuse");
        };
        assert_eq!(original.operation, operation);
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(original.clone()));
        assert!(matches!(ctx.charge_work(0, "later membership boundary"),
            Err(CodecError::ResourceLimit(later)) if later == original));
    });
}
