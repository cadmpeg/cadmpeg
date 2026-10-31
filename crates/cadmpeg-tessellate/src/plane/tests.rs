// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::FRAC_PI_2;

use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::math::Vector3;

use super::{angle, contains, signed_area};

/// Upper bound on the angle between `(1, 0, 0)` and `(1, EPSILON, 0)`.
const NEARLY_PARALLEL_ANGLE: f64 = 1e-6;

#[test]
fn a_right_angle_is_a_quarter_turn() {
    assert!(floats_agree(
        angle(Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        FRAC_PI_2
    ));
}

#[test]
fn nearly_parallel_vectors_have_a_finite_angle() {
    let value = angle(
        Vector3::new(1.0, 0.0, 0.0),
        Vector3::new(1.0, f64::EPSILON, 0.0),
    );
    assert!(value.is_finite());
    assert!(value >= 0.0);
    assert!(value < NEARLY_PARALLEL_ANGLE);
}

#[test]
fn a_counterclockwise_square_has_positive_area() {
    let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
    assert_eq!(signed_area(&square), 8.0);
    let reversed: Vec<_> = square.iter().rev().copied().collect();
    assert_eq!(signed_area(&reversed), -8.0);
}

#[test]
fn containment_follows_the_even_odd_rule() {
    let square = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
    assert!(contains(&square, [1.0, 1.0]));
    assert!(!contains(&square, [3.0, 1.0]));
    assert!(!contains(&square, [1.0, -0.5]));
}
