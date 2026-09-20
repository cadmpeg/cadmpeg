// SPDX-License-Identifier: Apache-2.0
use crate::math::Point3;
use crate::transform::Transform;

#[test]
fn numerical_audit_inverse_point_uses_scale_safe_affine_inverse() {
    let scale = 1.0e110;
    let matrix = Transform::affine([
        [scale, 0.0, 0.0, 0.0],
        [0.0, scale, 0.0, 0.0],
        [0.0, 0.0, scale, 0.0],
    ])
    .unwrap();
    let (point, tolerance) =
        super::super::inverse_affine_point(matrix, Point3::new(scale, scale, scale)).unwrap();
    assert_eq!(point, Point3::new(1.0, 1.0, 1.0));
    assert!((tolerance / (3.0_f64.sqrt() / scale) - 1.0).abs() <= 8.0 * f64::EPSILON);
}
