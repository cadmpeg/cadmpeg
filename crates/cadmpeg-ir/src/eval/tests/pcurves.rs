// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn analytic_pcurves_preserve_angular_parameterization() {
    let circle = PcurveGeometry::Circle(
        crate::geometry::CirclePcurve::try_new(
            Point2::new(2.0, 3.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, -1.0),
            4.0,
        )
        .unwrap(),
    );
    let ellipse = PcurveGeometry::Ellipse(
        crate::geometry::EllipsePcurve::try_new(
            Point2::new(2.0, 3.0),
            Point2::new(0.0, 1.0),
            Point2::new(-1.0, 0.0),
            4.0,
            2.0,
        )
        .unwrap(),
    );
    let polar = PcurveGeometry::PolarHarmonic(
        crate::geometry::PolarHarmonicPcurve::try_new(
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
        nurbs: crate::geometry::PolarPcurveNurbs::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                crate::geometry::PolarNurbsPole {
                    radial: Point2::new(2.0, 0.0),
                    axial: 3.0,
                },
                crate::geometry::PolarNurbsPole {
                    radial: Point2::new(2.0, 2.0),
                    axial: 4.0,
                },
                crate::geometry::PolarNurbsPole {
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
        crate::geometry::SphericalGreatCirclePcurve::try_new(0.25, 0.5, 1.0, -0.75).unwrap(),
    );
    let point = pcurve_uv(&geometry, 1.5).expect("great-circle pcurve evaluates");
    assert_eq!(point.u, 1.0);
    assert_eq!(point.v, (-0.75_f64).atan());
}

#[test]
fn general_harmonic_pcurves_evaluate_their_vector_coefficients() {
    let harmonic = PcurveGeometry::Harmonic(
        crate::geometry::HarmonicPcurve::try_new(
            Point2::new(2.0, 3.0),
            Point2::new(4.0, -1.0),
            Point2::new(2.0, 5.0),
        )
        .unwrap(),
    );
    let hyperbolic = PcurveGeometry::Hyperbolic(
        crate::geometry::HyperbolicPcurve::try_new(
            Point2::new(-3.0, 7.0),
            Point2::new(2.5, -4.0),
            Point2::new(1.5, 0.75),
        )
        .unwrap(),
    );
    let angle = std::f64::consts::FRAC_PI_3;
    assert_eq!(
        pcurve_uv(&harmonic, angle),
        Some(Point2::new(
            2.0 + 4.0 * angle.cos() + 2.0 * angle.sin(),
            3.0 - angle.cos() + 5.0 * angle.sin(),
        ))
    );
    let parameter = 0.75_f64;
    assert_eq!(
        pcurve_uv(&hyperbolic, parameter),
        Some(Point2::new(
            -3.0 + 2.5 * parameter.cosh() + 1.5 * parameter.sinh(),
            7.0 - 4.0 * parameter.cosh() + 0.75 * parameter.sinh(),
        ))
    );
}

#[test]
fn transformed_pcurves_apply_the_map_to_all_differential_orders() {
    let geometry = PcurveGeometry::Transformed {
        basis: Box::new(PcurveGeometry::Parabola(
            crate::geometry::ParabolaPcurve::try_new(
                Point2::new(1.0, 2.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                0.5,
            )
            .unwrap(),
        )),
        transform: Transform2::affine([[0.0, -2.0, 10.0], [2.0, 0.0, 20.0]])
            .expect("affine transform"),
    };

    assert_eq!(pcurve_uv(&geometry, 2.0), Some(Point2::new(2.0, 26.0)));
    assert_eq!(pcurve_tangent(&geometry, 2.0), Some(Point2::new(-2.0, 4.0)));
    let differential =
        pcurve_uv_differential_inner(&geometry, 2.0, 0).expect("transformed pcurve differential");
    assert_eq!(differential.acceleration, Some(Point2::new(0.0, 2.0)));
}

#[test]
fn signed_offset_pcurves_use_the_exact_left_normal() {
    let line = PcurveGeometry::Offset(
        crate::geometry::OffsetPcurve::try_new(
            2.0,
            Box::new(PcurveGeometry::Line(
                crate::geometry::LinePcurve::try_new(Point2::new(1.0, 2.0), Point2::new(3.0, 4.0))
                    .unwrap(),
            )),
        )
        .unwrap(),
    );
    let circle = PcurveGeometry::Offset(
        crate::geometry::OffsetPcurve::try_new(
            1.0,
            Box::new(PcurveGeometry::Circle(
                crate::geometry::CirclePcurve::try_new(
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
    assert_eq!(pcurve_uv(&circle, 0.0), Some(Point2::new(3.0, 0.0)));
    assert_eq!(pcurve_tangent(&line, 0.5), Some(Point2::new(3.0, 4.0)));
    assert_eq!(pcurve_tangent(&circle, 0.0), Some(Point2::new(0.0, 3.0)));

    let rational_arc = PcurveGeometry::Offset(
        crate::geometry::OffsetPcurve::try_new(
            0.25,
            Box::new(PcurveGeometry::Nurbs {
                nurbs: crate::geometry::PcurveNurbs::from_lanes(
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
        crate::geometry::OffsetPcurve::try_new(1.0, Box::new(line)).unwrap(),
    );
    let nested_point = pcurve_uv(&nested, 0.5).expect("nested offset point");
    assert!((nested_point.u - 0.1).abs() < 1.0e-12);
    assert!((nested_point.v - 5.8).abs() < 1.0e-12);
    assert_eq!(pcurve_tangent(&nested, 0.5), None);
}
