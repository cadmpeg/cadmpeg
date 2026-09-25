// SPDX-License-Identifier: Apache-2.0
use super::{
    angularly_equal, elliptical_arc_nurbs, parabolic_arc_nurbs, quarter_turn_spans,
    ANGULAR_TOLERANCE,
};
use cadmpeg_ir::eval::nurbs_curve_point_at;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::PositiveLength;

/// A sweep that is a whole number of quarter turns must not gain a span from
/// last-place noise. Expectations come from the geometry: a quarter turn is
/// one rational span, a semicircle two, a full turn four.
#[test]
fn an_exact_quarter_turn_multiple_is_stable_under_last_place_noise() {
    for (quarters, expected) in [(1.0, 1), (2.0, 2), (3.0, 3), (4.0, 4)] {
        let sweep = std::f64::consts::FRAC_PI_2 * quarters;
        for bits in [sweep.to_bits() - 1, sweep.to_bits(), sweep.to_bits() + 1] {
            assert_eq!(
                quarter_turn_spans(f64::from_bits(bits)),
                expected,
                "{quarters} quarter turn(s) at bits {bits:#x}"
            );
        }
    }
}

/// A sweep between two boundaries still rounds up, so a span never spans
/// more than a quarter turn.
#[test]
fn a_partial_quarter_turn_rounds_up() {
    for (quarters, expected) in [(0.5, 1), (1.5, 2), (2.5, 3), (3.5, 4)] {
        let sweep = std::f64::consts::FRAC_PI_2 * quarters;
        assert_eq!(
            quarter_turn_spans(sweep),
            expected,
            "{quarters} quarter turns"
        );
    }
}

/// A vanishing sweep still yields one span rather than none, which would
/// produce a curve with no control points.
#[test]
fn a_vanishing_sweep_still_yields_one_span() {
    assert_eq!(quarter_turn_spans(f64::MIN_POSITIVE), 1);
}

#[test]
fn angular_equality_has_one_inclusive_absolute_boundary() {
    assert!(angularly_equal(0.0, ANGULAR_TOLERANCE));
    assert!(angularly_equal(
        std::f64::consts::TAU,
        std::f64::consts::TAU - ANGULAR_TOLERANCE
    ));
    assert!(!angularly_equal(0.0, ANGULAR_TOLERANCE * 1.01));
}

#[test]
fn an_ellipse_arc_has_exact_rational_quadratic_points() {
    let curve = elliptical_arc_nurbs(
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        PositiveLength::new(4.0).expect("positive major radius"),
        PositiveLength::new(2.0).expect("positive minor radius"),
        [0.0, std::f64::consts::FRAC_PI_2],
    )
    .expect("arc lanes pair")
    .expect("valid ellipse arc");
    assert_eq!(curve.degree(), 2);
    assert_eq!(curve.control_points().len(), 3);
    for (parameter, expected) in [
        (0.0, Point3::new(5.0, 2.0, 3.0)),
        (
            std::f64::consts::FRAC_PI_4,
            Point3::new(1.0 + 2.0 * 2.0_f64.sqrt(), 2.0 + 2.0_f64.sqrt(), 3.0),
        ),
        (std::f64::consts::FRAC_PI_2, Point3::new(1.0, 4.0, 3.0)),
    ] {
        let actual = nurbs_curve_point_at(&curve, parameter).expect("ellipse NURBS evaluates");
        assert!(
            actual.distance(expected) <= 1.0e-12,
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn a_parabola_arc_has_exact_quadratic_points() {
    let curve = parabolic_arc_nurbs(
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
        PositiveLength::new(2.0).expect("positive focal distance"),
        [-1.0, 3.0],
    )
    .expect("arc lanes pair")
    .expect("valid parabola arc");
    for (parameter, expected) in [
        (-1.0, Point3::new(3.0, -2.0, 3.0)),
        (1.0, Point3::new(3.0, 6.0, 3.0)),
        (3.0, Point3::new(19.0, 14.0, 3.0)),
    ] {
        let actual = nurbs_curve_point_at(&curve, parameter).expect("parabola NURBS evaluates");
        assert!(
            actual.distance(expected) <= 1.0e-12,
            "{actual:?} != {expected:?}"
        );
    }
}

#[test]
fn audit_regression_parabola_keeps_finite_scaled_coordinates() {
    let curve = parabolic_arc_nurbs(
        Point3::new(0., 0., 0.),
        Vector3::new(0., 0., 1.),
        Vector3::new(1., 0., 0.),
        PositiveLength::new(1e308).expect("positive focal distance"),
        [1e-100, 2e-100],
    )
    .expect("valid quadratic NURBS")
    .expect("finite parabola interval");
    let points = curve.control_points();
    for (point, expected) in [(&points[0], 2e208), (&points[2], 4e208)] {
        assert!((point.y / expected - 1.).abs() <= 8. * f64::EPSILON);
        assert!(point.is_finite());
    }
}
