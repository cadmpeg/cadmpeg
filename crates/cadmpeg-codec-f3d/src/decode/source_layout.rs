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
    owner_ids: BTreeMap<String, String>,
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
            owner_ids: BTreeMap::new(),
            stream_prefix: String::new(),
        }
    }

    /// Stage the exact identity and stream mappings used by the native merge.
    pub(crate) fn rescope(
        &mut self,
        ctx: &DecodeContext<'_>,
        prefix: &str,
        mut identity: impl FnMut(&str) -> Option<String>,
    ) -> Result<(), CodecError> {
        let mut mapped = BTreeMap::new();
        for id in self.pointers.keys() {
            let current = self.owner_ids.get(id).unwrap_or(id);
            let target = identity(current).unwrap_or_else(|| current.to_owned());
            if mapped.values().any(|other| other == &target) {
                return Err(CodecError::malformed(
                    "F3Z pending layout owner identities collide",
                ));
            }
            let bytes = id
                .len()
                .checked_add(target.len())
                .and_then(|bytes| bytes.checked_add(size_of::<(String, String)>()))
                .ok_or_else(|| CodecError::malformed("F3Z pending layout identities overflow"))?;
            ctx.retain_bytes(u64_from_index(bytes), "F3Z pending layout identities")?;
            mapped.insert(id.to_owned(), target);
        }
        let length = prefix
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
        stream_prefix.push_str(prefix);
        stream_prefix.push_str(&self.stream_prefix);
        self.owner_ids = mapped;
        self.stream_prefix = stream_prefix;
        Ok(())
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

/// Consume only final, genuine captures, then apply the recorded merge mappings.
pub(crate) fn install(
    ctx: &DecodeContext<'_>,
    fidelity: &mut SourceFidelity,
    mut pending: Vec<PendingLayouts>,
) -> Result<(), CodecError> {
    let mut staged = SourceFidelity::default();
    staged.install_mapped_record_captures(ctx, |mut capture| {
        let batch = capture
            .batch_id()
            .ok_or_else(|| CodecError::malformed("F3D capture has no member batch"))?;
        let member = pending
            .iter_mut()
            .find(|member| member.batch == batch)
            .ok_or_else(|| CodecError::malformed("F3D capture has no pending member"))?;
        let pointers = member
            .pointers
            .remove(capture.id())
            .ok_or_else(|| CodecError::malformed("F3D captured owner has no pointer draft"))?;
        let target = member.owner_ids.remove(capture.id());
        for (field, offset) in pointers {
            capture.fields_mut().insert_pointer(field, offset)?;
        }
        capture.with_mapping(ctx, target, &member.stream_prefix)
    })?;
    if pending
        .iter()
        .any(|member| !member.pointers.is_empty() || !member.owner_ids.is_empty())
    {
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
