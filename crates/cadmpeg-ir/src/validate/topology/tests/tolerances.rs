// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn refused(check: impl Fn(&DecodeContext<'_>, &mut Vec<Finding>) -> Result<(), CodecError>) {
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = check(&ctx, &mut findings) else {
            panic!("tolerance check must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn document_tolerance_warning_preserves_resource_refusals() {
    let mut ir = CadIr::empty();
    ir.tolerances = crate::units::Tolerances::new(2.0e6, 1.0).unwrap();
    refused(|ctx, findings| super::super::check_tolerances(ctx, &ir, findings));
}

#[test]
fn topology_tolerance_warning_preserves_resource_refusals() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.model.vertices[0].tolerance = crate::scalar::PositiveReal::new(2.0e6);
    refused(|ctx, findings| super::super::check_topology_tolerances(ctx, &ir, findings));
}

#[test]
fn tolerance_validation_preserves_warning_owners_and_arena_order() {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.tolerances = crate::units::Tolerances::new(2.0e6, 1.0).unwrap();
    let tolerance = crate::scalar::PositiveReal::new(2.0e6);
    ir.model.vertices[0].tolerance = tolerance;
    ir.model.edges[0].tolerance = tolerance;
    ir.model.faces[0].tolerance = tolerance;
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_tolerances(&ctx, &ir, &mut findings).unwrap();
    super::super::check_topology_tolerances(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.len(), 4);
    for finding in &findings {
        assert_eq!(finding.check, Check::Tolerances);
        assert_eq!(finding.severity, Severity::Warning);
    }
    assert_eq!(findings[0].entity, None);
    assert_eq!(
        findings[0].message,
        "document tolerance is outside a sane canonical range"
    );
    for (finding, owner) in findings[1..].iter().zip([
        ir.model.vertices[0].id.as_str(),
        ir.model.edges[0].id.as_str(),
        ir.model.faces[0].id.as_str(),
    ]) {
        assert_eq!(finding.entity.as_deref(), Some(owner));
        assert_eq!(
            finding.message,
            "topology tolerance is outside a sane canonical range"
        );
    }
}
