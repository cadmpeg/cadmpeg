// SPDX-License-Identifier: Apache-2.0

use super::{contains, signed_area};

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
