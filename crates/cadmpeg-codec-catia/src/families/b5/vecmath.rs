// SPDX-License-Identifier: Apache-2.0
//! `[f64; 3]` vector helpers shared by the b5 parse graph and IR transfer.

use cadmpeg_ir::math::Vector3;

pub(super) fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    (Vector3::from(left) + Vector3::from(right)).into()
}

pub(super) fn scale(value: [f64; 3], scalar: f64) -> [f64; 3] {
    Vector3::from(value).scale(scalar).into()
}

pub(super) fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    Vector3::from(left).cross(Vector3::from(right)).into()
}

/// Normalize a direction with finite, nonzero length.
///
/// Divide each component directly: the reciprocal of a subnormal length
/// can overflow even when the normalized components are finite.
pub(super) fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = value[0].hypot(value[1]).hypot(value[2]);
    (length.is_finite() && length != 0.0)
        .then(|| [value[0] / length, value[1] / length, value[2] / length])
}

#[cfg(test)]
mod tests {
    use super::unit;

    #[test]
    fn unit_preserves_tiny_finite_direction() {
        assert_eq!(unit([1e-200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
        assert_eq!(unit([0.0, 0.0, 0.0]), None);
    }

    #[test]
    fn unit_preserves_subnormal_direction() {
        assert_eq!(unit([1e-310, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn unit_rejects_nonfinite_length() {
        for value in [
            [f64::INFINITY, 0.0, 0.0],
            [f64::NAN, 0.0, 0.0],
            [f64::MAX, f64::MAX, 0.0],
        ] {
            assert_eq!(unit(value), None);
        }
    }
}
