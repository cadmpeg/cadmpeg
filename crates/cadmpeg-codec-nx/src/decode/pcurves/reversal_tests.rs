// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::geometry::Surface;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;

use crate::decode::pcurves::{orient_tolerant_intersection_pcurve, reverse_pcurve_over_range};

#[test]
fn reversed_nurbs_pcurve_preserves_the_selected_interval() {
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0],
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 2.0),
                Point2::new(3.0, 1.0),
            ],
            Some(vec![1.0, 2.0, 1.5]),
            false,
        )
        .unwrap(),
    };
    let range = [0.25, 1.75];
    let reversed = reverse_pcurve_over_range(&pcurve, range)
        .expect("reversed lanes pair")
        .expect("reversible NURBS pcurve");
    for parameter in [range[0], 0.5, 1.0, 1.5, range[1]] {
        let expected =
            cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1.0e-12);
        assert!((actual.v - expected.v).abs() < 1.0e-12);
    }
}

#[test]
fn reversed_symmetric_analytic_pcurves_preserve_the_selected_interval() {
    let carriers = [
        PcurveGeometry::Ellipse(
            cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
                Point2::new(2.0, 3.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                4.0,
                2.0,
            )
            .unwrap(),
        ),
        PcurveGeometry::Parabola(
            cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
                Point2::new(2.0, 3.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                0.75,
            )
            .unwrap(),
        ),
        PcurveGeometry::Hyperbola(
            cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::try_new(
                Point2::new(2.0, 3.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                4.0,
                2.0,
            )
            .unwrap(),
        ),
    ];
    let range = [-1.5, 1.5];
    for carrier in carriers {
        let reversed = reverse_pcurve_over_range(&carrier, range)
            .expect("reversed lanes pair")
            .expect("symmetric analytic pcurve is exactly reversible");
        for parameter in [-1.5, -0.75, 0.0, 0.75, 1.5] {
            let expected = cadmpeg_ir::eval::pcurve_uv(&carrier, -parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1.0e-12);
            assert!((actual.v - expected.v).abs() < 1.0e-12);
        }
    }
}

#[test]
fn reversed_analytic_conics_preserve_arbitrary_selected_intervals() {
    let carriers = [
        PcurveGeometry::Ellipse(
            cadmpeg_ir::geometry::pcurve::EllipsePcurve::try_new(
                Point2::new(2.0, 3.0),
                Point2::new(0.6, 0.8),
                Point2::new(-0.8, 0.6),
                4.0,
                2.0,
            )
            .unwrap(),
        ),
        PcurveGeometry::Hyperbola(
            cadmpeg_ir::geometry::pcurve::HyperbolaPcurve::try_new(
                Point2::new(-3.0, 5.0),
                Point2::new(0.8, -0.6),
                Point2::new(0.6, 0.8),
                2.5,
                1.25,
            )
            .unwrap(),
        ),
    ];
    let range = [0.25, 1.75];
    for carrier in carriers {
        let reversed = reverse_pcurve_over_range(&carrier, range)
            .expect("reversed lanes pair")
            .expect("a finite conic interval has an exact coefficient reflection");
        assert!(matches!(
            (&carrier, &reversed),
            (PcurveGeometry::Ellipse(_), PcurveGeometry::Harmonic(_))
                | (PcurveGeometry::Hyperbola(_), PcurveGeometry::Hyperbolic(_))
        ));
        for parameter in [0.25, 0.5, 1.0, 1.5, 1.75] {
            let expected =
                cadmpeg_ir::eval::pcurve_uv(&carrier, range[0] + range[1] - parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1.0e-12);
            assert!((actual.v - expected.v).abs() < 1.0e-12);
        }

        let reflected_twice = reverse_pcurve_over_range(&reversed, range)
            .expect("reversed lanes pair")
            .expect("general conic coefficients remain exactly reversible");
        for parameter in [0.25, 0.75, 1.25, 1.75] {
            let expected = cadmpeg_ir::eval::pcurve_uv(&carrier, parameter).unwrap();
            let actual = cadmpeg_ir::eval::pcurve_uv(&reflected_twice, parameter).unwrap();
            assert!((actual.u - expected.u).abs() < 1.0e-12);
            assert!((actual.v - expected.v).abs() < 1.0e-12);
        }
    }
}

#[test]
fn reversed_parabola_preserves_an_arbitrary_selected_interval() {
    let pcurve = PcurveGeometry::Parabola(
        cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
            Point2::new(2.0, 3.0),
            Point2::new(0.6, 0.8),
            Point2::new(-0.8, 0.6),
            0.75,
        )
        .unwrap(),
    );
    let range = [0.25, 2.75];
    let reversed = reverse_pcurve_over_range(&pcurve, range)
        .expect("reversed lanes pair")
        .expect("a finite parabola interval has an exact quadratic reflection");
    assert!(matches!(
        &reversed,
        PcurveGeometry::Nurbs { nurbs }
            if nurbs.degree() == 2 && nurbs.weights().is_none() && !nurbs.periodic()
    ));
    for parameter in [0.25, 0.5, 1.0, 1.75, 2.5, 2.75] {
        let expected =
            cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1.0e-12);
        assert!((actual.v - expected.v).abs() < 1.0e-12);
    }

    let offset = PcurveGeometry::Offset(
        cadmpeg_ir::geometry::pcurve::OffsetPcurve::try_new(1.25, Box::new(pcurve.clone()))
            .unwrap(),
    );
    let PcurveGeometry::Offset(offset_pcurve) = reverse_pcurve_over_range(&offset, range)
        .expect("reversed lanes pair")
        .expect("offset parabola reflection closes recursively")
    else {
        panic!("reversed offset parabola");
    };
    let distance = offset_pcurve.distance();
    let basis = offset_pcurve.basis();
    assert_eq!(distance, -1.25);
    for parameter in [0.25, 1.0, 2.0, 2.75] {
        let expected =
            cadmpeg_ir::eval::pcurve_uv(&pcurve, range[0] + range[1] - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(basis, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1.0e-12);
        assert!((actual.v - expected.v).abs() < 1.0e-12);
    }
}

#[test]
fn reversed_offset_pcurve_reverses_its_basis_and_signed_side() {
    let pcurve = PcurveGeometry::Offset(
        cadmpeg_ir::geometry::pcurve::OffsetPcurve::try_new(
            2.5,
            Box::new(PcurveGeometry::Line(
                cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                    Point2::new(1.0, 3.0),
                    Point2::new(2.0, -1.0),
                )
                .unwrap(),
            )),
        )
        .unwrap(),
    );
    let reversed = reverse_pcurve_over_range(&pcurve, [2.0, 6.0])
        .expect("reversed lanes pair")
        .expect("offset construction is exactly reversible");
    let PcurveGeometry::Offset(offset_pcurve) = &reversed else {
        panic!("reversed offset");
    };
    let distance = offset_pcurve.distance();
    let basis = offset_pcurve.basis();
    assert_eq!(distance, -2.5);
    for parameter in [2.0, 3.0, 5.0, 6.0] {
        let expected_basis = cadmpeg_ir::eval::pcurve_uv(
            match &pcurve {
                PcurveGeometry::Offset(offset_pcurve) => {
                    let basis = offset_pcurve.basis();
                    basis
                }
                _ => unreachable!(),
            },
            8.0 - parameter,
        )
        .unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(basis, parameter).unwrap();
        assert_eq!(actual, expected_basis);
        let expected = cadmpeg_ir::eval::pcurve_uv(&pcurve, 8.0 - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&reversed, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1.0e-12);
        assert!((actual.v - expected.v).abs() < 1.0e-12);
    }

    let support = SurfaceId::mint("test:model:entity#nx:test:offset-orientation-support")
        .expect("identity grammar");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: support.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        source_object: None,
    });
    let first = cadmpeg_ir::eval::pcurve_uv(&pcurve, 2.0).unwrap();
    let second = cadmpeg_ir::eval::pcurve_uv(&pcurve, 6.0).unwrap();
    let oriented = orient_tolerant_intersection_pcurve(
        &ir,
        &CurveId::mint("test:model:entity#nx:test:unused-orientation-curve")
            .expect("identity grammar"),
        &support,
        &pcurve,
        [2.0, 6.0],
        [
            Point3::new(second.u, second.v, 0.0),
            Point3::new(first.u, first.v, 0.0),
        ],
        1.0e-12,
    )
    .expect("reversed lanes pair")
    .expect("offset endpoints select the reversed terminal branch");
    for parameter in [2.0, 3.0, 5.0, 6.0] {
        let expected = cadmpeg_ir::eval::pcurve_uv(&pcurve, 8.0 - parameter).unwrap();
        let actual = cadmpeg_ir::eval::pcurve_uv(&oriented, parameter).unwrap();
        assert!((actual.u - expected.u).abs() < 1.0e-12);
        assert!((actual.v - expected.v).abs() < 1.0e-12);
    }
}

#[test]
fn numerical_ranges_nurbs_reversal_avoids_reflection_sum_overflow() {
    let range = [1e308, 1.4e308];
    let pcurve = PcurveGeometry::Nurbs {
        nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![range[0], range[0], range[1], range[1]],
            vec![Point2::new(0., 0.), Point2::new(1., 1.)],
            None,
            false,
        )
        .unwrap(),
    };
    let reversed = reverse_pcurve_over_range(&pcurve, range).unwrap().unwrap();
    assert_eq!(
        cadmpeg_ir::eval::pcurve_uv(&reversed, range[0]),
        Some(Point2::new(1., 1.))
    );
    assert_eq!(
        cadmpeg_ir::eval::pcurve_uv(&reversed, range[1]),
        Some(Point2::new(0., 0.))
    );
    assert_eq!(
        reverse_pcurve_over_range(&reversed, range).unwrap(),
        Some(pcurve)
    );
}
#[test]
fn numerical_ranges_parabola_reversal_reuses_scaled_evaluation() {
    let pcurve = PcurveGeometry::Parabola(
        cadmpeg_ir::geometry::pcurve::ParabolaPcurve::try_new(
            Point2::new(0., 0.),
            Point2::new(1., 0.),
            Point2::new(0., 1.),
            1e200,
        )
        .unwrap(),
    );
    let range = [1e200, 2e200];
    let reversed = reverse_pcurve_over_range(&pcurve, range).unwrap().unwrap();
    for t in [0., 0.5, 1.] {
        // The reversed quadratic retains the original parameter domain.
        let point = cadmpeg_ir::eval::pcurve_uv(&reversed, (1. + t) * 1e200).unwrap();
        let expected = cadmpeg_ir::eval::pcurve_uv(&pcurve, (2. - t) * 1e200).unwrap();
        assert!((point.u / expected.u - 1.).abs() < 64. * f64::EPSILON);
        assert!((point.v / expected.v - 1.).abs() < 64. * f64::EPSILON);
    }
}

#[test]
fn analytic_reversal_preserves_finite_coefficients_at_extreme_parameters() {
    use cadmpeg_ir::geometry::pcurve::{HyperbolaPcurve, HyperbolicPcurve, LinePcurve};
    const RADIUS: f64 = 1e-10;
    let line = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(0.5, 0.0)).unwrap(),
    );
    let reversed = super::reverse_analytic_pcurve_over_range(&line, [1e308, 1.4e308]).unwrap();
    let PcurveGeometry::Line(reversed) = reversed else {
        panic!("expected line")
    };
    assert_eq!(*reversed.origin().as_raw(), Point2::new(1.2e308, 0.0));
    assert_eq!(*reversed.direction().as_raw(), Point2::new(-0.5, 0.0));
    for curve in [
        PcurveGeometry::Hyperbola(
            HyperbolaPcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
                RADIUS,
                RADIUS,
            )
            .unwrap(),
        ),
        PcurveGeometry::Hyperbolic(
            HyperbolicPcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(RADIUS, 0.0),
                Point2::new(0.0, RADIUS),
            )
            .unwrap(),
        ),
    ] {
        let reversed = super::reverse_analytic_pcurve_over_range(&curve, [359.0, 361.0]).unwrap();
        let PcurveGeometry::Hyperbolic(reversed) = reversed else {
            panic!("expected hyperbolic coefficients")
        };
        // exp(720)/2 * RADIUS, evaluated independently through logarithms.
        let expected = (720.0 + RADIUS.ln() - std::f64::consts::LN_2).exp();
        for component in [
            reversed.cosine().u,
            reversed.cosine().v,
            -reversed.sine().u,
            -reversed.sine().v,
        ] {
            assert!(component.is_finite());
            assert!((component / expected - 1.0).abs() < 512.0 * f64::EPSILON);
        }
    }
}
