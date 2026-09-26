// SPDX-License-Identifier: Apache-2.0
//! Shared numerical operations for CATIA geometry.

use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::UnitVector3;

/// Normalize a finite, nonzero direction, retaining its representation.
///
/// Scale by the largest component before measuring length. The IR route
/// keeps the exact component-division order used by CATIA readers.
pub(crate) fn unit_vector<V>(value: V) -> Option<UnitVector3>
where
    V: Into<[f64; 3]>,
{
    UnitVector3::normalized_by_largest_component(Vector3::from(value.into()))
}

/// Euclidean distance without squaring large or tiny coordinate differences.
pub(crate) fn distance<P: Into<[f64; 3]>>(left: P, right: P) -> f64 {
    let left = left.into();
    let right = right.into();
    (left[0] - right[0])
        .hypot(left[1] - right[1])
        .hypot(left[2] - right[2])
}

#[cfg(test)]
mod tests {
    use super::{distance, unit_vector};
    use cadmpeg_ir::math::{Point3, Vector3};

    const EPS_UNIT_ROUNDING: f64 = 4.0 * f64::EPSILON;

    #[test]
    fn unit_vector_preserves_direction_across_finite_magnitudes() {
        for magnitude in [f64::from_bits(1), 1e-310, 1e-200, 1.0, 1e200, f64::MAX] {
            for (value, expected) in [
                (
                    [magnitude, -magnitude, 0.0],
                    [
                        std::f64::consts::FRAC_1_SQRT_2,
                        -std::f64::consts::FRAC_1_SQRT_2,
                        0.0,
                    ],
                ),
                (
                    [0.0, magnitude, -magnitude],
                    [
                        0.0,
                        std::f64::consts::FRAC_1_SQRT_2,
                        -std::f64::consts::FRAC_1_SQRT_2,
                    ],
                ),
                (
                    [-magnitude, 0.0, magnitude],
                    [
                        -std::f64::consts::FRAC_1_SQRT_2,
                        0.0,
                        std::f64::consts::FRAC_1_SQRT_2,
                    ],
                ),
                ([magnitude, 0.0, 0.0], [1.0, 0.0, 0.0]),
            ] {
                let result = unit_vector(value).expect("finite nonzero direction");
                for (actual, expected) in
                    <[f64; 3]>::from(*result.as_raw()).into_iter().zip(expected)
                {
                    assert!((actual - expected).abs() <= EPS_UNIT_ROUNDING);
                }
                assert!((result.as_raw().norm() - 1.0).abs() <= EPS_UNIT_ROUNDING);
                let vector_result =
                    unit_vector(Vector3::from(value)).expect("finite nonzero vector direction");
                assert_eq!(vector_result, result);
            }
        }
    }

    #[test]
    fn unit_vector_rejects_nonfinite_components_on_each_axis() {
        assert!(unit_vector([0.0; 3]).is_none());
        assert!(unit_vector(Vector3::new(0.0, 0.0, 0.0)).is_none());
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for axis in 0..3 {
                let mut value = [1.0; 3];
                value[axis] = invalid;
                assert!(unit_vector(value).is_none());
                assert!(unit_vector(Vector3::from(value)).is_none());
            }
        }
    }

    #[test]
    fn distance_preserves_representable_large_and_tiny_separations() {
        for magnitude in [f64::from_bits(1), 1e-310, 1e-200, 1.0, 1e200, f64::MAX] {
            for axis in 0..3 {
                let mut point = [0.0; 3];
                point[axis] = -magnitude;
                assert_eq!(distance(point, [0.0; 3]), magnitude);
                assert_eq!(
                    distance(Point3::from(point), Point3::new(0.0, 0.0, 0.0)),
                    magnitude
                );
            }
        }
        assert_eq!(distance([1.0, 2.0, 3.0], [4.0, 6.0, 3.0]), 5.0);
        assert_eq!(
            distance([f64::MAX, 0.0, 0.0], [-f64::MAX, 0.0, 0.0]),
            f64::INFINITY
        );
    }
}
