// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::analytic::LineCurve;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn fallback_emitted_identity_lookup_preserves_work_refusal() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    ));
    let curves = ["rhino:test:curve#one", "rhino:test:curve#two"].map(|id| Curve {
        id: CurveId::try_from(id).unwrap(),
        geometry: geometry.clone(),
        source_object: None,
    });
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino emitted fallback identity lookup",
        |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let mut draft = super::super::BrepDraft::default();
            draft.draft.model_mut().curves.extend(curves.clone());
            let result = draft.free_carrier_fallback(&ctx, "test fallback");
            if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                assert_eq!(ctx.resource_refusal(), Some(*refusal));
            }
            result
        },
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("fallback identity lookup must propagate its refusal");
    };
    assert_eq!(refusal.operation, "Rhino emitted fallback identity lookup");
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
}

#[test]
fn fallback_self_link_equality_preserves_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut links = Vec::new();
    let error = super::super::append_link_to_record(
        &ctx,
        "test:link#1",
        &mut links,
        "test:link#1",
    )
    .unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("self-link comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino source link equality");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert!(links.is_empty());
}
