// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::transform::Transform;

use super::quaternion;

/// Agreement of a quaternion component with its closed form.
const COMPONENT_TOLERANCE: f64 = 1e-12;

#[test]
fn a_quarter_turn_about_x_is_its_half_angle_quaternion() {
    let rotation = Transform::affine([
        [1.0, 0.0, 0.0, 5.0],
        [0.0, 0.0, -1.0, 6.0],
        [0.0, 1.0, 0.0, 7.0],
    ])
    .unwrap();
    let half = std::f64::consts::FRAC_PI_4;
    let expected = [half.cos(), half.sin(), 0.0, 0.0];
    for (found, expected) in quaternion(&rotation).iter().zip(expected) {
        assert!(
            (found - expected).abs() <= COMPONENT_TOLERANCE,
            "{found} != {expected}"
        );
    }
}

#[test]
fn a_half_turn_keeps_a_unit_quaternion() {
    let rotation = Transform::affine([
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let [w, x, y, z] = quaternion(&rotation);
    assert!(w.abs() <= COMPONENT_TOLERANCE);
    assert!(x.abs() <= COMPONENT_TOLERANCE && y.abs() <= COMPONENT_TOLERANCE);
    assert!((z.abs() - 1.0).abs() <= COMPONENT_TOLERANCE);
}
