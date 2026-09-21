// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn numerical_0922_trim_intersection_keeps_disjoint_and_secant_cases() {
    for y in [0.0, 0.002] {
        assert_eq!(
            trim_line_circle_intersection([-10000.0, y], [10000.0, y], [0.0, 0.0], 0.001),
            None
        );
    }
    assert_eq!(
        trim_line_circle_intersection([-10000.0, 0.001], [10000.0, 0.001], [0.0, 0.0], 0.001),
        Some([0.0, 0.001])
    );
}
