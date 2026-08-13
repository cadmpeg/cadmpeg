// SPDX-License-Identifier: Apache-2.0
use super::angular_basis;

#[test]
fn angular_basis_canonicalizes_a_full_sweep_with_decimal_roundoff() {
    let basis = angular_basis(0.0, std::f64::consts::TAU + std::f64::consts::TAU * 5.0e-13)
        .expect("a near-full finite sweep has an exact rational basis");

    assert_eq!(basis.controls.len(), 9);
    assert_eq!(basis.knots.last(), Some(&std::f64::consts::TAU));
}
