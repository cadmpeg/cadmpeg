// SPDX-License-Identifier: Apache-2.0

const SMALL_MATRIX_ROTATION: f64 = 1.0e-8;
const EPS_MATRIX_RECONSTRUCTION: f64 = 1.0e-12;

#[test]
fn audit_regression_near_half_turn_retains_negative_axis() {
    let theta = std::f64::consts::PI - 5.0e-9;
    let (sine, cosine) = theta.sin_cos();
    let matrix = [
        [cosine, sine, 0., 0.],
        [-sine, cosine, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let rotation = super::matrix_axis_angle(&matrix).unwrap();
    assert!(rotation.direction.z < 0.0);
    assert!((rotation.angle.get() - theta).abs() <= 4.0 * f64::EPSILON);
}

#[test]
fn move_matrix_decomposes_to_translation_and_axis_angle() {
    let angle = std::f64::consts::PI / 3.0;
    let transform: [[f64; 4]; 4] = [
        [angle.cos(), 0.0, angle.sin(), -14.0],
        [0.0, 1.0, 0.0, 2.0],
        [-angle.sin(), 0.0, angle.cos(), 9.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rotation = super::matrix_axis_angle(&transform)
        .expect("nonidentity rotation");
    assert!((rotation.angle.get() - angle).abs() <= EPS_MATRIX_RECONSTRUCTION);
    assert!((rotation.direction.x - 0.0).abs() <= EPS_MATRIX_RECONSTRUCTION);
    assert!((rotation.direction.y - 1.0).abs() <= EPS_MATRIX_RECONSTRUCTION);
    assert!((rotation.direction.z - 0.0).abs() <= EPS_MATRIX_RECONSTRUCTION);
    assert_eq!(
        super::matrix_axis_angle(
            &crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
        ),
        None
    );
}

#[test]
fn numerical_audit_half_turn_recovers_axis_with_zero_x() {
    let transform = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let rotation = super::matrix_axis_angle(&transform).unwrap();
    assert_eq!(rotation.angle.get(), std::f64::consts::PI);
    let axis = [
        rotation.direction.x,
        rotation.direction.y,
        rotation.direction.z,
    ];
    for row in 0..3 {
        for column in 0..3 {
            let reconstructed =
                2.0 * axis[row] * axis[column] - (if row == column { 1.0 } else { 0.0 });
            assert!((reconstructed - transform[row][column]).abs() <= 8.0 * f64::EPSILON);
        }
    }
}

#[test]
fn numerical_seventh_matrix_angle_preserves_shallow_rotations() {
    for angle in [
        SMALL_MATRIX_ROTATION,
        -SMALL_MATRIX_ROTATION,
        std::f64::consts::PI,
    ] {
        let (sine, cosine) = angle.sin_cos();
        let matrix = [
            [cosine, -sine, 0.0, 0.0],
            [sine, cosine, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let rotation = super::matrix_axis_angle(&matrix).unwrap();
        assert!((rotation.angle.get() - angle.abs()).abs() <= 8.0 * f64::EPSILON * angle.abs());
        if angle.abs() < 1.0 {
            assert_eq!(rotation.direction.get().z.signum(), angle.signum());
        }
    }
}
