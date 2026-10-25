// SPDX-License-Identifier: Apache-2.0

use crate::eval::admission::EvaluationAdmission::Standard;
use crate::eval::{decode::pcurve_uv, pcurve_tangent};
use crate::geometry::pcurve::{
    OffsetPcurve, PcurveGeometry, PcurveNurbs, PlacedPcurve, PolarNurbsPole, PolarPcurveNurbs,
    TrimmedPcurve,
};
use crate::math::Point2;
use crate::transform::Transform2;

fn poles() -> Vec<Point2> {
    vec![
        Point2::new(1.0, 1.0),
        Point2::new(2.0, 1.0),
        Point2::new(2.0, 2.0),
        Point2::new(1.0, 2.0),
        Point2::new(1.0, 1.0),
        Point2::new(2.0, 1.0),
        Point2::new(2.0, 2.0),
    ]
}

fn cubic(period: f64) -> PcurveGeometry {
    PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            3,
            (-3..=7)
                .map(|index| f64::from(index) * period / 4.0)
                .collect::<Vec<_>>(),
            poles(),
            None,
            true,
        )
        .unwrap(),
    }
}

fn assert_same(first: Point2, second: Point2, tolerance: f64) {
    for (first, second) in [(first.u, second.u), (first.v, second.v)] {
        assert!((first - second).abs() <= tolerance, "{first} != {second}");
    }
}

#[test]
fn periodic_cubic_pcurve_points_and_derivatives_wrap_across_seams() {
    for (period, range) in [
        (
            0.005_123_231_207_432_89,
            [0.002_564_023_778_11, 0.007_687_254_985_543],
        ),
        (
            0.005_118_414_858_650_36,
            [0.002_559_207_429_328, 0.007_677_622_287_978],
        ),
    ] {
        let basis = cubic(period);
        let placed = PcurveGeometry::Transformed(
            PlacedPcurve::try_new(
                Box::new(basis.clone()),
                Transform2::affine([[0.0, -2.0, 10.0], [3.0, 0.0, -5.0]]).unwrap(),
            )
            .unwrap(),
        );
        let offset =
            PcurveGeometry::Offset(OffsetPcurve::try_new(0.1, Box::new(placed.clone())).unwrap());
        let trimmed = PcurveGeometry::Trimmed(
            TrimmedPcurve::try_new(range, true, Box::new(basis.clone())).unwrap(),
        );
        for geometry in [basis, placed, offset, trimmed] {
            for parameter in [0.0, period, 0.125 * period, range[0], range[1]] {
                let expected_point = pcurve_uv(Standard, &geometry, parameter).unwrap().get();
                let expected_tangent = pcurve_tangent(Standard, &geometry, parameter)
                    .unwrap()
                    .get();
                for turns in [-4, -1, 1, 5] {
                    let shifted = parameter + f64::from(turns) * period;
                    assert_same(
                        pcurve_uv(Standard, &geometry, shifted).unwrap().get(),
                        expected_point,
                        1.0e-11,
                    );
                    assert_same(
                        pcurve_tangent(Standard, &geometry, shifted).unwrap().get(),
                        expected_tangent,
                        1.0e-8,
                    );
                }
            }
        }
    }
}

#[test]
fn periodic_polar_cubic_pcurve_wraps_its_radial_and_axial_differentials() {
    let period = 0.005_123_231_207_432_89;
    let geometry = PcurveGeometry::PolarNurbs {
        nurbs: PolarPcurveNurbs::from_lanes(
            3,
            (-3..=7)
                .map(|index| f64::from(index) * period / 4.0)
                .collect::<Vec<_>>(),
            poles()
                .into_iter()
                .map(|radial| PolarNurbsPole {
                    radial,
                    axial: radial.u + 2.0 * radial.v,
                })
                .collect(),
            Some(vec![1.0; 7]),
            true,
        )
        .unwrap(),
    };
    for parameter in [0.0, period, period * 0.3] {
        for turns in [-3, 2] {
            let shifted = parameter + f64::from(turns) * period;
            assert_same(
                pcurve_uv(Standard, &geometry, shifted).unwrap().get(),
                pcurve_uv(Standard, &geometry, parameter).unwrap().get(),
                1.0e-11,
            );
            assert_same(
                pcurve_tangent(Standard, &geometry, shifted).unwrap().get(),
                pcurve_tangent(Standard, &geometry, parameter)
                    .unwrap()
                    .get(),
                1.0e-8,
            );
        }
    }
}

#[test]
fn periodic_pcurve_seams_choose_the_same_one_sided_derivative() {
    let knots = vec![-1.0, 0.0, 1.0, 2.0, 3.0];
    let nurbs = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            knots.clone(),
            vec![
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 0.0),
            ],
            None,
            true,
        )
        .unwrap(),
    };
    let polar = PcurveGeometry::PolarNurbs {
        nurbs: PolarPcurveNurbs::from_lanes(
            1,
            knots,
            vec![
                PolarNurbsPole {
                    radial: Point2::new(1.0, 1.0),
                    axial: 0.0,
                },
                PolarNurbsPole {
                    radial: Point2::new(2.0, 1.0),
                    axial: 1.0,
                },
                PolarNurbsPole {
                    radial: Point2::new(1.0, 1.0),
                    axial: 0.0,
                },
            ],
            None,
            true,
        )
        .unwrap(),
    };
    for (geometry, tangent) in [
        (nurbs, Point2::new(1.0, 0.0)),
        (polar, Point2::new(-0.5, 1.0)),
    ] {
        let point = pcurve_uv(Standard, &geometry, 0.0).unwrap().get();
        for parameter in [-4.0, -2.0, 0.0, 2.0, 4.0, 6.0] {
            assert_same(
                pcurve_uv(Standard, &geometry, parameter).unwrap().get(),
                point,
                1.0e-12,
            );
            assert_same(
                pcurve_tangent(Standard, &geometry, parameter)
                    .unwrap()
                    .get(),
                tangent,
                1.0e-12,
            );
        }
    }
}
