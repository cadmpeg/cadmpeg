// SPDX-License-Identifier: Apache-2.0
//! Exact framing and whitespace mappings through genuine stored source aliases.

use super::{reserve, Inventory};
use cadmpeg_core::decode::{DecodeContext, SourceOrigin, SourceStream};
use cadmpeg_core::CodecError;
use std::ops::Range;

impl Inventory {
    pub(super) fn project_alias_ranges(
        &mut self,
        ctx: &DecodeContext<'_>,
        actual: &[SourceStream],
    ) -> Result<(), CodecError> {
        let mut counts = ctx.alloc_filled(actual.len(), (0, 0), "source framing origin ranges")?;
        for (at, source) in actual.iter().enumerate() {
            let stream = self.0.get(source.name()).ok_or_else(|| {
                CodecError::malformed("source framing origin has no actual stream inventory")
            })?;
            counts[at] = (stream.framing.len(), stream.insignificant.len());
        }
        // Only the original ranges initiate a traversal. A range already mapped
        // into an ancestor must not initiate another copy of that same lineage.
        for (source, (framing, insignificant)) in actual.iter().zip(counts) {
            for (count, whitespace) in [(framing, false), (insignificant, true)] {
                for at in 0..count {
                    let stream = &self.0[source.name()];
                    let range = if whitespace {
                        stream.insignificant[at].clone()
                    } else {
                        stream.framing[at].clone()
                    };
                    self.project_parents(ctx, actual, source, range, whitespace)?;
                }
            }
        }
        Ok(())
    }

    fn project_parents(
        &mut self,
        ctx: &DecodeContext<'_>,
        actual: &[SourceStream],
        source: &SourceStream,
        range: Range<u64>,
        whitespace: bool,
    ) -> Result<(), CodecError> {
        if range.start > range.end || range.end > source.byte_len() {
            return Err(CodecError::malformed(
                "source framing origin range escapes its stream",
            ));
        }
        if range.is_empty() {
            return Ok(());
        }
        let _depth = ctx.enter_nested("source framing origin lineage")?;
        match source.origin() {
            SourceOrigin::Root | SourceOrigin::Expanded { .. } => Ok(()),
            SourceOrigin::Alias { parent, extent } => self.project_parent(
                ctx,
                actual,
                parent,
                translate(range, 0, extent.start..extent.end)?,
                whitespace,
            ),
            SourceOrigin::Composite { segments } => {
                for segment in segments {
                    let child = segment.child();
                    let start = range.start.max(child.start);
                    let end = range.end.min(child.end);
                    if start < end {
                        let extent = segment.extent();
                        self.project_parent(
                            ctx,
                            actual,
                            segment.parent(),
                            translate(start..end, child.start, extent.start..extent.end)?,
                            whitespace,
                        )?;
                    }
                }
                Ok(())
            }
        }
    }

    fn project_parent(
        &mut self,
        ctx: &DecodeContext<'_>,
        actual: &[SourceStream],
        name: &str,
        range: Range<u64>,
        whitespace: bool,
    ) -> Result<(), CodecError> {
        let at = actual
            .binary_search_by(|source| source.name().cmp(name))
            .map_err(|_| CodecError::malformed("source framing origin has no registered parent"))?;
        let source = &actual[at];
        if range.start > range.end || range.end > source.byte_len() {
            return Err(CodecError::malformed(
                "source framing origin escapes its parent",
            ));
        }
        let stream = self.0.get_mut(name).ok_or_else(|| {
            CodecError::malformed("source framing parent has no actual stream inventory")
        })?;
        let ranges = if whitespace {
            &mut stream.insignificant
        } else {
            &mut stream.framing
        };
        reserve(ctx, ranges, 1)?;
        ranges.push(range.clone());
        self.project_parents(ctx, actual, source, range, whitespace)
    }
}

fn translate(range: Range<u64>, from: u64, target: Range<u64>) -> Result<Range<u64>, CodecError> {
    let translated = range
        .start
        .checked_sub(from)
        .and_then(|start| target.start.checked_add(start))
        .zip(
            range
                .end
                .checked_sub(from)
                .and_then(|end| target.start.checked_add(end)),
        )
        .filter(|(start, end)| start <= end && *end <= target.end)
        .ok_or_else(|| CodecError::malformed("source framing origin translation overflows"))?;
    Ok(translated.0..translated.1)
}
