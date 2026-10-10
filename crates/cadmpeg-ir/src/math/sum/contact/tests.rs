// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::math::{Point2, Vector3};

const EPS_CONTACT_COMPOSITION: f64 = 1.0e-9;

fn impulse<const N: usize>(u: usize, v: usize, expected: f64) {
    // S=(1,-2,3)*u^u*v^v/(u!v!) at the origin.
    // a^(1..5)=(2,-3,5,-7,11), b^(1..5)=(-1,4,-6,8,-10).
    let mut partials = [[FiniteVector3::ZERO; 6]; 5];
    partials[u + v - 1][v] = FiniteVector3::new(Vector3::new(1.0, -2.0, 3.0)).unwrap();
    let uv = [[2.0, -1.0], [-3.0, 4.0], [5.0, -6.0], [-7.0, 8.0], [11.0, -10.0]]
        .map(|row| row.map(|value| FiniteReal::new(value).unwrap()));
    let actual = contact_derivative::<N>(partials, uv).unwrap();
    for (actual, expected) in actual.into_iter().zip([expected, -2.0 * expected, 3.0 * expected]) {
        assert!((actual.unwrap().get() - expected).abs() <= EPS_CONTACT_COMPOSITION);
    }
}

#[test]
fn contact_chain_orders_match_all_independent_exact_taylor_impulses() {
    // Coefficients come from independent polynomial products, not the
    // coloured-partition implementation or an observed evaluation result.
    impulse::<2>(0, 1, 4.0);
    impulse::<2>(0, 2, 1.0);
    impulse::<2>(1, 0, -3.0);
    impulse::<2>(1, 1, -4.0);
    impulse::<2>(2, 0, 4.0);
    impulse::<3>(0, 1, -6.0);
    impulse::<3>(0, 2, -12.0);
    impulse::<3>(0, 3, -1.0);
    impulse::<3>(1, 0, 5.0);
    impulse::<3>(1, 1, 33.0);
    impulse::<3>(1, 2, 6.0);
    impulse::<3>(2, 0, -18.0);
    impulse::<3>(2, 1, -12.0);
    impulse::<3>(3, 0, 8.0);
    impulse::<4>(0, 1, 8.0);
    impulse::<4>(0, 2, 72.0);
    impulse::<4>(0, 3, 24.0);
    impulse::<4>(0, 4, 1.0);
    impulse::<4>(1, 0, -7.0);
    impulse::<4>(1, 1, -140.0);
    impulse::<4>(1, 2, -114.0);
    impulse::<4>(1, 3, -8.0);
    impulse::<4>(2, 0, 67.0);
    impulse::<4>(2, 1, 168.0);
    impulse::<4>(2, 2, 24.0);
    impulse::<4>(3, 0, -72.0);
    impulse::<4>(3, 1, -32.0);
    impulse::<4>(4, 0, 16.0);
    impulse::<5>(0, 1, -10.0);
    impulse::<5>(0, 2, -280.0);
    impulse::<5>(0, 3, -300.0);
    impulse::<5>(0, 4, -40.0);
    impulse::<5>(0, 5, -1.0);
    impulse::<5>(1, 0, 11.0);
    impulse::<5>(1, 1, 495.0);
    impulse::<5>(1, 2, 1130.0);
    impulse::<5>(1, 3, 270.0);
    impulse::<5>(1, 4, 10.0);
    impulse::<5>(2, 0, -220.0);
    impulse::<5>(2, 1, -1295.0);
    impulse::<5>(2, 2, -660.0);
    impulse::<5>(2, 3, -40.0);
    impulse::<5>(3, 0, 470.0);
    impulse::<5>(3, 1, 680.0);
    impulse::<5>(3, 2, 80.0);
    impulse::<5>(4, 0, -240.0);
    impulse::<5>(4, 1, -80.0);
    impulse::<5>(5, 0, 32.0);
}

#[test]
fn directional_fifth_cancels_true_extreme_products_before_range_projection() {
    // S=(a-b)^5 has order5 partials120*(-1)^v. The actual line a=b
    // gives zero, although each separate nonzero product is out of range.
    let partials = std::array::from_fn(|v| FiniteVector3::new(Vector3::new(
        if v % 2 == 0 { 120.0 } else { -120.0 },
        if v % 2 == 0 { -60.0 } else { 60.0 }, 0.0,
    )).unwrap());
    let direction = FinitePoint2::new(Point2::new(2.0_f64.powi(500), 2.0_f64.powi(500))).unwrap();
    assert_eq!(directional_derivative::<5>(partials, direction), Some([Ok(FiniteReal::ZERO); 3]));
}

#[test]
fn contact_directional_projection_keeps_subnormal_sign_and_real_overflow() {
    let direction = FinitePoint2::new(Point2::new(0.5, 0.0)).unwrap();
    for sign in [-1.0, 1.0] {
        let mut partials = [FiniteVector3::ZERO; 6];
        partials[0] = FiniteVector3::new(Vector3::new(sign * 32.0 * f64::from_bits(1), 0.0, 0.0)).unwrap();
        assert_eq!(directional_derivative::<5>(partials, direction).unwrap()[0].unwrap().get(), sign * f64::from_bits(1));
        partials[0] = FiniteVector3::new(Vector3::new(sign * f64::MAX, 0.0, 0.0)).unwrap();
        let direction = FinitePoint2::new(Point2::new(2.0, 0.0)).unwrap();
        let actual = directional_derivative::<5>(partials, direction).unwrap();
        assert_eq!(actual[0], Err(sign * f64::INFINITY));
        assert_eq!([actual[1], actual[2]], [Ok(FiniteReal::ZERO); 2]);
    }
}
