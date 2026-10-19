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
fn composite_cycle_validation_admits_repeated_acyclic_edges_and_missing_children() {
    let ir = model(&[
        (
            "test:model:curve#a",
            &["test:model:curve#b", "test:model:curve#b"],
        ),
        ("test:model:curve#b", &["test:model:curve#missing"]),
    ]);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::check(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}
