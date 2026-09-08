// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use serde::{Deserialize, Serialize};

/// A finite matrix whose implicit cells agree with its compact encoding masks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CompactMatrixWire", into = "CompactMatrixWire")]
pub(crate) struct CompactMatrix {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[f64; 4]; 4],
}

#[derive(Serialize, Deserialize)]
struct CompactMatrixWire {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[f64; 4]; 4],
}

impl CompactMatrix {
    /// Constructs the matrix from masks and row-major explicit values.
    pub(crate) fn try_new(
        value_mask: u16,
        zero_mask: u16,
        mut explicit: impl FnMut(usize) -> Result<f64, CodecError>,
    ) -> Result<Self, CodecError> {
        let mut matrix = [[0.0; 4]; 4];
        for (index, value) in matrix.iter_mut().flatten().enumerate() {
            let bit = 1u16 << index;
            *value = match (value_mask & bit != 0, zero_mask & bit != 0) {
                (false, false) => explicit(index)?,
                (true, false) => 1.0,
                (false, true) => 0.0,
                (true, true) => -1.0,
            };
            if !value.is_finite() {
                return Err(CodecError::malformed(format_args!(
                    "compact matrix[{index}] is not finite"
                )));
            }
        }
        Ok(Self {
            value_mask,
            zero_mask,
            matrix,
        })
    }

    /// Admits finite rows that agree with the compact encoding masks.
    pub(crate) fn try_from_rows(
        value_mask: u16,
        zero_mask: u16,
        matrix: [[f64; 4]; 4],
    ) -> Result<Self, CodecError> {
        let expected = Self::try_new(value_mask, zero_mask, |index| {
            Ok(matrix[index / 4][index % 4])
        })?;
        if expected.matrix != matrix {
            return Err(CodecError::Malformed(
                "compact matrix disagrees with value_mask or zero_mask".into(),
            ));
        }
        Ok(Self { matrix, ..expected })
    }

    /// The admitted matrix rows.
    pub(crate) fn rows(&self) -> [[f64; 4]; 4] {
        self.matrix
    }
}

impl TryFrom<CompactMatrixWire> for CompactMatrix {
    type Error = CodecError;

    fn try_from(wire: CompactMatrixWire) -> Result<Self, Self::Error> {
        Self::try_from_rows(wire.value_mask, wire.zero_mask, wire.matrix)
    }
}

impl From<CompactMatrix> for CompactMatrixWire {
    fn from(value: CompactMatrix) -> Self {
        Self {
            value_mask: value.value_mask,
            zero_mask: value.zero_mask,
            matrix: value.matrix,
        }
    }
}

/// The assembly placement transform wire fields.
pub(crate) mod assembly_wire {
    use super::CompactMatrix;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Wire {
        transform_encoding: [u16; 2],
        transform: [[f64; 4]; 4],
    }

    /// Serializes an assembly compact matrix.
    pub(crate) fn serialize<S: Serializer>(
        matrix: &CompactMatrix,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        Wire {
            transform_encoding: [matrix.value_mask, matrix.zero_mask],
            transform: matrix.matrix,
        }
        .serialize(serializer)
    }

    /// Deserializes an assembly compact matrix.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<CompactMatrix, D::Error> {
        let wire = Wire::deserialize(deserializer)?;
        CompactMatrix::try_from_rows(
            wire.transform_encoding[0],
            wire.transform_encoding[1],
            wire.transform,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::CompactMatrix;

    #[test]
    fn masks_select_explicit_positive_zero_and_negative_cells() {
        let mut indices = Vec::new();
        let matrix = CompactMatrix::try_new(0x000a, 0xfffc, |index| {
            indices.push(index);
            Ok(2.5)
        })
        .unwrap();
        assert_eq!(indices, [0]);
        assert_eq!(matrix.rows()[0], [2.5, 1.0, 0.0, -1.0]);
        let wire = serde_json::to_value(matrix).unwrap();
        assert_eq!(
            serde_json::from_value::<CompactMatrix>(wire.clone()).unwrap(),
            matrix
        );
        for column in 1..4 {
            let mut invalid = wire.clone();
            invalid["matrix"][0][column] = serde_json::json!(3.0);
            assert!(serde_json::from_value::<CompactMatrix>(invalid).is_err());
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(CompactMatrix::try_new(0, 0, |_| Ok(value)).is_err());
            let mut rows = matrix.rows();
            rows[0][1] = value;
            assert!(CompactMatrix::try_from_rows(0x000a, 0xfffc, rows).is_err());
        }
    }
}
