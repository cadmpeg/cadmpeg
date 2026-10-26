// SPDX-License-Identifier: Apache-2.0
//! Genuine transient part extents use working rather than retained storage.

use super::{copy_working, SessionHandle};
use crate::decode::{u64_from_index, ByteRange, DecodeContext, PartCapture, View};
use crate::CodecError;

impl SessionHandle<'_> {
    fn working_part_extent(
        self,
        extent: View<'_>,
        activate: bool,
    ) -> Result<PartCapture, CodecError> {
        self.session(|session| {
            if session.sealed {
                return Err(CodecError::malformed(
                    "record captures have already been extracted",
                ));
            }
            if activate {
                // This is the same real session activation as retained captures;
                // only the transient copied names below use working storage.
                session.activate()?;
            }
            if !session.activated {
                return Err(CodecError::malformed(
                    "working part extent needs active source tracking",
                ));
            }
            let stream = session.streams.extent_name(
                extent.space(),
                ByteRange {
                    start: u64_from_index(extent.start()),
                    end: u64_from_index(extent.end()),
                },
            )?;
            let stream = copy_working(&session.budget, stream)?;
            let part_start = u64_from_index(extent.start());
            let part_end = u64_from_index(extent.end());
            let mut containing = None;
            // Prefer an actual enclosing extent in the part's own stream.
            for record in session
                .frames
                .iter()
                .rev()
                .filter_map(|frame| frame.record.as_ref())
            {
                let end = record
                    .offset
                    .checked_add(record.byte_len)
                    .ok_or_else(|| CodecError::malformed("record source extent overflows"))?;
                if record.stream == stream && record.offset <= part_start && end >= part_end {
                    containing = Some(record);
                    break;
                }
            }
            // A transformed child belongs to an ancestor record only if every
            // exact provenance path reaches that record's bounded source extent.
            if containing.is_none() {
                for record in session
                    .frames
                    .iter()
                    .rev()
                    .filter_map(|frame| frame.record.as_ref())
                {
                    let end = record
                        .offset
                        .checked_add(record.byte_len)
                        .ok_or_else(|| CodecError::malformed("record source extent overflows"))?;
                    if session.streams.contains(
                        &session.budget,
                        extent,
                        &record.stream,
                        ByteRange {
                            start: record.offset,
                            end,
                        },
                    )? {
                        containing = Some(record);
                        break;
                    }
                }
            }
            let record_id = containing
                .map(|record| copy_working(&session.budget, record.id()))
                .transpose()?;
            Ok(PartCapture::new(
                stream,
                u64_from_index(extent.start()),
                u64_from_index(extent.end().abs_diff(extent.start())),
                record_id,
            ))
        })
    }
}

impl DecodeContext<'_> {
    /// Validate a framing extent with temporary working names, activating the
    /// same actual source tracking session when it is cold.
    ///
    /// The first activation retains only the real shared tracking infrastructure.
    /// Repeated extent/owner names are working storage. No source bytes, semantic
    /// reads, record captures, stream registration or fresh account are invented.
    /// Unlike `working_part_extent`, this explicitly permits cold activation.
    ///
    /// # Errors
    /// Preserves the retained capture's foreign/detached, sealed, source-bound,
    /// provenance, caller-account allocation and first-refusal checks.
    pub fn framing_part_extent(&self, extent: View<'_>) -> Result<PartCapture, CodecError> {
        self.require_source(extent)?;
        self.budget.working_part_extent(extent, true)
    }

    /// Capture an actual part extent using transient working allocations.
    ///
    /// Uses the same canonical stream/domain and actual enclosing record proof
    /// as `capture_part_extent`. Owned stream and owner names use working
    /// storage; no source bytes, reads, registration or retained charge are added.
    /// Existing active source tracking is required and is never activated here.
    ///
    /// # Errors
    /// Rejects foreign or detached views, inactive or extracted sessions,
    /// invalid source extents, working allocation failure and prior refusal.
    pub fn working_part_extent(&self, extent: View<'_>) -> Result<PartCapture, CodecError> {
        self.require_source(extent)?;
        self.budget.working_part_extent(extent, false)
    }
}

#[cfg(test)]
mod tests;
