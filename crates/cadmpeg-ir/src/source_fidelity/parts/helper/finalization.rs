// SPDX-License-Identifier: Apache-2.0
//! Owned dependency finalization before a part stage releases document fields.

use super::{DeferredRecordPruning, PartDependencyPruning, PartOutput};
use crate::document::Model;
use crate::native::Native;
use crate::report::loss::LossNote;
use crate::SourceFidelity;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

/// Result of enforcing dependency pruning at the owned output boundary.
#[derive(Debug)]
pub enum PartFinalization {
    /// Every surviving typed reference has a representable dependency result.
    Ready(FinalizedPartOutput),
    /// No tuple is released; the unchanged stage can be inspected or retried.
    Deferred {
        /// The still-owned stage, including all original records and losses.
        output: PartOutput,
        /// Actual typed owners whose list deletion remains unrepresentable.
        records: Vec<DeferredRecordPruning>,
    },
}

/// A dependency-closed stage whose component tuple can be released.
///
/// The constructor is private. Source-byte B1 derivation remains a separate
/// required operation before the document is exportable.
#[derive(Debug)]
pub struct FinalizedPartOutput(PartOutput);

impl FinalizedPartOutput {
    // ParserPartOutput has no semantic insertion API. The only native records
    // the shared helper adds are stubs, and its failed callbacks stage no IDs.
    pub(super) fn from_parser(output: super::ParserPartOutput) -> Self {
        Self(output.0)
    }

    /// Consume dependency-closed records and their source facts.
    pub fn into_parts(self) -> (Model, Native, SourceFidelity, Vec<LossNote>) {
        (self.0.model, self.0.native, self.0.fidelity, self.0.losses)
    }
}

impl PartOutput {
    /// Enforce current typed dependency closure before releasing any components.
    ///
    /// Required records are dropped and variable reference lists are pruned by
    /// their actual owners. A deferred result retains this complete stage and
    /// exposes no component tuple. Session/Part resource errors retain their
    /// actual CodecError; no implementation deferral becomes a fake parse error.
    pub fn into_parts(mut self, ctx: &DecodeContext<'_>) -> Result<PartFinalization, CodecError> {
        match self.prune_dependencies(ctx)? {
            PartDependencyPruning::Complete { .. } => {
                Ok(PartFinalization::Ready(FinalizedPartOutput(self)))
            }
            PartDependencyPruning::Deferred { records } => Ok(PartFinalization::Deferred {
                output: self,
                records,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
