// SPDX-License-Identifier: Apache-2.0
//! Completed capture installation with occurrence-local identity mappings.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, RecordBatchId};
use cadmpeg_core::CodecError;

use crate::source_fidelity::{RecordLayout, SourceFidelity};

/// A completed core capture with its actual member batch and layout.
///
/// Only the installer constructs this value. A mapper can add layout fields
/// and apply the composition owner's actual identity and stream mappings.
///
/// ```compile_fail
/// use cadmpeg_ir::source_fidelity::parts::CapturedRecordLayout;
/// let capture = CapturedRecordLayout { id: String::new() };
/// ```
#[derive(Debug)]
pub struct CapturedRecordLayout {
    id: String,
    batch: Option<RecordBatchId>,
    layout: RecordLayout,
}

impl CapturedRecordLayout {
    /// Captured identity, or the target after its actual composition mapping.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Actual core member batch, absent only for an unbatched capture.
    pub fn batch_id(&self) -> Option<RecordBatchId> {
        self.batch
    }

    /// Add actual pointers and payloads without constructing read attribution.
    pub fn fields_mut(&mut self) -> &mut RecordLayout {
        &mut self.layout
    }

    /// Apply the owner's admitted target identity and exact stream prefix.
    ///
    /// The owned target identity is moved, not copied. The added prefix
    /// bytes of each stream name use the caller's retained-byte budget. Captured
    /// coordinates, read intervals, pointers and payloads remain unchanged.
    pub fn with_mapping(
        mut self,
        ctx: &DecodeContext<'_>,
        target: Option<String>,
        prefix: &str,
    ) -> Result<Self, CodecError> {
        if let Some(target) = target {
            self.id = target;
        }
        if !prefix.is_empty() {
            self.layout = self.layout.with_stream_prefix_in(ctx, prefix)?;
        }
        Ok(self)
    }
}

impl SourceFidelity {
    /// Install captures after removing only the actual finalized dependency drops.
    /// A foreign proof refuses before this session's captures are extracted.
    pub fn install_pruned_record_captures(
        &mut self,
        ctx: &DecodeContext<'_>,
        pruning: super::PrunedPartCaptures,
        mut fields: impl FnMut(&str, &mut RecordLayout) -> Result<(), CodecError>,
    ) -> Result<(), CodecError> {
        self.install_mapped_pruned_record_captures(ctx, pruning, |mut capture| {
            if capture.batch_id().is_some() {
                return Err(CodecError::malformed(
                    "a batched record capture needs install_mapped_pruned_record_captures",
                ));
            }
            fields(&capture.id, &mut capture.layout)?;
            Ok(capture)
        })
    }

    /// Install surviving captures through the actual member mapping after pruning.
    pub fn install_mapped_pruned_record_captures(
        &mut self,
        ctx: &DecodeContext<'_>,
        pruning: super::PrunedPartCaptures,
        map: impl FnMut(CapturedRecordLayout) -> Result<CapturedRecordLayout, CodecError>,
    ) -> Result<(), CodecError> {
        if ctx.session_token()? != pruning.session {
            return Err(CodecError::malformed(
                "capture pruning belongs to another decode session",
            ));
        }
        self.install_filtered_record_captures(ctx, Some(pruning), map)
    }

    /// Install completed captures through their actual batch-aware mapper.
    ///
    /// Drains the context once. Each callback owns one genuine capture; only
    /// this helper constructs its read layout and installs the mapped result.
    /// Duplicate target identities are errors. Pending ancestor transactions
    /// cannot release captures. The install is atomic: a mapper failure or a
    /// duplicate identity installs no capture.
    pub fn install_mapped_record_captures(
        &mut self,
        ctx: &DecodeContext<'_>,
        map: impl FnMut(CapturedRecordLayout) -> Result<CapturedRecordLayout, CodecError>,
    ) -> Result<(), CodecError> {
        self.install_filtered_record_captures(ctx, None, map)
    }

    fn install_filtered_record_captures(
        &mut self,
        ctx: &DecodeContext<'_>,
        pruning: Option<super::PrunedPartCaptures>,
        mut map: impl FnMut(CapturedRecordLayout) -> Result<CapturedRecordLayout, CodecError>,
    ) -> Result<(), CodecError> {
        let mut staged = SourceFidelity::default();
        for capture in ctx.take_record_captures()? {
            let batch = capture.batch_id();
            if pruning.as_ref().is_some_and(|proof| {
                proof
                    .dropped
                    .binary_search_by(|(id, owner)| {
                        id.as_str()
                            .cmp(capture.id())
                            .then_with(|| owner.cmp(&batch))
                    })
                    .is_ok()
            }) {
                continue;
            }
            let (id, layout) = RecordLayout::from_capture(capture);
            let mapped = map(CapturedRecordLayout { id, batch, layout })?;
            ctx.retain_bytes(
                u64_from_index(size_of::<(String, RecordLayout)>()),
                "captured record layout index",
            )?;
            staged.insert_record_layout(mapped.id, mapped.layout)?;
        }
        self.append(staged)
    }
}

#[cfg(test)]
mod tests;
