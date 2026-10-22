// SPDX-License-Identifier: Apache-2.0
//! Owned dependency finalization before a part stage releases document fields.

use super::{PartDependencyPruning, PartOutput};
use crate::document::Model;
use crate::native::Native;
use crate::report::loss::LossNote;
use crate::SourceFidelity;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// A dependency-closed stage whose component tuple can be released.
///
/// The constructor is private. Source-byte B1 derivation remains a separate
/// required operation before the document is exportable.
#[derive(Debug)]
pub struct FinalizedPartOutput(PartOutput);

impl FinalizedPartOutput {
    /// Consume dependency-closed records and their source facts.
    pub fn into_parts(self) -> (Model, Native, SourceFidelity, Vec<LossNote>) {
        (self.0.model, self.0.native, self.0.fidelity, self.0.losses)
    }
}

impl PartOutput {
    /// Enforce current typed dependency closure before releasing any components.
    ///
    /// Required records are dropped, records that cannot be formed without a
    /// dropped record are dropped with them, and variable reference lists are
    /// pruned by their actual owners. Session/Part resource errors retain
    /// their actual CodecError.
    pub fn into_parts(
        mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<FinalizedPartOutput, CodecError> {
        let PartDependencyPruning::Complete { .. } = self.prune_dependencies(ctx)?;
        Ok(FinalizedPartOutput(self))
    }
}

#[cfg(test)]
mod tests;
