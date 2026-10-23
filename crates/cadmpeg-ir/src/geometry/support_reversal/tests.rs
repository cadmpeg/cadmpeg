// SPDX-License-Identifier: Apache-2.0

use crate::eval::admission::EvaluationAdmission;
use crate::eval::decode::pcurve_uv;
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry, PcurveNurbs, PcurveNurbsPoles};
use crate::geometry::{
    DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide, SupportPcurve,
};
use crate::math::Point2;
use crate::units::FinitePoint2;

const EPS_REVERSED_POINT: f64 = 1.0e-12;

fn nurbs_pcurve() -> PcurveGeometry {
    let points = [(0.0, 0.0), (1.0, 2.0), (3.0, 1.0), (4.0, 3.0)]
        .into_iter()
        .map(|(u, v)| FinitePoint2::new(Point2::new(u, v)).expect("finite pole"))
        .collect();
    PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::new(
            2,
            vec![-1.0, -1.0, -1.0, 0.5, 2.0, 2.0, 2.0],
            PcurveNurbsPoles::Polynomial { points },
            false,
        )
        .expect("nurbs pcurve"),
    }
}

fn uv(geometry: &PcurveGeometry, parameter: f64) -> Point2 {
    pcurve_uv(EvaluationAdmission::Standard, geometry, parameter)
        .expect("finite pcurve point")
        .get()
}

#[test]
fn a_reversed_support_context_traverses_each_pcurve_as_minus_t() {
    let line = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(1.0, 1.0), Point2::new(0.5, -1.0)).expect("line"),
    );
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: None,
                pcurve: Some(SupportPcurve::new(nurbs_pcurve(), None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: Some(SupportPcurve::new(
                    line.clone(),
                    Some(DirectedParameterRange::new([3.0, 7.0]).expect("range")),
                )),
            },
        ],
        [-1.0, 2.0],
        [vec![-0.5, 1.5], vec![], vec![0.0]],
    )
    .expect("context");
    let reversed = context.reversed_about_zero().expect("reversible pcurves");

    assert_eq!(reversed.parameter_range().endpoints(), [-2.0, 1.0]);
    let discontinuities = crate::scalar::FiniteReal::raw_lanes(reversed.discontinuities());
    assert_eq!(discontinuities, [vec![-1.5, 0.5], vec![], vec![-0.0]]);

    let nurbs = &reversed.sides()[0]
        .pcurve
        .as_ref()
        .expect("pcurve")
        .geometry;
    let mapped = reversed.sides()[1].pcurve.as_ref().expect("pcurve");
    assert_eq!(
        mapped
            .parameter_range
            .map(DirectedParameterRange::endpoints),
        Some([-7.0, -3.0])
    );
    for parameter in [-1.0, 0.0, 0.5, 1.25, 2.0] {
        let original = uv(&nurbs_pcurve(), parameter);
        let mirrored = uv(nurbs, -parameter);
        assert!((original.u - mirrored.u).abs() < EPS_REVERSED_POINT);
        assert!((original.v - mirrored.v).abs() < EPS_REVERSED_POINT);
        let original = uv(&line, parameter);
        let mirrored = uv(&mapped.geometry, -parameter);
        assert!((original.u - mirrored.u).abs() < EPS_REVERSED_POINT);
        assert!((original.v - mirrored.v).abs() < EPS_REVERSED_POINT);
    }
    assert_eq!(reversed.reversed_about_zero(), Some(context));
}

#[test]
fn circle_and_harmonic_support_pcurves_reverse_about_zero() {
    use crate::geometry::pcurve::{CirclePcurve, HarmonicPcurve};
    let pcurves = [
        PcurveGeometry::Circle(
            CirclePcurve::try_new(
                Point2::new(1.0, 2.0),
                Point2::new(0.6, 0.8),
                Point2::new(-0.8, 0.6),
                1.5,
            )
            .expect("circle"),
        ),
        PcurveGeometry::Harmonic(
            HarmonicPcurve::try_new(
                Point2::new(0.5, -1.0),
                Point2::new(2.0, 0.5),
                Point2::new(-0.25, 1.5),
            )
            .expect("harmonic"),
        ),
    ];
    for pcurve in pcurves {
        let context = IntcurveSupportContext::try_new(
            [
                IntcurveSupportSide {
                    surface: None,
                    pcurve: Some(SupportPcurve::new(pcurve.clone(), None)),
                },
                IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                },
            ],
            [0.25, 2.5],
            std::array::from_fn(|_| Vec::new()),
        )
        .expect("context");
        let reversed = context.reversed_about_zero().expect("reversible pcurve");
        let mirrored = &reversed.sides()[0]
            .pcurve
            .as_ref()
            .expect("pcurve")
            .geometry;
        for parameter in [0.25, 1.0, 2.5] {
            let original = uv(&pcurve, parameter);
            let reflected = uv(mirrored, -parameter);
            assert!((original.u - reflected.u).abs() < EPS_REVERSED_POINT);
            assert!((original.v - reflected.v).abs() < EPS_REVERSED_POINT);
        }
    }
}
