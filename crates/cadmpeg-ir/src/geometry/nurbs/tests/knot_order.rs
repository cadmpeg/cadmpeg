// SPDX-License-Identifier: Apache-2.0
use crate::geometry::nurbs::{knots_nondecreasing, knots_strictly_increasing};

#[test]
fn knot_order_predicates_test_adjacent_pairs() {
    for (knots, ordered) in [
        (vec![], true),
        (vec![f64::NAN], true),
        (vec![1.0, 0.0, 2.0, 3.0], false),
        (vec![0.0, f64::NAN, 2.0, 3.0], false),
        (vec![0.0, 0.0, 1.0, 1.0], true),
    ] {
        assert_eq!(knots_nondecreasing(&knots), ordered);
    }
    for (knots, ordered) in [
        (vec![], true),
        (vec![f64::NAN], true),
        (vec![0.0, 0.0, 1.0, 2.0], false),
        (vec![1.0, 0.0, 1.0, 2.0], false),
        (vec![0.0, f64::NAN, 1.0, 2.0], false),
        (vec![0.0, 1.0, 2.0, 3.0], true),
    ] {
        assert_eq!(knots_strictly_increasing(&knots), ordered);
    }
}
