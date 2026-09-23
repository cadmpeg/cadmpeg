// SPDX-License-Identifier: Apache-2.0
//! Admitted placement frames.

use cadmpeg_ir::transform::Transform;
use serde::{Deserialize, Serialize};

/// A finite right-handed orthonormal affine frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "[[f64; 4]; 4]", into = "[[f64; 4]; 4]")]
pub(crate) struct FiniteFrame(Transform);

impl TryFrom<[[f64; 4]; 4]> for FiniteFrame {
    type Error = String;
    fn try_from(rows: [[f64; 4]; 4]) -> Result<Self, Self::Error> {
        (rows[3] == [0.0, 0.0, 0.0, 1.0])
            .then(|| Transform::affine([rows[0], rows[1], rows[2]]))
            .flatten()
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

#[cfg(test)]
mod tests {
    use cadmpeg_ir::transform::Transform;

    use cadmpeg_ir::units::FiniteVector;

    use super::FiniteFrame;

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
            assert!(FiniteVector::new([value, 1.0, 1.0]).is_none());
        }
        let mut matrix = Transform::identity().rows();
        matrix[0][0] = 2.0;
        assert!(FiniteFrame::try_from(matrix).is_err());
        assert!(serde_json::from_value::<FiniteFrame>(serde_json::json!(matrix)).is_err());
        assert!(FiniteVector::new([-1.0, 0.0, 2.0]).is_some());
    }
}
