// SPDX-License-Identifier: Apache-2.0
//! Failures returned while streaming STEP output.

use cadmpeg_core::CodecError;

/// Failure returned while streaming STEP output.
///
/// Unsupported or reduced IR content appears in [`cadmpeg_ir::report::ExportReport::losses`] after a
/// successful write.
#[derive(Debug, thiserror::Error)]
pub enum StepError {
    /// The output sink rejected a write.
    #[error("failed to write STEP output: {0}")]
    Io(#[from] std::io::Error),
}

impl From<StepError> for CodecError {
    fn from(error: StepError) -> Self {
        match error {
            StepError::Io(error) => Self::Io(error),
        }
    }
}
