// SPDX-License-Identifier: Apache-2.0
//! Finite model-space endpoint coordinates.

use cadmpeg_ir::math::Point3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub(crate) struct FinitePoint([f64; 3]);

impl TryFrom<[f64; 3]> for FinitePoint {
    type Error = &'static str;
    fn try_from(point: [f64; 3]) -> Result<Self, Self::Error> {
        if point.iter().all(|value| value.is_finite()) {
            Ok(Self(point))
        } else {
            Err("point: coordinates must be finite")
        }
    }
}

impl From<FinitePoint> for [f64; 3] {
    fn from(point: FinitePoint) -> Self {
        point.0
    }
}

impl From<FinitePoint> for Point3 {
    fn from(point: FinitePoint) -> Self {
        let [x, y, z] = point.0;
        Self::new(x, y, z)
    }
}

#[cfg(test)]
mod tests {
    use super::FinitePoint;

    #[test]
    fn endpoint_coordinates_preserve_subnormal_values_and_signed_zero() {
        let point = FinitePoint::try_from([f64::from_bits(1), -0.0, f64::MAX]).unwrap();
        let json = serde_json::to_string(&point).unwrap();
        let round_trip: FinitePoint = serde_json::from_str(&json).unwrap();
        assert_eq!(
            <[f64; 3]>::from(round_trip).map(f64::to_bits),
            <[f64; 3]>::from(point).map(f64::to_bits)
        );
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(FinitePoint::try_from([value, 0.0, 0.0])
                .unwrap_err()
                .contains("point"));
        }
    }
}
