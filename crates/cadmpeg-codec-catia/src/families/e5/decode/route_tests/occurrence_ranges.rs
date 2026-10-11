// SPDX-License-Identifier: Apache-2.0

use crate::assemble::quintic_jet_pcurve;
use crate::families::e5::decode::{
    parameter_range_agreement_tolerance, EPS_E5_DECODE_EXACT_GEOMETRY,
};

use cadmpeg_ir::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point3;

#[test]
fn support_range_agreement_requires_matching_endpoints() {
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [0.0, 5.0]).is_some());
    assert!(parameter_range_agreement_tolerance([0.0, 5.0], [1.0, 6.0]).is_none());
}

#[test]
fn e5_boundary_nurbs_cache_refuses_before_copy() {
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid linear NURBS"),
    ));
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        curve.try_clone_for_decode(ctx, "catia_e5_boundary_curve_copy")
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_boundary_curve_copy")
    );
}

#[test]
fn e5_intersection_context_copy_refuses_retained_limit() {
    let sides = ["left", "right"].map(|name| cadmpeg_ir::geometry::IntcurveSupportSide {
        surface: Some(
            SurfaceId::mint(format!("catia:test:surface#{name}")).expect("valid surface identity"),
        ),
        pcurve: None,
    });
    let context = cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
        sides,
        [0.0, 1.0],
        std::array::from_fn(|_| Vec::new()),
    )
    .expect("valid support context");
    let refused = crate::test_support::with_retained_limit(0, |ctx| {
        crate::resource::copy_intcurve_support_context(
            ctx,
            &context,
            "catia_e5_intersection_context",
        )
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_intersection_context")
    );
    let copied = crate::test_support::with_service_context(|ctx| {
        crate::resource::copy_intcurve_support_context(
            ctx,
            &context,
            "catia_e5_intersection_context",
        )
    })
    .expect("service budget admits context copy");
    assert_eq!(copied, context);
}

#[test]
fn quintic_jet_reproduces_endpoint_second_order_data() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
        .expect("fixture fits input limit");
    let curve = quintic_jet_pcurve(
        &ctx,
        5,
        &[0.0, 2.0],
        &[[0.0, 0.0], [2.0, 0.0]],
        (&[[1.0, 0.0], [1.0, 0.0]], &[[0.0, 0.0], [0.0, 0.0]]),
        &mut crate::nurbs::LaneRefusals::new(),
        "test record",
    )
    .expect("service resource budget")
    .expect("linear quintic segment");
    for parameter in [0.0, 0.5, 1.0, 2.0] {
        let point = cadmpeg_ir::eval::decode::pcurve_uv(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
            &curve,
            parameter,
        )
        .expect("jet evaluation");
        assert!((point.u - parameter).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(point.v.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    }
}

#[test]
fn reversing_nurbs_preserves_tiny_knot_domain() {
    let tiny = 1e-200;
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            1,
            vec![tiny, tiny, 2.0 * tiny, 2.0 * tiny],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            None,
            false,
        )
        .expect("fixture constructor admission")
        .expect("valid tiny-domain NURBS"),
    ));
    let (reversed, range) = crate::test_support::with_service_context(|ctx| {
        crate::nurbs::reverse_curve_geometry(
            ctx,
            &curve,
            [tiny, 2.0 * tiny],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record",
        )
    })
    .expect("service profile admits range operation")
    .expect("reversed NURBS");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(reversed)) = reversed else {
        panic!("expected NURBS");
    };
    assert_eq!(range, [tiny, 2.0 * tiny]);
    assert_eq!(
        reversed.knots().as_slice(),
        [tiny, tiny, 2.0 * tiny, 2.0 * tiny]
    );
    assert_eq!(
        reversed.control_points(),
        [Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)]
    );
}
