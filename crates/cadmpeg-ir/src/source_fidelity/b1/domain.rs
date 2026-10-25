// SPDX-License-Identifier: Apache-2.0
//! Opaque input-root domains preserve an enclosing caller's source coordinates.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, SourceExtentProof, SourceOrigin, View};
use cadmpeg_core::CodecError;
use std::ops::Range;
use std::sync::Arc;

#[derive(Debug)]
struct RootProof {
    source: SourceExtentProof,
    byte_len: u64,
}

/// The exact codec input window in its original registered physical stream.
///
/// A bounded physical root may belong to an enclosing caller. Ordinary member
/// streams still require their complete domain; a partial alias or expansion
/// cannot acquire this privilege.
#[derive(Debug, Clone)]
pub struct SourceRootDomain(Arc<RootProof>);

impl PartialEq for SourceRootDomain {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for SourceRootDomain {}

impl SourceRootDomain {
    /// Bind the actual codec root to its live source session before capture seal.
    pub fn capture(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Self, CodecError> {
        let source = ctx.source_extent_proof(root)?;
        let streams = ctx.source_streams()?;
        let stream = streams
            .iter()
            .find(|stream| stream.name() == source.stream())
            .ok_or_else(|| CodecError::malformed("codec root has no actual source stream"))?;
        if source.range().end > stream.byte_len()
            || ((source.range().start != 0 || source.range().end != stream.byte_len())
                && stream.origin() != &SourceOrigin::Root)
        {
            return Err(CodecError::malformed(
                "partial member cannot supply a codec root domain",
            ));
        }
        ctx.retain_bytes(
            u64_from_index(size_of::<RootProof>() + 2 * size_of::<usize>()),
            "codec root source domain authority",
        )?;
        Ok(Self(Arc::new(RootProof {
            source,
            byte_len: stream.byte_len(),
        })))
    }

    /// Registered stream name, with its original coordinate origin.
    pub fn stream(&self) -> &str {
        self.0.source.stream()
    }

    /// Complete physical stream length, including an enclosing caller's bytes.
    pub fn byte_len(&self) -> u64 {
        self.0.byte_len
    }

    /// Exact input window; bytes outside it belong to the enclosing caller.
    pub fn range(&self) -> Range<u64> {
        self.0.source.range().start..self.0.source.range().end
    }

    pub(super) fn validate(&self, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
        ctx.require_source_membership(&self.0.source)?;
        let streams = ctx.source_streams()?;
        let stream = streams
            .iter()
            .find(|stream| stream.name() == self.stream())
            .ok_or_else(|| {
                CodecError::malformed("codec root domain has no original source stream")
            })?;
        if stream.byte_len() != self.byte_len()
            || self.range().end > stream.byte_len()
            || ((self.range().start != 0 || self.range().end != stream.byte_len())
                && stream.origin() != &SourceOrigin::Root)
        {
            return Err(CodecError::malformed(
                "codec root domain differs from original source provenance",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
