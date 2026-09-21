// SPDX-License-Identifier: Apache-2.0
use super::super::{
    circular_arc_partials, curve_point_solved, curve_tangent_solved, minor_circular_arc_point,
    pcurve_tangent, pcurve_uv, ContactTrackDifferential,
};
use crate::geometry::analytic::{CircleCurve, HyperbolaCurve, ParabolaCurve};
use crate::geometry::pcurve::{HyperbolaPcurve, ParabolaPcurve};
use crate::geometry::{pcurve::PcurveGeometry, SolvedCurveGeometry};
use crate::math::{Point2, Point3, Vector3};
const EPS_RELATIVE: f64 = 1024.0 * f64::EPSILON;
#[test]
fn numerical_ranges_parabola_point_and_tangent_stay_finite() {
    let curve = SolvedCurveGeometry::Parabola(
        ParabolaCurve::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            1e308,
        )
        .unwrap(),
    );
    let point = curve_point_solved(&curve, 0.5).unwrap();
    assert_eq!(point, Point3::new(2.5e307, 1e308, 0.));
    let pcurve = PcurveGeometry::Parabola(
        ParabolaPcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(1., 0.),
            Point2::new(0., 1.),
            1e200,
        )
        .unwrap(),
    );
    let point = pcurve_uv(&pcurve, 1e200).unwrap();
    assert!((point.u / 2.5e199 - 1.).abs() < EPS_RELATIVE);
    assert_eq!(point.v, 1e200);
    assert_eq!(pcurve_tangent(&pcurve, 1e200), Some(Point2::new(0.5, 1.)));
}
#[test]
fn numerical_ranges_hyperbolas_scale_before_exponentiation_overflows() {
    let curve = SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(1., 0., 0.),
            1e-10,
            1e-10,
        )
        .unwrap(),
    );
    let pcurve = PcurveGeometry::Hyperbola(
        HyperbolaPcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(1., 0.),
            Point2::new(0., 1.),
            1e-10,
            1e-10,
        )
        .unwrap(),
    );
    for t in [-720.0_f64, 720.] {
        let expected = (t.abs() + 1e-10_f64.ln()).exp() * 0.5;
        let point = curve_point_solved(&curve, t).unwrap();
        assert!((point.x / expected - 1.).abs() < EPS_RELATIVE);
        assert!((point.y / (t.signum() * expected) - 1.).abs() < EPS_RELATIVE);
        assert!(curve_tangent_solved(&curve, t).unwrap().is_finite());
        assert!(pcurve_uv(&pcurve, t).unwrap().is_finite());
        assert!(pcurve_tangent(&pcurve, t).unwrap().is_finite());
    }
}
#[test]
fn numerical_ranges_offset_keeps_finite_cancellation() {
    let c = std::f64::consts::FRAC_1_SQRT_2;
    let curve = SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(1e308, 0., 0.),
            Vector3::new(0., 0., 1.),
            Vector3::new(c, c, 0.),
            1.6e308,
        )
        .unwrap(),
    );
    let point = curve_point_solved(&curve, std::f64::consts::FRAC_PI_4).unwrap();
    assert!((point.x / 1e308 - 1.).abs() < EPS_RELATIVE);
    assert!((point.y / 1.6e308 - 1.).abs() < EPS_RELATIVE);
}
#[test]
fn numerical_ranges_shallow_arc_point_and_partials_keep_curvature() {
    let radius = 1e9;
    let angle = 1e-9_f64;
    let zero = Vector3::new(0., 0., 0.);
    let center = Point3::new(0., 0., 0.);
    let first = ContactTrackDifferential {
        point: Point3::new(radius, 0., 0.),
        tangent: zero,
        normal: Vector3::new(1., 0., 0.),
        normal_derivative: Some(zero),
    };
    let second = ContactTrackDifferential {
        point: Point3::new(radius * angle.cos(), radius * angle.sin(), 0.),
        tangent: zero,
        normal: Vector3::new(angle.cos(), angle.sin(), 0.),
        normal_derivative: Some(zero),
    };
    let point = minor_circular_arc_point(center, first.point, second.point, radius, 0.5).unwrap();
    assert!((point.y - 0.5).abs() < EPS_RELATIVE);
    let partials = circular_arc_partials(center, zero, &first, &second, radius, 0., 0.5).unwrap();
    assert!((partials.point.y - 0.5).abs() < EPS_RELATIVE);
    assert!((partials.du.y - 1.).abs() < EPS_RELATIVE);
    assert!((partials.du.x / (-0.5 * angle) - 1.).abs() < EPS_RELATIVE);
    assert_eq!(partials.dv, zero);
}

#[test]
fn numerical_ranges_hyperbolic_point_survives_unrepresentable_derivatives() {
    let curve = PcurveGeometry::Hyperbolic(
        crate::geometry::pcurve::HyperbolicPcurve::try_new(
            Point2::new(-1e308, 0.),
            Point2::new(1e308, 0.),
            Point2::new(1e308, 0.),
        )
        .unwrap(),
    );
    let point = pcurve_uv(&curve, 1.).unwrap();
    let expected = (1.0_f64.exp() - 1.) * 1e308;
    assert!((point.u / expected - 1.).abs() < EPS_RELATIVE);
    assert_eq!(point.v, 0.);
    assert_eq!(pcurve_tangent(&curve, 1.), None);
}

#[test]
fn numerical_ranges_nurbs_basis_supports_spans_wider_than_f64() {
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![-1e308, -1e308, 1e308, 1e308],
            vec![Point2::new(0., 0.), Point2::new(1., 0.)],
            None,
            false,
        )
        .unwrap(),
    };
    for (parameter, expected) in [(-1e308, 0.), (0., 0.5), (1e308, 1.)] {
        assert_eq!(
            pcurve_uv(&pcurve, parameter),
            Some(Point2::new(expected, 0.))
        );
        let tangent = pcurve_tangent(&pcurve, parameter).unwrap();
        assert!((tangent.u / 5e-309 - 1.).abs() < EPS_RELATIVE);
        assert_eq!(tangent.v, 0.);
    }
}
