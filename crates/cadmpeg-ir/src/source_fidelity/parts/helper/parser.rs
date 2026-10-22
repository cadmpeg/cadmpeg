// SPDX-License-Identifier: Apache-2.0
//! Constrained part output for parsers that build only transient syntax values.

use super::{FinalizedPartOutput, PartOutput};
use crate::CodecFormat;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Parser-only part output with no semantic record insertion or stage access.
///
/// A callback returns its actual syntax value, while the shared helper alone
/// can insert failure stubs, source facts and diagnostics. No callback can add
/// a built record to a failed stage. Actual nested `ctx.record` attempts still
/// contribute dependency-drop seeds, and finalization runs the same dependency
/// closure as semantic output. This is not a bypass for a semantic PartOutput.
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

    /// Finalize actual attempted-record dependencies before releasing output.
    /// Source-byte B1 finalization remains required before export.
    pub fn into_parts(self, ctx: &DecodeContext<'_>) -> Result<FinalizedPartOutput, CodecError> {
        self.0.into_parts(ctx)
    }
}

#[cfg(test)]
mod tests;
