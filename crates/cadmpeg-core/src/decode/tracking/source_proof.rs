// SPDX-License-Identifier: Apache-2.0
//! Opaque source ancestry evidence for independently scheduled source parts.

use super::RecordBatchId;
use crate::decode::{ByteRange, DecodeContext, View};
use crate::CodecError;

/// One exact source extent bound to its live decode session and capture batch.
#[derive(Debug)]
pub struct SourceExtentProof {
    session: std::sync::Weak<()>,
    batch: Option<RecordBatchId>,
    stream: String,
    range: ByteRange,
}

impl SourceExtentProof {
    /// Canonical name of the registered source stream.
    pub fn stream(&self) -> &str {
        &self.stream
    }

    /// Exact original bounds of the proved source extent.
    pub const fn range(&self) -> ByteRange {
        self.range
    }
}

impl DecodeContext<'_> {
    /// Require the original live session and batch of an actual source proof.
    pub fn require_source_authority(&self, proof: &SourceExtentProof) -> Result<(), CodecError> {
        self.require_source_membership(proof)?;
        self.budget.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "source proof capture batch is not active",
                ));
            }
            Ok(())
        })
    }

    /// Verify a source proof's original session and batch without granting reads.
    ///
    /// This read-only check also works after capture extraction. It cannot
    /// reopen record scopes or grant any of the mutation authority required by
    /// [`Self::require_source_authority`].
    pub fn require_source_membership(&self, proof: &SourceExtentProof) -> Result<(), CodecError> {
        self.budget.session(|session| {
            let authority = proof
                .session
                .upgrade()
                .ok_or_else(|| CodecError::malformed("source proof decode session has ended"))?;
            if !session
                .source_identity
                .as_ref()
                .is_some_and(|identity| std::sync::Arc::ptr_eq(&authority, identity))
            {
                return Err(CodecError::malformed(
                    "source proof belongs to another decode session",
                ));
            }
            if session.frames.iter().rev().find_map(|frame| frame.batch) != proof.batch {
                return Err(CodecError::malformed(
                    "source proof capture batch is not active",
                ));
            }
            Ok(())
        })
    }

    /// Prove the actual registered source extent without granting semantic reads.
    pub fn source_extent_proof(&self, extent: View<'_>) -> Result<SourceExtentProof, CodecError> {
        self.require_source(extent)?;
        self.budget.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed("source proof session is sealed"));
            }
            let range = ByteRange {
                start: crate::decode::u64_from_index(extent.start()),
                end: crate::decode::u64_from_index(extent.end()),
            };
            let name = session.streams.extent_name(extent.space(), range)?;
            let stream = super::copy_string(&session.budget, name)?;
            let batch = session.frames.iter().rev().find_map(|frame| frame.batch);
            Ok(SourceExtentProof {
                session: super::source_identity(session)?,
                batch,
                stream,
                range,
            })
        })
    }

    /// Require every registered origin path of `child` to lie inside this proof.
    pub fn require_source_ancestry(
        &self,
        proof: &SourceExtentProof,
        child: View<'_>,
    ) -> Result<(), CodecError> {
        self.require_source(child)?;
        self.budget.session(|session| {
            let authority = proof
                .session
                .upgrade()
                .ok_or_else(|| CodecError::malformed("source proof decode session has ended"))?;
            if !session
                .source_identity
                .as_ref()
                .is_some_and(|identity| std::sync::Arc::ptr_eq(&authority, identity))
            {
                return Err(CodecError::malformed(
                    "source proof belongs to another decode session",
                ));
            }
            if session.sealed
                || session.frames.iter().rev().find_map(|frame| frame.batch) != proof.batch
            {
                return Err(CodecError::malformed(
                    "source proof capture batch is not active",
                ));
            }
            if !session
                .streams
                .descends(&session.budget, child, &proof.stream, proof.range)?
            {
                return Err(CodecError::malformed(
                    "part source does not descend from its declared parent",
                ));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests;
