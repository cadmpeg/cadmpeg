// SPDX-License-Identifier: Apache-2.0
use super::{higher, orders, QuadraticPoles};
use super::super::finite_lanes;
use crate::features::FinitePoint3;
use crate::math::Point3;

#[test]
fn quadratic_orders_keep_tiny_coefficients_and_weight_derivative_cancellation() {
    use crate::geometry::nurbs::WeightedPole3;
    use crate::scalar::{FiniteReal, NonZeroReal};
    let s = 2.0_f64.powi(-600);
    let q = 2.0_f64.powi(300);
    assert_eq!(s * s, 0.0);
    for exponent in [-900, 0, 900] {
        for sign in [-1.0, 1.0] {
            let common = sign * 2.0_f64.powi(exponent);
            let poles = [(0.0, common), (0.0, common), (q, 2.0 * common)]
                .map(|(x, weight)| WeightedPole3 {
                    point: FinitePoint3::new(Point3::new(x, -0.0, 0.0)).unwrap(),
                    weight: NonZeroReal::new(weight).unwrap(),
                });
            let [base, first, second] = orders(QuadraticPoles::Spatial(&poles),
                FiniteReal::new(s).unwrap()).unwrap();
            // W=common*(1+s²), W'=2*common*s, W''=2*common.
            // The complete W rounds to common. Differencing rounded
            // Bernstein coefficients would instead erase or double W'.
            let weight = base.values[3].unwrap();
            assert_eq!(first.values[3].unwrap().quotient(weight).unwrap().get(), 2.0 * s);
            assert_eq!(second.values[3].unwrap().quotient(weight).unwrap().get(), 2.0);
            // C=2q*s²/(1+s²). Its value and first two derivatives
            // differ from these powers of two below binary64 resolution.
            let point = finite_lanes(base.project(base, &[]).unwrap()).unwrap();
            assert_eq!(point[0].get(), 2.0_f64.powi(-899));
            assert_eq!(point[1].get().to_bits(), (-0.0_f64).to_bits());
            let tangent = finite_lanes(first.project(base, &[(first, point)]).unwrap()).unwrap();
            assert_eq!(tangent[0].get(), 2.0_f64.powi(-298));
            let acceleration = finite_lanes(second.project(base,
                &[(second, point), (first, tangent), (first, tangent)]).unwrap()).unwrap();
            assert_eq!(acceleration[0].get(), 2.0_f64.powi(302));
        }
    }
}

#[test]
fn quadratic_third_uses_complete_source_constant_equality_and_nonzero_weight() {
    use crate::geometry::nurbs::WeightedPole3;
    use crate::math::sum::scaled_finite;
    use crate::scalar::{FiniteReal, NonZeroReal};
    let width = scaled_finite(f64::from_bits(1)).unwrap();
    let poles = [1.0, 1.0, 2.0].map(|weight| WeightedPole3 {
        point: FinitePoint3::new(Point3::new(f64::MAX, -0.0, f64::from_bits(1))).unwrap(),
        weight: NonZeroReal::new(weight).unwrap(),
    });
    assert_eq!(higher(QuadraticPoles::Spatial(&poles), FiniteReal::new(0.5).unwrap(), width, false, false).and_then(|value| value.third),
        Some([Ok(FiniteReal::ZERO); 3]));
    let poles = [1.0, -1.0, 1.0].map(|weight| WeightedPole3 {
        point: FinitePoint3::new(Point3::new(f64::MAX, -0.0, f64::from_bits(1))).unwrap(),
        weight: NonZeroReal::new(weight).unwrap(),
    });
    assert!(higher(QuadraticPoles::Spatial(&poles), FiniteReal::new(0.5).unwrap(), width, false, false).and_then(|value| value.third).is_none());
}

#[test]
fn planar_quadratic_higher_keeps_true_tiny_terms_and_complete_source_constants() {
    use crate::geometry::pcurve::WeightedPole2;
    use crate::math::{Point2, sum::scaled_finite};
    use crate::scalar::{FiniteReal, NonZeroReal};
    use crate::units::FinitePoint2;
    let s = 2.0_f64.powi(-600); let q = 2.0_f64.powi(300);
    let width = scaled_finite(1.0).unwrap();
    for exponent in [-900, 0, 900] {
        for sign in [-1.0, 1.0] {
            let common = sign * 2.0_f64.powi(exponent);
            let poles = [(0.0, common), (0.0, common), (q, 2.0 * common)].map(|(u, weight)| WeightedPole2 {
                point: FinitePoint2::new(Point2::new(u, -0.0)).unwrap(),
                weight: NonZeroReal::new(weight).unwrap(),
            });
            // C=2q*s²/(1+s²). Terms beyond these derivative controls lie
            // below binary64 resolution at the stated source parameter.
            for max_order in [3, 4, 5] {
                let value = higher(QuadraticPoles::Planar(&poles), FiniteReal::new(s).unwrap(), width,
                    max_order >= 4, max_order >= 5).unwrap();
                for (at, (lanes, expected)) in [value.third, value.fourth, value.fifth].into_iter()
                    .zip([-48.0 * 2.0_f64.powi(-300), -48.0 * q, 1440.0 * 2.0_f64.powi(-300)]).enumerate() {
                    if at + 3 > max_order { assert!(lanes.is_none()); }
                    else { assert_eq!(finite_lanes(lanes.unwrap()).unwrap().map(FiniteReal::get), [expected, 0.0, 0.0]); }
                }
            }
        }
    }
    let poles = [1.0, 1.0, 2.0].map(|weight| WeightedPole2 {
        point: FinitePoint2::new(Point2::new(f64::MAX, f64::from_bits(1))).unwrap(),
        weight: NonZeroReal::new(weight).unwrap(),
    });
    for parameter in [0.0, 0.5, 1.0] {
        let value = higher(QuadraticPoles::Planar(&poles), FiniteReal::new(parameter).unwrap(),
            scaled_finite(f64::from_bits(1)).unwrap(), true, true).unwrap();
        assert_eq!([value.third, value.fourth, value.fifth], [Some([Ok(FiniteReal::ZERO); 3]); 3]);
    }
    let poles = [1.0, -1.0, 1.0].map(|weight| WeightedPole2 {
        point: FinitePoint2::new(Point2::new(f64::MAX, -0.0)).unwrap(),
        weight: NonZeroReal::new(weight).unwrap(),
    });
    assert!(higher(QuadraticPoles::Planar(&poles), FiniteReal::new(0.5).unwrap(), width, true, true).is_none());
}
