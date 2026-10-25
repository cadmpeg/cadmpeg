// SPDX-License-Identifier: Apache-2.0
//! Declared source parents backed by successful staged part invocations.

use std::sync::{Arc, Weak};

use cadmpeg_core::decode::{DecodeContext, SourceExtentProof, View};
use cadmpeg_core::CodecError;

use super::PartOutput;
use crate::ids::UnknownId;
use crate::source_fidelity::parts::{PartLevel, PartState};

#[derive(Debug)]
pub(super) struct ParentAnchor {
    pub(super) id: UnknownId,
    source: SourceExtentProof,
}

/// Authority to schedule a source child beneath a successful existing part.
/// The authority expires when its containing output transaction is discarded.
#[derive(Debug, Clone)]
pub struct PartParent(Weak<ParentAnchor>);

impl PartParent {
    pub(super) fn validate(
        &self,
        ctx: &DecodeContext<'_>,
        child: View<'_>,
    ) -> Result<UnknownId, CodecError> {
        let anchor = self
            .0
            .upgrade()
            .ok_or_else(|| CodecError::malformed("declared part parent was rolled back"))?;
        ctx.require_source_ancestry(&anchor.source, child)?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(anchor.id.as_str().len()),
            "part parent identity",
        )?;
        Ok(anchor.id.clone())
    }
}

impl PartOutput {
    /// Whether a genuine staged invocation already belongs to this source parent.
    pub fn has_descendant(&self, parent: &PartParent) -> Result<bool, CodecError> {
        let anchor = parent
            .0
            .upgrade()
            .ok_or_else(|| CodecError::malformed("declared part parent was rolled back"))?;
        if !self
            .parents
            .get(&anchor.id)
            .is_some_and(|held| Arc::ptr_eq(held, &anchor))
        {
            return Err(CodecError::malformed(
                "source parent belongs to another output",
            ));
        }
        // A staged descendant has its complete ancestor chain in this output,
        // including the immediate child of the proven parent.
        Ok(self
            .fidelity
            .parts
            .values()
            .any(|node| node.parent() == Some(&anchor.id)))
    }

    /// Obtain a source parent only from an actual successful part in this stage.
    pub fn parent(
        &mut self,
        ctx: &DecodeContext<'_>,
        extent: View<'_>,
        level: &str,
    ) -> Result<PartParent, CodecError> {
        let level = PartLevel::parse(level).ok_or_else(|| {
            CodecError::malformed("declared parent level is absent from Appendix C")
        })?;
        let source = ctx.source_extent_proof(extent)?;
        let id = self
            .fidelity
            .parts
            .iter()
            .find(|(_, record)| {
                record.level() == level
                    && record.stream() == source.stream()
                    && record.offset() == cadmpeg_core::decode::u64_from_index(extent.start())
                    && record.byte_len()
                        == cadmpeg_core::decode::u64_from_index(extent.window().len())
                    && matches!(record.state(), PartState::Decoded {})
            })
            .map(|(id, _)| id)
            .ok_or_else(|| {
                CodecError::malformed("declared parent has no successful matching invocation")
            })?;
        if let Some(anchor) = self.parents.get(id) {
            return Ok(PartParent(Arc::downgrade(anchor)));
        }
        let bytes = id
            .as_str()
            .len()
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(size_of::<ParentAnchor>()))
            .and_then(|bytes| bytes.checked_add(size_of::<(UnknownId, Arc<ParentAnchor>)>()))
            .and_then(|bytes| bytes.checked_add(size_of::<[usize; 2]>()))
            .ok_or_else(|| CodecError::malformed("source parent authority storage overflows"))?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(bytes),
            "source parent authority",
        )?;
        let id = id.clone();
        let anchor = Arc::new(ParentAnchor {
            id: id.clone(),
            source,
        });
        let parent = PartParent(Arc::downgrade(&anchor));
        self.parents.insert(id, anchor);
        Ok(parent)
    }
}

pub(super) fn independent_state(
    ctx: &DecodeContext<'_>,
    staged: &mut PartOutput,
    enclosing: &UnknownId,
) -> Result<PartOutput, CodecError> {
    let mut independent = std::collections::BTreeSet::new();
    for (id, node) in &staged.fidelity.parts {
        let mut parent = node.parent();
        let mut enclosed = false;
        while let Some(id) = parent {
            if id == enclosing {
                enclosed = true;
                break;
            }
            parent = staged
                .fidelity
                .parts
                .get(id)
                .and_then(crate::source_fidelity::parts::PartRecord::parent);
        }
        if !enclosed {
            independent.insert(id.clone());
        }
    }
    let mut preserved = PartOutput::stage(staged.codec);
    for id in &independent {
        if let Some(node) = staged.fidelity.parts.remove(id) {
            preserved.fidelity.parts.insert(id.clone(), node);
        }
        if let Some(anchor) = staged.parents.remove(id) {
            preserved.parents.insert(id.clone(), anchor);
        }
    }
    let mut indexes = std::collections::BTreeMap::new();
    for failure in &staged.failures {
        if independent.contains(&failure.id) {
            indexes.insert(failure.loss_index, 0);
        }
    }
    for (index, loss) in std::mem::take(&mut staged.losses).into_iter().enumerate() {
        if let Some(target) = indexes.get_mut(&index) {
            *target = preserved.losses.len();
            super::reserve_output(ctx, true, &mut preserved.losses, 1)?;
            preserved.losses.push(loss);
        }
    }
    let unknowns = staged
        .native
        .namespace_mut(staged.codec.as_str())
        .arenas_mut()
        .remove("unknowns")
        .unwrap_or_default();
    let mut kept = Vec::new();
    for record in unknowns {
        if independent.contains(record.id()) {
            let id = record.id().to_owned();
            staged.ids.remove(&id);
            preserved.ids.insert(id.clone());
            if let Some(owner) = staged.native_owners.remove(&id) {
                preserved.native_owners.insert(id.clone(), owner);
            }
            let unknown = UnknownId::try_from(id).map_err(CodecError::malformed)?;
            if let Some(retained) = staged.fidelity.retained_records.remove(&unknown) {
                preserved
                    .fidelity
                    .retained_records
                    .insert(unknown.clone(), retained);
            }
            if let Some(layout) = staged.fidelity.record_layout.remove(unknown.as_str()) {
                preserved
                    .fidelity
                    .record_layout
                    .insert(unknown.as_str().to_owned(), layout);
            }
            super::reserve_output(ctx, true, &mut kept, 1)?;
            kept.push(record);
        }
    }
    if !kept.is_empty() {
        preserved
            .native
            .namespace_mut(staged.codec.as_str())
            .arenas_mut()
            .insert("unknowns".into(), kept);
    }
    for mut failure in std::mem::take(&mut staged.failures) {
        if independent.contains(&failure.id) {
            failure.loss_index = indexes[&failure.loss_index];
            super::reserve_output(ctx, true, &mut preserved.failures, 1)?;
            preserved.failures.push(failure);
        } else {
            staged.failures.push(failure);
        }
    }
    Ok(preserved)
}

#[cfg(test)]
mod tests;
