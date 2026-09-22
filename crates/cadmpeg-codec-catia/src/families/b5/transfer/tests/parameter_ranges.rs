// SPDX-License-Identifier: Apache-2.0

use super::super::*;
#[test]
fn numerical_audit_disjoint_small_range_recharts_curve() {
    for d in [1., 1e-16] {
        let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0., 0., d, d],
                vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
                None,
                false,
            )
            .unwrap(),
        ));
        let mut refusals = crate::nurbs::LaneRefusals::new();
        let mapped =
            curve_on_parameter_range(curve, [0., d], [2. * d, 3. * d], &"test", &mut refusals)
                .unwrap();
        let point = cadmpeg_ir::eval::curve_point(&mapped, 2.5 * d).unwrap();
        assert!((point.x - 0.5).abs() <= 8. * f64::EPSILON);
    }
}
