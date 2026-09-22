// SPDX-License-Identifier: Apache-2.0

use super::super::*;
#[test]
fn numerical_audit_polygon_incidence_ignores_length_scale() {
    for scale in [1., 1e-5] {
        assert!(segments_intersect(
            [[-scale, 0.], [scale, 0.]],
            [[0., -scale], [0., scale]]
        ));
        assert!(polygon_strictly_contains(
            &[[-scale, 0.], [0., -scale], [scale, 0.], [0., scale]],
            [0., 0.]
        ));
        assert!(!polygon_strictly_contains(
            &[[-scale, 0.], [0., -scale], [scale, 0.], [0., scale]],
            [scale, 0.]
        ));
    }
}
#[test]
fn numerical_audit_polygon_admission_ignores_translation() {
    for offset in [0., 1e4] {
        let p = [[0., 0.], [0.001, 0.], [0.001, 0.001], [0., 0.001]]
            .map(|p| [p[0] + offset, p[1] + offset]);
        assert!(valid_parameter_polygon(&p));
    }
    assert!(!valid_parameter_polygon(&[[0., 0.], [1., 0.], [2., 0.]]));
}
