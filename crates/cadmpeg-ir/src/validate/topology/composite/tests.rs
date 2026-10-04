// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::geometry::{
    CompositeCurveSegment, CompositeCurveTransition, CurveGeometry, SolvedCurveGeometry,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn model(rows: &[(&str, &[&str])]) -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    let template = ir.model.curves[0].clone();
    ir.model.curves = rows
        .iter()
        .map(|(identity, children)| {
            let mut curve = template.clone();
            curve.id = (*identity).try_into().unwrap();
            curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
                segments: children
                    .iter()
                    .map(|child| CompositeCurveSegment {
                        curve: (*child).try_into().unwrap(),
                        same_sense: true,
                        transition: CompositeCurveTransition::Discontinuous,
                    })
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap(),
                self_intersect: None,
            });
            curve
        })
        .collect();
    ir
}

#[test]
fn composite_cycle_validation_preserves_all_resource_refusals() {
    let ir = model(&[("test:model:curve#a", &["test:model:curve#a"])]);
    for dimension in [
        ResourceDimension::MaterializedBytes,
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
        ResourceDimension::RetainedBytes,
        ResourceDimension::RecursionDepth,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = 0,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 0,
            _ => panic!("test dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(limit)) = super::check(&ctx, &ir, &mut findings) else {
            panic!("cycle validation must refuse");
        };
        assert_eq!(limit.dimension, dimension);
        assert!(findings.is_empty());
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
        );
    }
}

#[test]
fn composite_cycle_validation_keeps_lexical_roots_and_repeated_backedges() {
    let ir = model(&[
        ("test:model:curve#z", &["test:model:curve#z"]),
        (
            "test:model:curve#a",
            &["test:model:curve#a", "test:model:curve#a"],
        ),
    ]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.entity.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("test:model:curve#a"),
            Some("test:model:curve#a"),
            Some("test:model:curve#z")
        ]
    );
    for finding in findings {
        assert_eq!(
            finding.check,
            crate::report::check::Check::ReferentialIntegrity
        );
        assert_eq!(finding.severity, crate::report::Severity::Error);
        assert_eq!(finding.message, "composite curve graph contains a cycle");
    }
}

#[test]
fn composite_cycle_validation_uses_active_session_depth_for_nested_frames() {
    let ir = model(&[
        ("test:model:curve#a", &["test:model:curve#b"]),
        ("test:model:curve#b", &["test:model:curve#missing"]),
    ]);
    for (limit, caller) in [(1, false), (2, true)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_recursion_depth = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let guard = caller.then(|| ctx.enter_nested("active caller").unwrap());
        let mut findings = Vec::new();
        let Err(CodecError::ResourceLimit(resource)) = super::check(&ctx, &ir, &mut findings)
        else {
            panic!("nested frame must refuse");
        };
        assert_eq!(resource.dimension, ResourceDimension::RecursionDepth);
        assert!(findings.is_empty());
        drop(guard);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == resource)
        );
    }
}

#[test]
fn composite_cycle_validation_releases_scoped_storage_without_retained_copies() {
    let ir = model(&[
        (
            "test:model:curve#a",
            &["test:model:curve#b", "test:model:curve#b"],
        ),
        ("test:model:curve#b", &["test:model:curve#missing"]),
    ]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 16384;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut findings = Vec::new();
    super::check(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    drop(ctx.reserve_scoped(16384, "cycle scopes released").unwrap());
    ctx.finish_session().unwrap();
}
