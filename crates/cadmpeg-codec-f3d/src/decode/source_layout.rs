// SPDX-License-Identifier: Apache-2.0
//! Deferred member-local layout installation after outer capture commitment.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, RecordBatchId, ResourceDimension, ResourceLimit,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::SourceFidelity;

/// Pointer drafts associated with one genuine member capture batch.
pub(crate) struct PendingLayouts {
    batch: RecordBatchId,
    pointers: BTreeMap<String, BTreeMap<String, u64>>,
    occurrences: Vec<String>,
    stream_prefix: String,
}

impl PendingLayouts {
    pub(crate) fn new(
        batch: RecordBatchId,
        pointers: BTreeMap<String, BTreeMap<String, u64>>,
    ) -> Self {
        Self {
            batch,
            pointers,
            occurrences: Vec::new(),
            stream_prefix: String::new(),
        }
    }

    /// Stage the identity and stream mappings the native merge applies to
    /// every record of this batch under one F3Z occurrence.
    pub(crate) fn rescope(
        &mut self,
        ctx: &DecodeContext<'_>,
        occurrence: &str,
    ) -> Result<(), CodecError> {
        let prefix = crate::f3z::merge::occurrence_stream_prefix(occurrence)?;
        let bytes = occurrence
            .len()
            .checked_add(size_of::<String>())
            .ok_or_else(|| CodecError::malformed("F3Z pending occurrence overflows"))?;
        ctx.retain_bytes(u64_from_index(bytes), "F3Z pending layout occurrence")?;
        self.occurrences
            .try_reserve_exact(1)
            .map_err(|_| allocation(ctx, u64_from_index(bytes), "F3Z pending layout occurrence"))?;
        let length = prefix
            .as_str()
            .len()
            .checked_add(self.stream_prefix.len())
            .ok_or_else(|| CodecError::malformed("F3Z pending stream prefix overflows"))?;
        ctx.retain_bytes(u64_from_index(length), "F3Z pending layout stream prefix")?;
        let mut stream_prefix = String::new();
        stream_prefix.try_reserve_exact(length).map_err(|_| {
            allocation(
                ctx,
                u64_from_index(length),
                "F3Z pending layout stream prefix",
            )
        })?;
        stream_prefix.push_str(prefix.as_str());
        stream_prefix.push_str(&self.stream_prefix);
        self.occurrences.push(occurrence.to_owned());
        self.stream_prefix = stream_prefix;
        Ok(())
    }

    /// The record id the native merge gives a member-local record id.
    fn target(&self, id: &str) -> String {
        self.occurrences
            .iter()
            .fold(id.to_owned(), |current, occurrence| {
                crate::f3z::merge::rescope_record_id(&current, occurrence).unwrap_or(current)
            })
    }
}

pub(crate) fn append(
    ctx: &DecodeContext<'_>,
    destination: &mut Vec<PendingLayouts>,
    members: impl IntoIterator<Item = PendingLayouts>,
) -> Result<(), CodecError> {
    for member in members {
        ctx.retain_bytes(
            u64_from_index(size_of::<PendingLayouts>()),
            "F3D pending layout batches",
        )?;
        destination.try_reserve_exact(1).map_err(|_| {
            allocation(
                ctx,
                u64_from_index(size_of::<PendingLayouts>()),
                "F3D pending layout batches",
            )
        })?;
        destination.push(member);
    }
    Ok(())
}

/// Install every final capture, then apply the recorded merge mappings.
///
/// A capture with a pointer draft gains its pointers. A pointer draft with no
/// committed capture is an internal invariant failure.
pub(crate) fn install(
    ctx: &DecodeContext<'_>,
    fidelity: &mut SourceFidelity,
    mut pending: Vec<PendingLayouts>,
) -> Result<(), CodecError> {
    let mut staged = SourceFidelity::default();
    staged.install_mapped_record_captures(ctx, |mut capture| {
        let Some(member) = capture
            .batch_id()
            .and_then(|batch| pending.iter_mut().find(|member| member.batch == batch))
        else {
            return Ok(capture);
        };
        for (field, offset) in member.pointers.remove(capture.id()).into_iter().flatten() {
            capture.fields_mut().insert_pointer(field, offset)?;
        }
        let target = member.target(capture.id());
        capture.with_mapping(ctx, Some(target), &member.stream_prefix)
    })?;
    if pending.iter().any(|member| !member.pointers.is_empty()) {
        return Err(CodecError::malformed(
            "F3D pointer draft has no committed owner capture",
        ));
    }
    fidelity.append(staged)
}

fn allocation(ctx: &DecodeContext<'_>, bytes: u64, operation: &'static str) -> CodecError {
    ResourceLimit::allocation_failed(
        ResourceDimension::RetainedBytes,
        ctx.policy().limits.max_retained_bytes,
        bytes,
        operation,
    )
    .into()
}

#[cfg(test)]
mod tests;
