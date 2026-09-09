// SPDX-License-Identifier: Apache-2.0
//! Admitted placement frames and scale vectors.

use cadmpeg_ir::transform::Transform;
use serde::{Deserialize, Serialize};

/// A finite right-handed orthonormal affine frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub struct FiniteFrame(Transform);

impl TryFrom<[[f64; 4]; 4]> for FiniteFrame {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        Transform::from_rows(rows)
            .filter(Transform::is_proper_rigid)
            .map(Self)
            .ok_or_else(|| "frame must be finite, affine, and orthonormal".to_owned())
    }
}
impl From<FiniteFrame> for [[f64; 4]; 4] {
    fn from(value: FiniteFrame) -> Self {
        value.rows()
    }
}
impl FiniteFrame {
    pub(crate) fn rows(self) -> [[f64; 4]; 4] {
        self.0.rows()
    }
    pub(crate) fn transform(self) -> Transform {
        self.0
    }
}

/// A scale vector with finite components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[f64; 3]", into = "[f64; 3]")]
pub struct FiniteVec3([f64; 3]);
impl TryFrom<[f64; 3]> for FiniteVec3 {
    type Error = String;
    fn try_from(values: [f64; 3]) -> Result<Self, Self::Error> {
        if values.iter().any(|value| !value.is_finite()) {
            return Err("scale vector components must be finite".to_owned());
        }
        Ok(Self(values))
    }
}
impl From<FiniteVec3> for [f64; 3] {
    fn from(value: FiniteVec3) -> Self {
        value.0
    }
}
impl FiniteVec3 {
    pub(crate) fn values(self) -> [f64; 3] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_reject_nonfinite_cells_and_nonorthonormal_axes() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for row in 0..4 {
                for column in 0..4 {
                    let mut matrix = Transform::identity().rows();
                    matrix[row][column] = value;
                    assert!(FiniteFrame::try_from(matrix).is_err());
                }
            }
            assert!(FiniteVec3::try_from([value, 1.0, 1.0]).is_err());
        }
        let mut matrix = Transform::identity().rows();
        matrix[0][0] = 2.0;
        assert!(FiniteFrame::try_from(matrix).is_err());
        assert!(serde_json::from_value::<FiniteFrame>(serde_json::json!(matrix)).is_err());
        assert!(FiniteVec3::try_from([-1.0, 0.0, 2.0]).is_ok());
    }
}
