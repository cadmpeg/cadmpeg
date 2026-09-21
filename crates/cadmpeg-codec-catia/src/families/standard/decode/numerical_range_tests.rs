// SPDX-License-Identifier: Apache-2.0

use super::*;

const SMALL_PARAMETER_DOMAIN: f64 = 1e-12;

#[test]
fn numerical_0922_quintic_keeps_both_branches() {
    for d in [1., SMALL_PARAMETER_DOMAIN] {
        let mut poles = (0..6)
            .map(|i| Point3::new(-1. + 2. * i as f64 / 5., 0., 0.))
            .collect::<Vec<_>>();
        poles.extend((0..6).map(|i| Point3::new(1. - 2. * i as f64 / 5., 0., 0.)));
        let mut knots = vec![0.; 6];
        knots.extend(vec![0.5 * d; 6]);
        knots.extend(vec![d; 6]);
        let n = NurbsCurve::from_lanes(5, knots, poles, None, false).unwrap();
        let result = standard_limit_curve_point_parameter(&n, Point3::new(0., 0., 0.), 2e-3);
        println!("CATIA folded quintic locus d{d:e}: {result:?}");
        assert_eq!(result, None);
    }
}
#[test]
fn numerical_0922_finite_bezier_midpoint() {
    for x in [0., 1e308] {
        let poles = (0..6)
            .map(|i| Point3::new(x, i as f64 / 5., 0.))
            .collect::<Vec<_>>();
        let mut knots = vec![0.; 6];
        knots.extend(vec![1.; 6]);
        let n = NurbsCurve::from_lanes(5, knots, poles, None, false).unwrap();
        let result = standard_limit_curve_point_parameter(&n, Point3::new(x, 0.5, 0.), 2e-3);
        println!("CATIA straight quintic at x{x:e} midpoint: {result:?}");
        assert_eq!(result, Some(0.5));
    }
}
