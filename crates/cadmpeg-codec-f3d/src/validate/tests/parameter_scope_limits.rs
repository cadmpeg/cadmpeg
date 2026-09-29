// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    crate::native::F3dNative {
        design_parameter_scopes: vec![DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#1",
            DesignFeatureKind::Fillet,
            1,
        )],
        ..Default::default()
    }
}

fn scope_error(max_items: u64, max_retained: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let native = native();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    policy.limits.max_retained_bytes = max_retained;
    let (decode, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    ctx.decode = Some(&decode);
    super::super::validate_parameter_scopes(&ctx, &mut Vec::new()).unwrap_err()
}

#[test]
fn parameter_scope_index_refuses_collection_limit() {
    let error = scope_error(0, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D parameter scope records")
    );
}

#[test]
fn parameter_scope_finding_refuses_collection_limit() {
    let error = scope_error(1, u64::MAX);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "collect F3D native validation findings")
    );
}

#[test]
fn parameter_scope_entity_refuses_retained_limit() {
    let error = scope_error(u64::MAX, 0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "retain F3D validation entity")
    );
}

#[test]
fn invalid_parameter_scope_preserves_finding() {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let native = native();
    let ctx = super::super::Ctx::new(&ir, &native, None).unwrap();
    let mut findings = Vec::new();
    super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].message,
        "Fusion Design parameter scope has an invalid paired frame"
    );
}
