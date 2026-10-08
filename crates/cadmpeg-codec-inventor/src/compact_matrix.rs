// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use serde::{Deserialize, Serialize};

/// A finite matrix whose implicit cells agree with its compact encoding masks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(into = "CompactMatrixWire")]
pub(crate) struct CompactMatrix {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[FiniteReal; 4]; 4],
}

#[derive(Serialize, Deserialize)]
pub(crate) struct CompactMatrixWire {
    value_mask: u16,
    zero_mask: u16,
    matrix: [[f64; 4]; 4],
}

impl CompactMatrix {
    pub(crate) fn masks(&self) -> (u16, u16) {
        (self.value_mask, self.zero_mask)
    }

    /// Constructs the matrix from masks and row-major explicit values.
    pub(crate) fn try_new(
        value_mask: u16,
        zero_mask: u16,
        mut explicit: impl FnMut(usize) -> Result<FiniteReal, CodecError>,
    ) -> Result<Self, CodecError> {
        let mut matrix = [[FiniteReal::ZERO; 4]; 4];
        for index in 0_usize..16 {
            let value = &mut matrix[index / 4][index % 4];
            let bit = 1u16 << index;
            *value = match (value_mask & bit != 0, zero_mask & bit != 0) {
                (false, false) => explicit(index)?,
                (true, false) => FiniteReal::ONE,
                (false, true) => FiniteReal::ZERO,
                (true, true) => FiniteReal::ONE.negated(),
            };
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
            FiniteReal::new(matrix[index / 4][index % 4]).ok_or_else(|| {
                CodecError::malformed(format_args!("compact matrix[{index}] is not finite"))
            })
        })?;
        if expected.rows() != matrix {
            return Err(CodecError::Malformed(
                "compact matrix disagrees with value_mask or zero_mask".into(),
            ));
        }
        Ok(expected)
    }

    /// The admitted matrix rows.
    pub(crate) fn rows(&self) -> [[f64; 4]; 4] {
        self.matrix.map(|row| row.map(FiniteReal::get))
    }

    /// The admitted matrix cells for transfer into a checked transform.
    pub(crate) fn checked_rows(&self) -> [[FiniteReal; 4]; 4] {
        self.matrix
    }
}

impl CompactMatrixWire {
    pub(crate) fn into_matrix(self) -> Result<CompactMatrix, CodecError> {
        CompactMatrix::try_from_rows(self.value_mask, self.zero_mask, self.matrix)
    }
}

impl From<CompactMatrix> for CompactMatrixWire {
    fn from(value: CompactMatrix) -> Self {
        Self {
            value_mask: value.value_mask,
            zero_mask: value.zero_mask,
            matrix: value.rows(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CompactMatrix, CompactMatrixWire};

    #[test]
    fn compact_matrix_fixed_cells_need_no_work_admission() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let matrix = CompactMatrix::try_from_rows(0, 0, [[2.5; 4]; 4])
            .expect("sixteen fixed cells need no variable work");
        assert_eq!(matrix.rows(), [[2.5; 4]; 4]);
        let matrix = CompactMatrix::try_new(u16::MAX, 0, |_| {
            panic!("implicit cells do not read explicit values")
        })
        .expect("implicit cells need no variable work");
        assert_eq!(matrix.rows(), [[1.0; 4]; 4]);
        ctx.finish_session().expect("fixed cells use no decode work");
    }

    #[test]
    fn masks_select_explicit_positive_zero_and_negative_cells() {
        let mut indices = Vec::new();
        let matrix = CompactMatrix::try_new(0x000a, 0xfffc, |index| {
            indices.push(index);
            Ok(cadmpeg_ir::scalar::FiniteReal::new(2.5).expect("finite matrix cell"))
        })
        .expect("matrix fixture agrees with its masks");
        assert_eq!(indices, [0]);
        assert_eq!(matrix.rows()[0], [2.5, 1.0, 0.0, -1.0]);
        let wire = serde_json::to_value(matrix).expect("matrix fixture agrees with its masks");
        let decoded = serde_json::from_value::<CompactMatrixWire>(wire.clone())
            .expect("matrix fixture agrees with its masks")
            .into_matrix()
            .expect("matrix fixture agrees with its masks");
        assert_eq!(decoded, matrix);
        for column in 1..4 {
            let mut invalid = wire.clone();
            invalid["matrix"][0][column] = serde_json::json!(3.0);
            assert!(serde_json::from_value::<CompactMatrixWire>(invalid)
                .expect("matrix wire fixture")
                .into_matrix()
                .is_err());
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(CompactMatrix::try_from_rows(0, 0, [[value; 4]; 4]).is_err());
            let mut rows = matrix.rows();
            rows[0][1] = value;
            assert!(CompactMatrix::try_from_rows(0x000a, 0xfffc, rows).is_err());
        }
    }
}
