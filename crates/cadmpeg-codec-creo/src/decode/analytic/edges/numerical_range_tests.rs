// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;
const POINT_FIT_TOLERANCE: f64 = 1e-9;
use cadmpeg_ir::math::Point3;

#[test]
fn numerical_0922_inverse_keeps_both_branches() {
    for d in [1., SMALL_PARAMETER_DOMAIN] {
        let n = NurbsCurve::from_lanes(
            1,
            vec![0., 0., 0.5 * d, d, d],
            vec![
                Point3::new(-1., 0., 0.),
                Point3::new(1., 0., 0.),
                Point3::new(-1., 0., 0.),
            ],
            None,
            false,
        )
        .expect("valid folded degree-one curve");
        let g = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(n.clone()));
        let result =
            degree_one_nurbs_point_parameter(&g, &n, [0., 0., 0.], [0., d], POINT_FIT_TOLERANCE);
        println!("Creo folded segment domain {d:e}: {result:?}");
        assert_eq!(result, None);
    }
}
#[test]
fn numerical_0922_finite_knot_domain_reverses() {
    for d in [[0., 1.], [1e308, 1.1e308]] {
        let mut c = NurbsCurve::from_lanes(
            1,
            vec![d[0], d[0], d[1], d[1]],
            vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
            None,
            false,
        )
        .expect("valid translated degree-one curve");
        let result = reverse_nonperiodic_nurbs(&mut c, d);
        println!("Creo reverse finite line domain{d:?}: {result:?}");
        assert_eq!(result, Some(()));
        assert_eq!(c.knots(), &[d[0], d[0], d[1], d[1]]);
        assert_eq!(c.control_points()[0], Point3::new(1., 0., 0.));
    }
}

#[test]
fn numerical_audit_full_edge_survives_small_knot_domain() {
    for d in [1., 1e-14] {
        let c = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(
                1,
                vec![0., 0., d, d],
                vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
                None,
                false,
            )
            .expect("valid degree-one carrier"),
        ));
        assert_eq!(
            nonperiodic_nurbs_edge_parameter_range(&c, [[0., 0., 0.], [1., 0., 0.]]),
            Some([0., d])
        );
    }
}
