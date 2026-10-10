// SPDX-License-Identifier: Apache-2.0

use super::super::bind_hem_operation_from_parameters;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn hem_binding_entry_keeps_original_refusal_before_family_or_missing_parameter_gate() {
    for kind in [DesignFeatureKind::Chamfer, DesignFeatureKind::Hem] {
        let mut scope = DesignParameterScope::empty("f3d:stream:design-parameter-scope#100", kind, 42);
        let native = scope.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original Hem binding refusal") else {
            panic!("actual zero-work context refusal");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.additional, 1);
        assert!(matches!(bind_hem_operation_from_parameters(&ctx, &[], &mut scope, &[], &[]), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(scope, native);
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn hem_binding_ineligible_family_needs_no_positive_resource_or_scope_mutation() {
    let mut scope = DesignParameterScope::empty("f3d:stream:design-parameter-scope#100", DesignFeatureKind::Chamfer, 42);
    let native = scope.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    bind_hem_operation_from_parameters(&ctx, &[], &mut scope, &[], &[]).unwrap();
    assert_eq!(scope, native);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}
