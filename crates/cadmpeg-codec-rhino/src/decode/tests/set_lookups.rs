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
    let scan = super::scan_with_objects(&[super::object_record(
        super::ArchiveVersion::V5,
        1,
        super::POINT_CLASS,
    )]);
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, root) = DecodeContext::from_root_bytes(scan.data, &arena, &policy)?;
        let mut transaction =
            super::DecodeContext::new(&scan, crate::mesh::MeshExpand::new(&ctx, root))?;
        let id = transaction.unknown(0).unwrap().id().to_string();
        let result = transaction.append_link(0, &id);
        assert!(transaction.unknown(0).unwrap().links().is_empty());
        if let Err(CodecError::ResourceLimit(refusal)) = &result {
            assert_eq!(ctx.resource_refusal(), Some(*refusal));
        }
        result
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino source link equality",
        run,
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("self-link comparison must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "Rhino source link equality");
    assert!(!run(u64::MAX).unwrap());
}
