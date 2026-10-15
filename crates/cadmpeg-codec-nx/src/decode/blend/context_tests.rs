// SPDX-License-Identifier: Apache-2.0
//! Default-policy admission for in-memory geometry evaluation.

use super::closest_spine_parameter;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
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
    let nurbs =
        NurbsCurve::from_lanes(1, knots, points, None, false).expect("clamped linear test spine");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        parameter_range: None,
        id: id.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
        source_object: None,
    });
    (ir, id)
}

#[test]
fn in_memory_spine_inverse_accepts_normal_input_under_default_policy() {
    let (ir, curve) = spine_model(2);
    // This test asserts the default policy.
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())
        .expect("empty in-memory root is admitted");
    let parameter = closest_spine_parameter(&ctx, &ir, &curve, Point3::new(0.25, 0.0, 0.0), None)
        .expect("linear spine has a closest parameter");
    assert!((parameter - 0.25).abs() <= 4.0 * f64::EPSILON);
}

#[test]
fn closest_pcurve_parameters_fit_a_polynomial_line() {
    use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use cadmpeg_ir::math::Point2;

    let pcurve = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            None,
            false,
        )
        .expect("polynomial pcurve"),
    };
    let result = crate::test_support::with_decode_context(|ctx| {
        super::closest_pcurve_parameters(ctx, &pcurve, Point2::new(0.5, 0.0), None)
    })
    .expect("service admission")
    .expect("linear closest parameter");
    assert_eq!(result, vec![0.5]);
}
