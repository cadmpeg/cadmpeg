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
pub struct FinalizedPartOutput(PartOutput, PrunedPartCaptures);

/// Actual dependency-pruned identities from one finalized producer stage.
///
/// ```compile_fail
/// use cadmpeg_ir::source_fidelity::parts::PrunedPartCaptures;
/// let forged = PrunedPartCaptures { dropped: vec!["arbitrary".into()] };
/// ```
#[derive(Debug)]
#[must_use]
pub struct PrunedPartCaptures {
    pub(super) session: cadmpeg_core::decode::DecodeSessionToken,
    pub(super) dropped: Vec<(String, Option<cadmpeg_core::decode::RecordBatchId>)>,
}

impl PrunedPartCaptures {
    /// Whether this stage removed the actual unbatched typed record.
    pub fn was_pruned(&self, id: &str) -> bool {
        self.was_pruned_in_batch(id, None)
    }

    /// Whether this stage removed a record in the actual member namespace.
    pub fn was_pruned_in_batch(
        &self,
        id: &str,
        batch: Option<cadmpeg_core::decode::RecordBatchId>,
    ) -> bool {
        self.dropped
            .binary_search_by(|(candidate, owner)| {
                candidate.as_str().cmp(id).then_with(|| owner.cmp(&batch))
            })
            .is_ok()
    }

    /// Combine genuine finalized stages from this same live decode session.
    pub fn append(&mut self, ctx: &DecodeContext<'_>, other: Self) -> Result<(), CodecError> {
        let session = ctx.session_token()?;
        if self.session != session || other.session != session {
            return Err(CodecError::malformed(
                "capture pruning belongs to another decode session",
            ));
        }
        ctx.reserve_working(
            &mut self.dropped,
            other.dropped.len(),
            "part capture pruning working storage",
        )?;
        self.dropped.extend(other.dropped);
        self.dropped.sort_unstable();
        self.dropped.dedup();
        Ok(())
    }
}

impl FinalizedPartOutput {
    /// Release the stage together with its one-shot capture-pruning authority.
    pub fn into_parts_with_capture_pruning(
        self,
    ) -> (
        Model,
        Native,
        SourceFidelity,
        Vec<LossNote>,
        PrunedPartCaptures,
    ) {
        (
            self.0.model,
            self.0.native,
            self.0.fidelity,
            self.0.losses,
            self.1,
        )
    }
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
        let session = self.bind_context(ctx)?;
        let PartDependencyPruning::Complete { dropped_records } = self.prune_dependencies(ctx)?;
        let mut dropped = Vec::new();
        ctx.reserve_working(
            &mut dropped,
            self.record_batches.len(),
            "part capture pruning working storage",
        )?;
        for (id, batch) in std::mem::take(&mut self.record_batches) {
            if dropped_records.binary_search(&id).is_ok() {
                dropped.push((id, batch));
            }
        }
        dropped.sort_unstable();
        dropped.dedup();
        Ok(FinalizedPartOutput(
            self,
            PrunedPartCaptures { session, dropped },
        ))
    }
}

#[cfg(test)]
mod tests;
