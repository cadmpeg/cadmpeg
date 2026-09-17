// SPDX-License-Identifier: Apache-2.0
use crate::math::Vector3;
use crate::transform::{
    scaled_finite, ExactSignedSum, Transform, TransformError, MAX_SCALED_EXPONENT,
    MIN_SCALED_EXPONENT, MIN_SIGNIFICAND_EXPONENT,
};

const EPS_INVERSE_CHECK: f64 = 1.0e-12;
const EPS_NORMAL_DIRECTION: f64 = 1.0e-15;

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
        translation_overflow.apply_normal(Vector3::new(0.0, 1.0, 0.0)),
        Some(Vector3::new(0.0, 1.0, 0.0))
    );
    for scale in [f64::from_bits(1), f64::MIN_POSITIVE, 1.0, f64::MAX] {
        assert_eq!(
            Transform::identity().apply_normal(Vector3::new(scale, 0.0, 0.0)),
            Some(Vector3::new(1.0, 0.0, 0.0))
        );
        let actual = Transform::identity()
            .apply_normal(Vector3::new(scale, scale, 0.0))
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
            transform.apply_normal(Vector3::new(0.0, 1.0, 0.0)),
            Some(Vector3::new(0.0, 1.0, 0.0))
        );
    }
    for normal in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::NAN, 1.0, 0.0),
        Vector3::new(1.0, f64::INFINITY, 0.0),
    ] {
        assert!(Transform::identity().apply_normal(normal).is_none());
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
        overflow.apply_normal(Vector3::new(f64::MAX, 0.0, 0.0)),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );

    let underflow = Transform::affine([
        [2.0_f64.powi(800), 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        underflow.apply_normal(Vector3::new(2.0_f64.powi(-800), 0.0, 0.0)),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );

    let anisotropic = Transform::affine([
        [2.0_f64.powi(800), 0.0, 0.0, 0.0],
        [0.0, 2.0_f64.powi(-800), 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    assert_eq!(
        anisotropic.apply_normal(Vector3::new(0.0, 1.0, 0.0)),
        Some(Vector3::new(0.0, 1.0, 0.0))
    );
    let diagonal_component = 1.0 / 2.0_f64.sqrt();
    assert_eq!(
        anisotropic.apply_normal(Vector3::new(2.0_f64.powi(800), 2.0_f64.powi(-800), 0.0,)),
        Some(Vector3::new(diagonal_component, diagonal_component, 0.0))
    );

    let cancellation = Transform::affine([
        [1.0e100, 0.0, 0.0, 0.0],
        [-1.0e300, 1.0e100, 0.0, 0.0],
        [1.0e300, 0.0, 1.0e100, 0.0],
    ])
    .unwrap();
    let component = 1.0 / 3.0_f64.sqrt();
    assert_eq!(
        cancellation.apply_normal(Vector3::new(1.0, 1.0, 1.0)),
        Some(Vector3::new(component, component, component))
    );
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
        .expect("finite transformed normal");
    assert!((actual.x - 3.0 / expected).abs() < EPS_NORMAL_DIRECTION);
    assert!((actual.y - 5.0 / expected).abs() < EPS_NORMAL_DIRECTION);
    assert_eq!(actual.z, 0.0);
}

#[test]
fn every_exponent_the_scaled_value_constructors_produce_stays_inside_the_stated_range() {
    // The two ends `scaled_finite` reaches: the smallest positive subnormal and
    // the largest finite magnitude.
    let smallest_subnormal = f64::from_bits(1);
    let low = scaled_finite(smallest_subnormal).expect("finite value");
    let high = scaled_finite(f64::MAX).expect("finite value");
    assert_eq!(low.exponent.0, MIN_SIGNIFICAND_EXPONENT + 1);
    assert_eq!(high.exponent.0, 1024);

    // The two ends `ExactSignedSum::finish` reaches: three products of the
    // largest finite magnitude, and one product of two smallest subnormals.
    let mut largest = ExactSignedSum::default();
    for _ in 0..3 {
        largest.add_product(f64::MAX, f64::MAX);
    }
    let largest = largest.finish().expect("nonzero sum");
    let mut smallest = ExactSignedSum::default();
    smallest.add_product(smallest_subnormal, smallest_subnormal);
    let smallest = smallest.finish().expect("nonzero sum");

    let range = MIN_SCALED_EXPONENT..=MAX_SCALED_EXPONENT;
    for value in [low, high, largest, smallest] {
        assert!(
            range.contains(&value.exponent.0),
            "exponent {} left {MIN_SCALED_EXPONENT}..={MAX_SCALED_EXPONENT}",
            value.exponent.0
        );
    }

    // The difference of any two of them is a plain subtraction inside the
    // span the range states, at both signs.
    let span = MAX_SCALED_EXPONENT - MIN_SCALED_EXPONENT;
    for left in [low, high, largest, smallest] {
        for right in [low, high, largest, smallest] {
            let difference = left.exponent.difference(right.exponent);
            assert!(
                (-span..=span).contains(&difference),
                "difference {difference} left -{span}..={span}"
            );
        }
    }

    // `apply_normal` scales every value by the maximum exponent of the set, so
    // the difference the subtraction states is never positive and the power of
    // two it feeds `powi` is in `(0.0, 1.0]`.
    let scale_exponent = [low, high, largest, smallest]
        .iter()
        .map(|value| value.exponent)
        .max()
        .expect("nonempty set");
    for value in [low, high, largest, smallest] {
        assert!(value.exponent.difference(scale_exponent) <= 0);
        let scaled = value.scaled_by(scale_exponent);
        assert!(scaled.is_finite(), "scaled value {scaled} is not finite");
    }
}
