// SPDX-License-Identifier: Apache-2.0
//! Default-policy admission for in-memory geometry evaluation.

use super::closest_spine_parameter;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::math::Point3;

fn spine_model(count: u32) -> (CadIr, CurveId) {
    let id = CurveId::mint("nx:test:curve#context-spine").expect("valid test identity");
    let points: Vec<Point3> = (0..count)
        .map(|index| Point3::new(f64::from(index), 0.0, 0.0))
        .collect();
    let mut knots = vec![0.0];
    knots.extend((0..count).map(f64::from));
    knots.push(f64::from(count - 1));
    let bytes = serde_json::to_vec(&(knots.as_slice(), points.as_slice())).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items =
        cadmpeg_core::decode::u64_from_index(knots.len() + points.len());
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let nurbs = NurbsCurve::from_lanes(&ctx, 1, knots, points, None, false)
        .expect("service storage")
        .expect("clamped linear test spine");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        source_object: None,
    });
    (ir, id)
}

#[test]
fn in_memory_spine_inverse_refuses_default_scoped_limit() {
    // An empty root admits 16 MiB of scoped storage. Each residual uses 24 bytes.
    const POLES_OVER_SCOPED_LIMIT: u32 = 16 * 1024 * 1024 / 24 + 1;
    let (ir, curve) = spine_model(POLES_OVER_SCOPED_LIMIT);
    // This test asserts the default policy.
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("empty in-memory root is admitted");
    let error = closest_spine_parameter(&ctx, &ir, &curve, Point3::new(0.25, 0.0, 0.0), None)
        .expect_err("residual storage exceeds the default empty-root allowance");
    assert_eq!(error.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(error.operation, "nx spine NURBS residuals");
    assert_eq!(error.limit, 16 * 1024 * 1024);
    assert!(error.additional > error.limit);
}

#[test]
fn in_memory_spine_inverse_accepts_normal_input_under_default_policy() {
    let (ir, curve) = spine_model(2);
    // This test asserts the default policy.
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("empty in-memory root is admitted");
    let parameter = closest_spine_parameter(&ctx, &ir, &curve, Point3::new(0.25, 0.0, 0.0), None)
        .expect("normal residual storage fits the default allowance")
        .expect("linear spine has a closest parameter");
    assert!((parameter - 0.25).abs() <= 4.0 * f64::EPSILON);
}

#[test]
fn closest_pcurve_controls_refuse_one_below_collection_need() {
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use cadmpeg_ir::math::Point2;

    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(&cadmpeg_test_support::service_decode_context(), 
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        ).expect("fixture pcurve construction admission")
        .expect("polynomial pcurve"),
    };
    let error = crate::test_support::with_decode_context_over(
        &[],
        |policy| policy.limits.max_collection_items = 1,
        |ctx| super::closest_pcurve_parameters(ctx, &pcurve, Point2::new(0.5, 0.0), None),
    )
    .expect_err("two controls exceed one slot");
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems
            && limit.operation == "IR pcurve control copy"
            && limit.used == 0
            && limit.additional == 2));
    let result = crate::test_support::with_decode_context(|ctx| {
        super::closest_pcurve_parameters(ctx, &pcurve, Point2::new(0.5, 0.0), None)
    })
    .expect("service admission")
    .expect("linear closest parameter");
    assert_eq!(result, vec![0.5]);
}
