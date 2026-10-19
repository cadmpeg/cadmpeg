// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::{KnotVector, NurbsError};
use crate::geometry::pcurve::{
    PcurveNurbs, PcurveNurbsPoles, PolarNurbsPole, PolarPcurveNurbs, WeightedPole2,
};
use crate::math::Point2;
use crate::scalar::NonZeroReal;
use crate::units::FinitePoint2;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn knots() -> Vec<f64> {
    vec![0.0, 0.0, 1.0, 1.0]
}
fn points() -> Vec<Point2> {
    vec![Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)]
}
fn polar_poles() -> Vec<PolarNurbsPole> {
    points()
        .into_iter()
        .enumerate()
        .map(|(index, radial)| PolarNurbsPole {
            radial,
            axial: if index == 0 { 5.0 } else { 6.0 },
        })
        .collect()
}

#[test]
fn pcurve_construction_moves_admitted_lanes_and_admits_finite_knot_conversion() {
    let knots = KnotVector::new(knots()).expect("knots");
    let knot_address = knots.as_ptr();
    let points: Vec<_> = points()
        .into_iter()
        .map(|point| WeightedPole2 {
            point: FinitePoint2::new(point).expect("finite"),
            weight: NonZeroReal::ONE,
        })
        .collect();
    let pole_address = points.as_ptr();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let built =
        PcurveNurbs::new(1, knots, PcurveNurbsPoles::Rational { points }, false).expect("valid");
    let PcurveNurbsPoles::Rational { points } = built.pole_rows() else {
        panic!("rational");
    };
    assert_eq!(points.as_ptr(), pole_address);
    assert_eq!(built.knots().as_ptr(), knot_address);
    ctx.finish_session().expect("no duplicate admission");
}

#[test]
fn pcurve_construction_checks_weight_lanes_before_degree_and_knots() {
    assert_eq!(
        PcurveNurbs::from_lanes(0, Vec::<f64>::new(), points(), Some(vec![0.0]), false),
        Err(NurbsError::WeightLaneLength {
            field: "pcurve poles".into(),
            poles: 2,
            weights: 1
        })
    );
    assert_eq!(
        PolarPcurveNurbs::from_lanes(
            0,
            Vec::<f64>::new(),
            polar_poles(),
            Some(vec![1.0, 0.0]),
            false,
        ),
        Err(NurbsError::UnusableWeight {
            field: "polar poles".into(),
            index: 1,
            weight: 0.0
        })
    );
}
