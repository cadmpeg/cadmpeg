// SPDX-License-Identifier: Apache-2.0
//! Checked identity changes for occurrence-local source streams.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use super::{helper::part_id, validate_tree, PartRecord, PartState};
use crate::ids::UnknownId;
use crate::provenance::CodecFormat;
use crate::source_fidelity::SourceFidelity;

/// Address-derived identity changes for one composed part tree.
#[derive(Debug, Default)]
pub struct PartIdentityMap(
    BTreeMap<String, String>,
    crate::source_fidelity::b1::finalization::B1RescopeProof,
);

impl PartIdentityMap {
    /// Move classified source streams admitted by this exact composition proof.
    /// The map must be the unchanged sixth component of the composed fidelity.
    pub fn append_b1_coverage(
        self,
        destination: &mut SourceFidelity,
        coverage: BTreeMap<String, crate::source_fidelity::b1::finalization::ClassifiedStream>,
    ) -> Result<(), CodecError> {
        self.1.append(destination, coverage)
    }

    /// The new canonical identity of a part present in the source tree.
    pub fn get(&self, id: &str) -> Option<&str> {
        self.0.get(id).map(String::as_str)
    }

    /// Check that a composed tree contains exactly this mapping's targets.
    pub fn validate_rescoped_tree(
        &self,
        tree: &BTreeMap<UnknownId, PartRecord>,
    ) -> Result<(), CodecError> {
        if tree.len() != self.0.len() || self.0.values().any(|id| !tree.contains_key(id.as_str())) {
            return Err(CodecError::malformed(
                "part mapping does not cover composed tree",
            ));
        }
        validate_tree(tree).map_err(CodecError::malformed)
    }
}

impl SourceFidelity {
    /// Prefix actual source stream names and recompute every part's identity.
    ///
    /// Parent and covering-ancestor references follow the same mapping. The
    /// returned map must also be applied to document records, annotations,
    /// retained-record keys and layouts by the composition owner. Failure
    /// leaves this tree unchanged.
    pub fn rescope_part_tree(
        &mut self,
        ctx: &DecodeContext<'_>,
        codec: CodecFormat,
        prefix: &str,
    ) -> Result<PartIdentityMap, CodecError> {
        validate_tree(&self.parts).map_err(CodecError::malformed)?;
        let mut ids = BTreeMap::new();
        let mut mapped = BTreeMap::new();
        for (old, node) in &self.parts {
            let expected = part_id(ctx, codec, node.level, &node.stream, node.offset)?;
            if old != &expected {
                return Err(CodecError::malformed(
                    "part identity differs from its source address",
                ));
            }
            let length = prefix
                .len()
                .checked_add(node.stream.len())
                .ok_or_else(|| CodecError::malformed("part stream name length overflows"))?;
            ctx.retain_bytes(u64_from_index(length), "composed part stream")?;
            let mut stream = String::new();
            stream.try_reserve_exact(length).map_err(|_| {
                cadmpeg_core::decode::ResourceLimit::allocation_failed(
                    cadmpeg_core::decode::ResourceDimension::RetainedBytes,
                    ctx.policy().limits.max_retained_bytes,
                    u64_from_index(length),
                    "composed part stream",
                )
            })?;
            stream.push_str(prefix);
            stream.push_str(&node.stream);
            let stream = crate::StreamName::try_from(stream).map_err(CodecError::malformed)?;
            let new = part_id(ctx, codec, node.level, stream.as_str(), node.offset)?;
            let storage = old
                .as_str()
                .len()
                .checked_add(new.as_str().len())
                .and_then(|n| n.checked_add(size_of::<PartRecord>()))
                .and_then(|n| {
                    node.framing_bytes
                        .len()
                        .checked_mul(size_of::<std::ops::Range<u64>>())
                        .and_then(|frames| n.checked_add(frames))
                })
                .ok_or_else(|| CodecError::malformed("composed part tree storage overflows"))?;
            ctx.retain_bytes(u64_from_index(storage), "composed part tree")?;
            ids.insert(old.as_str().to_owned(), new.as_str().to_owned());
            let mut node = node.clone();
            node.stream = stream.as_str().to_owned();
            if mapped.insert(new, node).is_some() {
                return Err(CodecError::malformed("composed part identity collision"));
            }
        }
        let coverage = crate::source_fidelity::b1::finalization::B1RescopeProof::stage(
            ctx,
            &self.b1_spans,
            prefix,
        )?;
        let ids = PartIdentityMap(ids, coverage);
        for node in mapped.values_mut() {
            if let Some(parent) = &mut node.parent {
                *parent = mapped_identity(ctx, &ids, parent)?;
            }
            if let PartState::Covered { by } = &mut node.state {
                *by = mapped_identity(ctx, &ids, by)?;
            }
        }
        validate_tree(&mapped).map_err(CodecError::malformed)?;
        self.parts = mapped;
        Ok(ids)
    }

    /// Append a complete reached tree after validating ancestry and collisions.
    /// Failure leaves both the existing tree and other fidelity fields unchanged.
    pub fn append_part_tree(
        &mut self,
        mut parts: BTreeMap<UnknownId, PartRecord>,
    ) -> Result<(), CodecError> {
        validate_tree(&parts).map_err(CodecError::malformed)?;
        if parts.keys().any(|id| self.parts.contains_key(id)) {
            return Err(CodecError::malformed("duplicate composed source part"));
        }
        self.parts.append(&mut parts);
        Ok(())
    }
}

fn mapped_identity(
    ctx: &DecodeContext<'_>,
    ids: &PartIdentityMap,
    id: &UnknownId,
) -> Result<UnknownId, CodecError> {
    let mapped = ids
        .get(id.as_str())
        .ok_or_else(|| CodecError::malformed("composed part ancestry target is absent"))?;
    ctx.retain_bytes(
        u64_from_index(mapped.len()),
        "composed part ancestor identity",
    )?;
    UnknownId::try_from(mapped.to_owned()).map_err(CodecError::malformed)
}

#[cfg(test)]
mod tests;
