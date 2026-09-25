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
        let n = NurbsCurve::from_lanes(5, knots, poles, None, false).expect("valid quintic curve");
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
        let n = NurbsCurve::from_lanes(5, knots, poles, None, false).expect("valid quintic curve");
        let result = standard_limit_curve_point_parameter(&n, Point3::new(x, 0.5, 0.), 2e-3);
        println!("CATIA straight quintic at x{x:e} midpoint: {result:?}");
        assert_eq!(result, Some(0.5));
    }
}

#[test]
fn standard_limit_curve_finds_interior_point_on_wide_finite_domain() {
    let poles = (0..6)
        .map(|index| Point3::new(0.0, index as f64 / 5.0, 0.0))
        .collect::<Vec<_>>();
    let mut knots = vec![-f64::MAX; 6];
    knots.extend(vec![f64::MAX; 6]);
    let curve =
        NurbsCurve::from_lanes(5, knots, poles, None, false).expect("wide finite quintic domain");
    let parameter = standard_limit_curve_point_parameter(&curve, Point3::new(0.0, 0.2, 0.0), 2e-3)
        .expect("interior point parameter");
    assert!((parameter / f64::MAX + 0.6).abs() <= 0.01);
}

use cadmpeg_ir::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
fn audit_plane(d: [f64; 2], s: f64) -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![d[0], d[0], d[1], d[1]], false),
        NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., s, 0.)],
                vec![Point3::new(s, 0., 0.), Point3::new(s, s, 0.)],
            ],
            None,
        ),
        false,
    )
    .expect("valid bilinear surface")
}
#[test]
fn numerical_0922b_surface_membership_wide_chart() {
    for d in [[0., 1.], [-1e308, 1e308]] {
        let r = point_on_nurbs_surface(Point3::new(0.3, 0.7, 0.), &audit_plane(d, 1.));
        println!("CATIA plane chart{d:?}: {r:?}");
        assert_eq!(r, Some(true));
    }
}
#[test]
fn numerical_0922b_surface_membership_large_plane() {
    for scale in [1., 1e200] {
        let r = point_on_nurbs_surface(
            Point3::new(0.3 * scale, 0.7 * scale, 0.),
            &audit_plane([0., 1.], scale),
        );
        println!("CATIA plane scale{scale:e}: {r:?}");
        assert_eq!(r, Some(true));
    }
}
