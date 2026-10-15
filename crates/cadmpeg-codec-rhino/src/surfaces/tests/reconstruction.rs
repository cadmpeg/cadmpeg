// SPDX-License-Identifier: MIT OR Apache-2.0
use crate::surfaces::reconstruct_knots;

const STORED: [f64; 7] = [0., 1., 2., 3., 4., 5., 6.];

#[test]
fn raw_knot_reconstruction_extends_both_ends() {
    assert_eq!(
        reconstruct_knots(&STORED, 3, 6).expect("reconstructed knots"),
        [-1., 0., 1., 2., 3., 4., 5., 6., 7.]
    );
}

#[test]
fn periodic_knot_scan_rejects_short_and_unequal_intervals() {
    assert!(!crate::surfaces::periodic_knots(&STORED, 2, 6));
    assert!(!crate::surfaces::periodic_knots(
        &[0., 0., 0., 1., 2., 3., 3.],
        3,
        6
    ));
}
