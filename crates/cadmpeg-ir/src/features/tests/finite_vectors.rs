// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::math::Vector3;

#[test]
fn a_transform_hands_out_its_columns_and_translation_as_finite_vectors() {
    use crate::transform::Transform;

    let transform = Transform::affine([
        [1.0, 2.0, 3.0, -f64::MAX],
        [4.0, 5.0, 6.0, 0.5],
        [7.0, 8.0, 9.0, f64::MAX],
    ])
    .unwrap();
    assert_eq!(
        transform.translation().get(),
        Vector3::new(-f64::MAX, 0.5, f64::MAX)
    );
    assert_eq!(
        transform
            .linear_columns()
            .map(crate::features::FiniteVector3::get),
        [
            Vector3::new(1.0, 4.0, 7.0),
            Vector3::new(2.0, 5.0, 8.0),
            Vector3::new(3.0, 6.0, 9.0),
        ]
    );
}

#[test]
fn unit_directions_cross_into_a_finite_vector_and_finite_vectors_normalize() {
    use crate::units::UnitVector3;

    let x = UnitVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap();
    let y = UnitVector3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
    assert_eq!(x.finite_cross(y).get(), Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(x.finite_cross(x).unit_nonzero(), None);
    let far = crate::features::FiniteVector3::new(Vector3::new(f64::MAX, f64::MAX, 0.0)).unwrap();
    let unit = far.unit_nonzero().unwrap();
    assert!((unit.x - std::f64::consts::FRAC_1_SQRT_2).abs() <= 2.0 * f64::EPSILON);
    assert_eq!(unit.x, unit.y);
}
