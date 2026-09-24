// SPDX-License-Identifier: Apache-2.0
use crate::features::{FinitePoint3, FiniteVector3};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::{Transform, Transform2, TransformError};

const EPS_INVERSE_CHECK: f64 = 1.0e-12;
const EPS_NORMAL_DIRECTION: f64 = 1.0e-15;

#[test]
fn transform2_preserves_finite_cancellation() {
    let point_map = Transform2::affine([[2.0, 0.0, -1.0e308], [0.0, 1.0, 0.0]]).unwrap();
    assert_eq!(
        point_map.apply_point(Point2::new(1.0e308, 0.0)),
        Point2::new(1.0e308, 0.0)
    );
    let vector_map = Transform2::affine([[2.0, -2.0, 0.0], [0.0, 1.0, 0.0]]).unwrap();
    assert_eq!(
        vector_map.apply_vector(Point2::new(1.0e308, 1.0e308)),
        Point2::new(0.0, 1.0e308)
    );
}

#[test]
fn large_well_conditioned_matrix_has_a_representable_inverse() {
    let scale = 1.0e308;
    let transform = Transform::affine([
        [scale, scale, 0.0, 0.0],
        [scale, -scale, 0.0, 0.0],
        [0.0, 0.0, scale, 0.0],
    ])
    .unwrap();
    let inverse = transform.try_inverse_affine().expect("finite inverse");
    let rows = inverse.affine_rows();
    for (actual, expected) in [
        (rows[0][0], 0.5),
        (rows[0][1], 0.5),
        (rows[1][0], 0.5),
        (rows[1][1], -0.5),
        (rows[2][2], 1.0),
    ] {
        assert!((actual * scale - expected).abs() <= 4.0 * f64::EPSILON);
    }
}

#[test]
fn normal_does_not_need_unused_inverse_entries_to_be_representable() {
    let transform = Transform::affine([
        [1.0e-310, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        transform.try_inverse_affine(),
        Err(TransformError::NonFinite)
    );
    assert_eq!(
        transform
            .apply_normal(Vector3::new(0.0, 1.0, 0.0))
            .map(Vector3::from),
        Some(Vector3::new(0.0, 1.0, 0.0))
    );
}

#[test]
fn finite_inverses_survive_extreme_scales_and_axis_permutations() {
    for exponent in [-800, -600, 0, 600, 800] {
        let scale = 2.0_f64.powi(exponent);
        for scales in [[scale; 3], [scale, 1.0 / scale, -2.0]] {
            for permutation in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                let mut source = [[0.0; 4]; 3];
                let mut expected = [[0.0; 4]; 3];
                for row in 0..3 {
                    source[row][permutation[row]] = scales[row];
                    source[row][3] = scales[row] * [2.0, -3.0, 4.0][row];
                    expected[permutation[row]][row] = 1.0 / scales[row];
                    expected[permutation[row]][3] = -[2.0, -3.0, 4.0][row];
                }
                let transform = Transform::affine(source).unwrap();
                let inverse = transform.try_inverse_affine().unwrap();
                assert_eq!(inverse.affine_rows(), expected);
                assert_eq!(inverse.compose(transform), Ok(Transform::identity()));
                assert_eq!(transform.compose(inverse), Ok(Transform::identity()));
            }
        }
    }
    let dense = Transform::affine([
        [1.0, 2.0, 3.0, 0.0],
        [0.0, 1.0, 4.0, 0.0],
        [5.0, 6.0, 0.0, 0.0],
    ])
    .unwrap();
    let expected = [
        [-24.0, 18.0, 5.0, 0.0],
        [20.0, -15.0, -4.0, 0.0],
        [-5.0, 4.0, 1.0, 0.0],
    ];
    for (actual, expected) in dense
        .try_inverse_affine()
        .unwrap()
        .affine_rows()
        .iter()
        .flatten()
        .zip(expected.iter().flatten())
    {
        assert!((actual - expected).abs() <= EPS_INVERSE_CHECK);
    }
}

#[test]
fn normal_direction_ignores_translation_and_avoids_squared_length_overflow() {
    let translation_overflow = Transform::affine([
        [0.5, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        translation_overflow.try_inverse_affine(),
        Err(TransformError::NonFinite)
    );
    assert_eq!(
        translation_overflow
            .apply_normal(Vector3::new(0.0, 1.0, 0.0))
            .map(Vector3::from),
        Some(Vector3::new(0.0, 1.0, 0.0))
    );
    for scale in [f64::from_bits(1), f64::MIN_POSITIVE, 1.0, f64::MAX] {
        assert_eq!(
            Transform::identity()
                .apply_normal(Vector3::new(scale, 0.0, 0.0))
                .map(Vector3::from),
            Some(Vector3::new(1.0, 0.0, 0.0))
        );
        let actual = Transform::identity()
            .apply_normal(Vector3::new(scale, scale, 0.0))
            .map(Vector3::from)
            .unwrap();
        let component = 1.0 / 2.0_f64.sqrt();
        assert_eq!(actual, Vector3::new(component, component, 0.0));
    }
    for exponent in [-800, -600, 600, 800] {
        let scale = 2.0_f64.powi(exponent);
        let transform = Transform::affine([
            [scale, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, scale, 0.0],
        ])
        .unwrap();
        assert_eq!(
            transform
                .apply_normal(Vector3::new(0.0, 1.0, 0.0))
                .map(Vector3::from),
            Some(Vector3::new(0.0, 1.0, 0.0))
        );
    }
    for normal in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::NAN, 1.0, 0.0),
        Vector3::new(1.0, f64::INFINITY, 0.0),
    ] {
        assert!(Transform::identity()
            .apply_normal(normal)
            .map(Vector3::from)
            .is_none());
    }
    let singular = Transform::affine([[0.0; 4]; 3]).unwrap();
    assert_eq!(singular.try_inverse_affine(), Err(TransformError::Singular));
    let inverse_overflow = Transform::affine([
        [f64::from_bits(1), 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        inverse_overflow.try_inverse_affine(),
        Err(TransformError::NonFinite)
    );
}

#[test]
fn normal_transform_preserves_product_range_and_cancellation() {
    let overflow = Transform::affine([
        [0.5, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        overflow
            .apply_normal(Vector3::new(f64::MAX, 0.0, 0.0))
            .map(Vector3::from),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );

    let underflow = Transform::affine([
        [2.0_f64.powi(800), 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        underflow
            .apply_normal(Vector3::new(2.0_f64.powi(-800), 0.0, 0.0))
            .map(Vector3::from),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );

    let anisotropic = Transform::affine([
        [2.0_f64.powi(800), 0.0, 0.0, 0.0],
        [0.0, 2.0_f64.powi(-800), 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        anisotropic
            .apply_normal(Vector3::new(0.0, 1.0, 0.0))
            .map(Vector3::from),
        Some(Vector3::new(0.0, 1.0, 0.0))
    );
    let diagonal_component = 1.0 / 2.0_f64.sqrt();
    assert_eq!(
        anisotropic
            .apply_normal(Vector3::new(2.0_f64.powi(800), 2.0_f64.powi(-800), 0.0,))
            .map(Vector3::from),
        Some(Vector3::new(diagonal_component, diagonal_component, 0.0))
    );

    let cancellation = Transform::affine([
        [1.0e100, 0.0, 0.0, 0.0],
        [-1.0e300, 1.0e100, 0.0, 0.0],
        [1.0e300, 0.0, 1.0e100, 0.0],
    ])
    .unwrap();
    let component = 1.0 / 3.0_f64.sqrt();
    let transformed = cancellation
        .apply_normal(Vector3::new(1.0, 1.0, 1.0))
        .map(Vector3::from)
        .expect("finite normal direction");
    for value in [transformed.x, transformed.y, transformed.z] {
        assert!((value - component).abs() <= EPS_NORMAL_DIRECTION);
    }
}

#[test]
fn affine_point_keeps_a_subnormal_tie_breaking_tail() {
    let transform = Transform::affine([
        [2.0_f64.powi(-537), 2.0_f64.powi(-564), 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let result = transform
        .apply_point(Point3::new(2.0_f64.powi(-538), 2.0_f64.powi(-564), 0.0))
        .unwrap();
    assert_eq!(result.x.to_bits(), 1);
}

#[test]
fn normal_transform_preserves_nonzero_subnormal_products() {
    let transform = Transform::affine([
        [2.0_f64.powi(1000), 0.0, 0.0, 0.0],
        [0.0, 2.0_f64.powi(1000), 0.0, 0.0],
        [0.0, 0.0, 2.0_f64.powi(1000), 0.0],
    ])
    .expect("affine transform");
    let normal = Vector3::new(3.0 * 2.0_f64.powi(-75), 5.0 * 2.0_f64.powi(-75), 0.0);
    let expected = 34.0_f64.sqrt();
    let actual = transform
        .apply_normal(normal)
        .map(Vector3::from)
        .expect("finite transformed normal");
    assert!((actual.x - 3.0 / expected).abs() < EPS_NORMAL_DIRECTION);
    assert!((actual.y - 5.0 / expected).abs() < EPS_NORMAL_DIRECTION);
    assert_eq!(actual.z, 0.0);
}

#[test]
fn numerical_audit_affine_cancellation_keeps_representable_results() {
    use crate::math::Point3;
    let matrix = Transform::affine([
        [1.0, 1.0, -1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let point = Point3::new(f64::MAX, f64::MAX, f64::MAX);
    assert_eq!(
        matrix.apply_point(point).map(FinitePoint3::get),
        Some(point)
    );
    assert_eq!(
        matrix
            .apply_vector(Vector3::from(<[f64; 3]>::from(point)))
            .map(FiniteVector3::get),
        Some(Vector3::new(f64::MAX, f64::MAX, f64::MAX))
    );
    let translation = Transform::affine([
        [1.0, 0.0, 0.0, f64::MAX],
        [0.0, 1.0, 0.0, f64::MAX],
        [0.0, 0.0, 1.0, f64::MAX],
    ])
    .unwrap();
    assert_eq!(
        matrix
            .compose(translation)
            .unwrap()
            .apply_point(Point3::new(0.0, 0.0, 0.0))
            .map(FinitePoint3::get),
        Some(point)
    );
    let source = Transform::affine([
        [1.0, -1.0, 1.0, f64::MAX],
        [0.0, 1.0, 0.0, f64::MAX],
        [0.0, 0.0, 1.0, f64::MAX],
    ])
    .unwrap();
    let inverse = source.try_inverse_affine().unwrap();
    assert_eq!(inverse.affine_rows().map(|row| row[3]), [-f64::MAX; 3]);
}
