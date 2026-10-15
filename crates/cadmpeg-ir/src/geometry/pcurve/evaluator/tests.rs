// SPDX-License-Identifier: Apache-2.0

use super::PcurveEvaluatorLanes;
use crate::geometry::pcurve::{PcurveNurbsPoles, WeightedPole2};
use crate::math::Point2;
use crate::scalar::NonZeroReal;
use crate::units::FinitePoint2;

fn poles(rational: bool) -> PcurveNurbsPoles<FinitePoint2> {
    let points = [Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)]
        .map(|point| FinitePoint2::new(point).unwrap());
    if rational {
        PcurveNurbsPoles::Rational {
            points: points
                .into_iter()
                .zip([1.0, 2.0])
                .map(|(point, weight)| WeightedPole2 {
                    point,
                    weight: NonZeroReal::new(weight).unwrap(),
                })
                .collect(),
        }
    } else {
        PcurveNurbsPoles::Polynomial {
            points: points.to_vec(),
        }
    }
}

#[test]
fn evaluator_lanes_copy_both_lanes() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for rational in [false, true] {
        let source = poles(rational);
        let lanes = PcurveEvaluatorLanes::new(&source);
        assert_eq!(
            lanes.points(),
            [Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)]
        );
        assert_eq!(lanes.weights(), rational.then_some([1.0, 2.0].as_slice()));
    }
    ctx.finish_session().unwrap();
}

#[test]
fn evaluator_lanes_keep_empty_weight_presence() {
    let ctx = cadmpeg_test_support::service_decode_context();
    for rational in [false, true] {
        let source = if rational {
            PcurveNurbsPoles::Rational { points: Vec::new() }
        } else {
            PcurveNurbsPoles::Polynomial { points: Vec::new() }
        };
        let lanes = PcurveEvaluatorLanes::new(&source);
        assert!(lanes.points().is_empty());
        assert_eq!(lanes.weights(), rational.then_some([].as_slice()));
    }
    ctx.finish_session().unwrap();
}
