// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn hem_empty_geometry_gate_is_free_and_preserves_the_original_refusal() {
    let scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#42",
        crate::records::feature::scope::DesignFeatureKind::Hem,
        42,
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let empty = crate::history::hem_geometry_semantics(&ctx, &scope, 7, &[]).unwrap();
    assert_eq!(empty.direction, None);
    assert_eq!(empty.gap_length_form, None);
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty Hem refusal")
    else {
        panic!("zero work ceiling refuses one actual work unit");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert!(
        matches!(crate::history::hem_geometry_semantics(&ctx, &scope, 7, &[]),
        Err(CodecError::ResourceLimit(later)) if later == original)
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn hem_empty_gap_carriers_are_free_and_preserve_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(
        crate::history::hem_gap_length_form(&ctx, &[], &[], None).unwrap(),
        None
    );
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) =
        ctx.charge_work(1, "original empty Hem gap refusal")
    else {
        panic!("zero work ceiling refuses one actual work unit");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert!(
        matches!(crate::history::hem_gap_length_form(&ctx, &[], &[], None),
        Err(CodecError::ResourceLimit(later)) if later == original)
    );
    assert_eq!(ctx.resource_refusal(), Some(original));
}
