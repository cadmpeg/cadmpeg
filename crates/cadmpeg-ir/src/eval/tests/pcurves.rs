// SPDX-License-Identifier: Apache-2.0

use crate::eval::pcurve_tangent;
use crate::eval::pcurve_uv;
use crate::eval::pcurve_uv_differential;
use crate::geometry::pcurve::PcurveGeometry;
use crate::math::Point2;
use crate::transform::Transform2;

#[test]
fn analytic_pcurves_preserve_angular_parameterization() {
    let circle = PcurveGeometry::Circle(
        crate::geometry::pcurve::CirclePcurve::try_new(
            Point2::new(2.0, 3.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, -1.0),
            4.0,
        )
        .unwrap(),
    );
    let ellipse = PcurveGeometry::Ellipse(
        crate::geometry::pcurve::EllipsePcurve::try_new(
            Point2::new(2.0, 3.0),
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 0.0),
            4.0,
            2.0,
        )
        .unwrap(),
    );
    let polar = PcurveGeometry::PolarHarmonic(
        crate::geometry::pcurve::PolarHarmonicPcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(2.0, 0.0),
            Point2::new(0.0, 2.0),
            3.0,
            4.0,
            0.0,
        )
        .unwrap(),
    );
    let polar_nurbs = PcurveGeometry::PolarNurbs {
        nurbs: crate::geometry::pcurve::PolarPcurveNurbs::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                crate::geometry::pcurve::PolarNurbsPole {
                    radial: Point2::new(2.0, 0.0),
                    axial: 3.0,
                },
                crate::geometry::pcurve::PolarNurbsPole {
                    radial: Point2::new(2.0, 2.0),
                    axial: 4.0,
                },
                crate::geometry::pcurve::PolarNurbsPole {
                    radial: Point2::new(0.0, 2.0),
                    axial: 5.0,
                },
            ],
            Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            false,
        )
        .unwrap(),
    };

    let circle_tangent =
        pcurve_tangent(&circle, std::f64::consts::FRAC_PI_2).expect("circle tangent");
    let circle = pcurve_uv(&circle, std::f64::consts::FRAC_PI_2).expect("circle evaluates");
    let ellipse = pcurve_uv(&ellipse, std::f64::consts::FRAC_PI_2).expect("ellipse evaluates");
    let polar = pcurve_uv(&polar, std::f64::consts::FRAC_PI_2).expect("polar curve evaluates");
    let polar_nurbs = pcurve_uv(&polar_nurbs, 0.5).expect("polar NURBS evaluates");
    assert!((circle.u - 2.0).abs() < 1.0e-12 && (circle.v + 1.0).abs() < 1.0e-12);
    assert!((circle_tangent.u + 4.0).abs() < 1.0e-12 && circle_tangent.v.abs() < 1.0e-12);
    assert!(ellipse.u.abs() < 1.0e-12 && (ellipse.v - 3.0).abs() < 1.0e-12);
    assert!((polar.u - std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!((polar.v - 3.0).abs() < 1.0e-12);
    assert!((polar_nurbs.u - std::f64::consts::FRAC_PI_4).abs() < 1.0e-12);
    assert!((polar_nurbs.v - 4.0).abs() < 1.0e-12);
}

#[test]
fn spherical_great_circle_pcurve_preserves_affine_source_parameterization() {
    let geometry = PcurveGeometry::SphericalGreatCircle(
        crate::geometry::pcurve::SphericalGreatCirclePcurve::try_new(0.25, 0.5, 1.0, -0.75)
            .unwrap(),
    );
    let point = pcurve_uv(&geometry, 1.5).expect("great-circle pcurve evaluates");
    assert_eq!(point.u, 1.0);
    assert_eq!(point.v, (-0.75_f64).atan());
}

#[test]
fn general_harmonic_pcurves_evaluate_their_vector_coefficients() {
    let harmonic = PcurveGeometry::Harmonic(
        crate::geometry::pcurve::HarmonicPcurve::try_new(
            Point2::new(2.0, 3.0),
            Point2::new(4.0, -1.0),
            Point2::new(2.0, 5.0),
        )
        .unwrap(),
    );
    let hyperbolic = PcurveGeometry::Hyperbolic(
        crate::geometry::pcurve::HyperbolicPcurve::try_new(
            Point2::new(-3.0, 7.0),
            Point2::new(2.5, -4.0),
            Point2::new(1.5, 0.75),
        )
        .unwrap(),
    );
    let angle = std::f64::consts::FRAC_PI_3;
    assert_eq!(
        pcurve_uv(&harmonic, angle).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(
            2.0 + 4.0 * angle.cos() + 2.0 * angle.sin(),
            3.0 - angle.cos() + 5.0 * angle.sin(),
        ))
    );
    let parameter = 0.75_f64;
    assert_eq!(
        pcurve_uv(&hyperbolic, parameter).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(
            -3.0 + 2.5 * parameter.cosh() + 1.5 * parameter.sinh(),
            7.0 - 4.0 * parameter.cosh() + 0.75 * parameter.sinh(),
        ))
    );
}

#[test]
fn transformed_pcurves_apply_the_map_to_all_differential_orders() {
    let geometry = PcurveGeometry::Transformed(
        crate::geometry::pcurve::PlacedPcurve::try_new(
            Box::new(PcurveGeometry::Parabola(
                crate::geometry::pcurve::ParabolaPcurve::try_new(
                    Point2::new(1.0, 2.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, 1.0),
                    0.5,
                )
                .unwrap(),
            )),
            Transform2::affine([[0.0, -2.0, 10.0], [2.0, 0.0, 20.0]]).expect("affine transform"),
        )
        .expect("placed pcurve"),
    );

    assert_eq!(
        pcurve_uv(&geometry, 2.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(2.0, 26.0))
    );
    assert_eq!(
        pcurve_tangent(&geometry, 2.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(-2.0, 4.0))
    );
    let differential =
        pcurve_uv_differential(&geometry, 2.0).expect("transformed pcurve differential");
    assert_eq!(
        differential
            .acceleration
            .map(crate::units::FinitePoint2::get),
        Some(Point2::new(0.0, 2.0))
    );
}

#[test]
fn signed_offset_pcurves_use_the_exact_left_normal() {
    let line = PcurveGeometry::Offset(
        crate::geometry::pcurve::OffsetPcurve::try_new(
            2.0,
            Box::new(PcurveGeometry::Line(
                crate::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(1.0, 2.0),
                    Point2::new(3.0, 4.0),
                )
                .unwrap(),
            )),
        )
        .unwrap(),
    );
    let circle = PcurveGeometry::Offset(
        crate::geometry::pcurve::OffsetPcurve::try_new(
            1.0,
            Box::new(PcurveGeometry::Circle(
                crate::geometry::pcurve::CirclePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, 1.0),
                    4.0,
                )
                .unwrap(),
            )),
        )
        .unwrap(),
    );
    let point = pcurve_uv(&line, 0.5).expect("regular line offset evaluates");
    assert!((point.u - 0.9).abs() < 1.0e-12);
    assert!((point.v - 5.2).abs() < 1.0e-12);
    assert_eq!(
        pcurve_uv(&circle, 0.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(3.0, 0.0))
    );
    assert_eq!(
        pcurve_tangent(&line, 0.5).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(3.0, 4.0))
    );
    assert_eq!(
        pcurve_tangent(&circle, 0.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(0.0, 3.0))
    );

    let rational_arc = PcurveGeometry::Offset(
        crate::geometry::pcurve::OffsetPcurve::try_new(
            0.25,
            Box::new(PcurveGeometry::Nurbs {
                nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    vec![
                        Point2::new(1.0, 0.0),
                        Point2::new(1.0, 1.0),
                        Point2::new(0.0, 1.0),
                    ],
                    Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
                    false,
                )
                .unwrap(),
            }),
        )
        .unwrap(),
    );
    for parameter in [0.0, 0.5, 1.0] {
        let point =
            pcurve_uv(&rational_arc, parameter).expect("regular rational NURBS offset evaluates");
        let tangent = pcurve_tangent(&rational_arc, parameter).expect("rational offset tangent");
        assert!((point.u.hypot(point.v) - 0.75).abs() < 1.0e-12);
        assert!((point.u * tangent.u + point.v * tangent.v).abs() < 1.0e-12);
    }

    let nested = PcurveGeometry::Offset(
        crate::geometry::pcurve::OffsetPcurve::try_new(1.0, Box::new(line)).unwrap(),
    );
    let nested_point = pcurve_uv(&nested, 0.5).expect("nested offset point");
    assert!((nested_point.u - 0.1).abs() < 1.0e-12);
    assert!((nested_point.v - 5.8).abs() < 1.0e-12);
    assert_eq!(
        pcurve_tangent(&nested, 0.5),
        Err(crate::eval::EvaluationFailure::NoValue)
    );
}

#[test]
fn evaluation_extrapolates_past_a_declared_domain_for_every_carrier() {
    // Degree 1 over two poles on [0, 1]: the knot interval is the declared
    // domain and the curve is the segment (0, 0) to (1, 2).
    let nurbs = PcurveGeometry::Nurbs {
        nurbs: crate::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 2.0)],
            None,
            false,
        )
        .unwrap(),
    };
    let trimmed = PcurveGeometry::Trimmed(
        crate::geometry::pcurve::TrimmedPcurve::try_new(
            [0.25, 0.75],
            true,
            Box::new(nurbs.clone()),
        )
        .unwrap(),
    );

    // The bare NURBS extrapolates its end span on both sides of the knot
    // interval, so out-of-domain is not a refusal anywhere in this evaluator.
    assert_eq!(
        pcurve_uv(&nurbs, 0.5).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(0.5, 1.0))
    );
    assert_eq!(
        pcurve_uv(&nurbs, 2.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(2.0, 4.0))
    );
    assert_eq!(
        pcurve_uv(&nurbs, -1.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(-1.0, -2.0))
    );
    assert_eq!(
        pcurve_tangent(&nurbs, 2.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(1.0, 2.0))
    );

    // The trim declares [0.25, 0.75] and reparameterizes nothing, so it
    // answers exactly what its basis answers at every parameter, inside the
    // interval and outside it.
    for parameter in [-1.0, 0.1, 0.25, 0.5, 0.75, 2.0] {
        assert_eq!(pcurve_uv(&trimmed, parameter), pcurve_uv(&nurbs, parameter));
        assert_eq!(
            pcurve_tangent(&trimmed, parameter),
            pcurve_tangent(&nurbs, parameter)
        );
    }
}

#[test]
fn an_offset_pcurve_whose_point_overflows_reports_the_non_finite_point() {
    let offset = |distance: f64| {
        PcurveGeometry::Offset(
            crate::geometry::pcurve::OffsetPcurve::try_new(
                distance,
                Box::new(PcurveGeometry::Line(
                    crate::geometry::pcurve::LinePcurve::try_new(
                        Point2::new(f64::MAX, 0.0),
                        Point2::new(0.0, 1.0),
                    )
                    .unwrap(),
                )),
            )
            .unwrap(),
        )
    };
    // The left normal of the upward line is -u, so a negative distance moves
    // the finite basis point past the largest finite u.
    assert_eq!(
        pcurve_uv(&offset(-f64::MAX), 0.0),
        Err(crate::eval::EvaluationFailure::NonFinite(Point2::new(
            f64::INFINITY,
            0.0
        )))
    );
    assert_eq!(
        pcurve_uv(&offset(f64::MAX), 0.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(0.0, 0.0))
    );
    assert_eq!(
        pcurve_tangent(&offset(f64::MAX), 0.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(0.0, 1.0))
    );
}
