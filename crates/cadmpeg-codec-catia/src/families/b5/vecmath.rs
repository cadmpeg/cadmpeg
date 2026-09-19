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

#[cfg(test)]
mod tests {
    use crate::math::unit_vector;

    #[test]
    fn unit_preserves_tiny_finite_direction() {
        assert_eq!(unit_vector([1e-200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
        assert_eq!(unit_vector([0.0, 0.0, 0.0]), None);
    }

    #[test]
    fn unit_preserves_subnormal_direction() {
        assert_eq!(unit_vector([1e-310, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
    }

    #[test]
    fn unit_rejects_nonfinite_length() {
        for value in [[f64::INFINITY, 0.0, 0.0], [f64::NAN, 0.0, 0.0]] {
            assert_eq!(unit_vector(value), None);
        }
    }
}
