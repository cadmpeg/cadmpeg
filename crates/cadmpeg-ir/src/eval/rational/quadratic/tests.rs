// SPDX-License-Identifier: Apache-2.0
use super::{higher, orders};
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
            let [base, first, second] = orders(&poles,
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
    assert_eq!(higher(&poles, FiniteReal::new(0.5).unwrap(), width, false, false).and_then(|value| value.third),
        Some([Ok(FiniteReal::ZERO); 3]));
    let poles = [1.0, -1.0, 1.0].map(|weight| WeightedPole3 {
        point: FinitePoint3::new(Point3::new(f64::MAX, -0.0, f64::from_bits(1))).unwrap(),
        weight: NonZeroReal::new(weight).unwrap(),
    });
    assert!(higher(&poles, FiniteReal::new(0.5).unwrap(), width, false, false).and_then(|value| value.third).is_none());
}
