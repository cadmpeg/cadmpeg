// SPDX-License-Identifier: Apache-2.0
//! Constrained part output for parsers that build only transient syntax values.

use super::{FinalizedPartOutput, PartOutput};
use crate::CodecFormat;

/// Parser-only part output with no semantic record insertion or stage access.
///
/// A callback returns its actual syntax value, while the shared helper alone
/// can insert failure stubs, source facts and diagnostics. No callback can add
/// a built record ID to a failed stage, so the dependency-drop seed set is empty
/// by construction. This is not a bypass for a semantic PartOutput.
///
/// ```compile_fail
/// use cadmpeg_ir::source_fidelity::parts::ParserPartOutput;
/// let parser = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Step);
/// let semantic_stage = parser.0;
/// ```
#[derive(Debug)]
pub struct ParserPartOutput(pub(super) PartOutput);

impl ParserPartOutput {
    /// Begin source-addressed syntax parsing for one codec.
    pub fn new(codec: CodecFormat) -> Self {
        Self(PartOutput::new(codec))
    }

    /// Release dependency-closed output whose callbacks cannot build records.
    /// Source-byte B1 finalization remains required before export.
    pub fn into_parts(self) -> FinalizedPartOutput {
        FinalizedPartOutput::from_parser(self)
    }
}

#[cfg(test)]
mod tests;
