// SPDX-License-Identifier: Apache-2.0
//! Shared numerical operations for CATIA geometry.

/// Normalize a finite, nonzero direction, retaining its representation.
///
/// Scale by the largest component before measuring length. This avoids
/// overflow and subnormal rounding of the length, including when the true
/// length exceeds `f64::MAX`. Component division avoids reciprocal overflow.
pub(crate) fn unit_vector<V>(value: V) -> Option<V>
where
    V: Into<[f64; 3]> + From<[f64; 3]>,
{
    let components = value.into();
    if !components.iter().all(|component| component.is_finite()) {
        return None;
    }
    let scale = components[0]
        .abs()
        .max(components[1].abs())
        .max(components[2].abs());
    if scale == 0.0 {
        return None;
    }
    let scaled = components.map(|component| component / scale);
    let length = scaled[0].hypot(scaled[1]).hypot(scaled[2]);
    Some(V::from(scaled.map(|component| component / length)))
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
    use cadmpeg_ir::math::Point3;

    const EPS_UNIT_ROUNDING: f64 = 4.0 * f64::EPSILON;

    #[test]
    fn unit_vector_preserves_direction_across_finite_magnitudes() {
        for magnitude in [f64::from_bits(1), 1e-310, 1e-200, 1.0, 1e200, f64::MAX] {
            for value in [
                [magnitude, -magnitude, 0.0],
                [0.0, magnitude, -magnitude],
                [-magnitude, 0.0, magnitude],
            ] {
                let result = unit_vector(value).expect("finite nonzero direction");
                for (actual, source) in result.into_iter().zip(value) {
                    let expected = if source == 0.0 {
                        0.0
                    } else {
                        std::f64::consts::FRAC_1_SQRT_2.copysign(source)
                    };
                    assert!((actual - expected).abs() <= EPS_UNIT_ROUNDING);
                }
                assert!(
                    (result[0].hypot(result[1]).hypot(result[2]) - 1.0).abs() <= EPS_UNIT_ROUNDING
                );
            }
        }
    }

    #[test]
    fn unit_vector_rejects_nonfinite_components_on_each_axis() {
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for axis in 0..3 {
                let mut value = [1.0; 3];
                value[axis] = invalid;
                assert!(unit_vector(value).is_none());
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
