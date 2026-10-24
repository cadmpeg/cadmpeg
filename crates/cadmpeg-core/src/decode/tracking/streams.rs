// SPDX-License-Identifier: Apache-2.0
//! Explicit named source streams and exact parent mappings.

use std::borrow::Cow;
use std::ops::Range;

use super::{copy_string, reserve};
use crate::decode::budget::DecodeBudget;
use crate::decode::{u64_from_index, ByteRange, SpaceId, View};
use crate::CodecError;

/// An immutable description of one explicitly registered source stream.
///
/// This snapshot contains provenance, not read coverage or B1 classification.
#[derive(Debug, PartialEq, Eq)]
pub struct SourceStream {
    name: String,
    byte_len: u64,
    origin: SourceOrigin,
}

impl SourceStream {
    /// Canonical stable name; identical reopens produce one inventory entry.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Exact stream domain length; the domain starts at zero.
    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }
    /// Explicit physical, stored, expanded or composite provenance.
    pub fn origin(&self) -> &SourceOrigin {
        &self.origin
    }
}

/// Provenance of a stream, expressed only in canonical stream names.
#[derive(Debug, PartialEq, Eq)]
pub enum SourceOrigin {
    /// The physical input, occupying `[0, byte_len)`.
    Root,
    /// A stored one-to-one alias; it is not another physical input.
    Alias {
        /// Canonical parent stream name.
        parent: String,
        /// Exact extent in the parent, with the same length as this stream.
        extent: ByteRange,
    },
    /// Decoded bytes from one exact compressed parent extent.
    Expanded {
        /// Canonical parent stream name.
        parent: String,
        /// Entire compressed extent justified by a semantic child read.
        extent: ByteRange,
    },
    /// Concatenated source extents in contiguous child-coordinate order.
    Composite {
        /// Exact segment mappings, including empty inputs when present.
        segments: Vec<SourceSegment>,
    },
}

/// One composite child interval and its exact parent mapping.
#[derive(Debug, PartialEq, Eq)]
pub struct SourceSegment {
    child: ByteRange,
    parent: String,
    extent: ByteRange,
}

impl SourceSegment {
    /// Interval in the composite stream.
    pub fn child(&self) -> ByteRange {
        self.child
    }
    /// Canonical name of the source stream for this interval.
    pub fn parent(&self) -> &str {
        &self.parent
    }
    /// Equal-length interval in that parent stream.
    pub fn extent(&self) -> ByteRange {
        self.extent
    }
}

#[derive(Debug, Default)]
pub(super) struct Streams {
    root: Option<Stream>,
    derived: Vec<Stream>,
}

#[derive(Debug)]
struct Stream {
    space: SpaceId,
    name: Cow<'static, str>,
    byte_len: u64,
    depth: u64,
    origin: Option<Origin>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Origin {
    Alias { parent: SpaceId, extent: ByteRange },
    Compressed { parent: SpaceId, extent: ByteRange },
    Composite(Vec<Segment>),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Segment {
    start: u64,
    parent: SpaceId,
    extent: ByteRange,
}

impl Streams {
    pub(super) fn has_root(&self) -> bool {
        self.root.is_some()
    }

    pub(super) fn root(&mut self, byte_len: u64) {
        self.root = Some(Stream {
            space: SpaceId::ROOT,
            name: Cow::Borrowed("file"),
            byte_len,
            depth: 0,
            origin: None,
        });
    }

    fn get(&self, space: SpaceId) -> Result<&Stream, CodecError> {
        self.root
            .iter()
            .chain(&self.derived)
            .find(|stream| stream.space == space)
            .ok_or_else(|| CodecError::malformed("source stream has no registered provenance"))
    }

    pub(super) fn require_end(&self, view: View<'_>) -> Result<(), CodecError> {
        let stream = self.get(view.space())?;
        validate_extent(
            stream,
            ByteRange {
                start: u64_from_index(view.start()),
                end: u64_from_index(view.end()),
            },
        )?;
        if u64_from_index(view.end()) != stream.byte_len {
            return Err(CodecError::malformed(
                "source window does not reach its registered stream end",
            ));
        }
        Ok(())
    }

    pub(super) fn name(&self, space: SpaceId) -> Result<&str, CodecError> {
        Ok(&self.get(space)?.name)
    }

    pub(super) fn extent_name(
        &self,
        space: SpaceId,
        extent: ByteRange,
    ) -> Result<&str, CodecError> {
        let stream = self.get(space)?;
        validate_extent(stream, extent)?;
        Ok(&stream.name)
    }

    pub(super) fn rename_root(
        &mut self,
        budget: &DecodeBudget,
        name: &str,
    ) -> Result<(), CodecError> {
        if name.is_empty() {
            return Err(CodecError::malformed("source stream name is empty"));
        }
        if self.derived.iter().any(|stream| stream.name == name) {
            return Err(CodecError::malformed(
                "source stream name already belongs to another stream",
            ));
        }
        let root = self
            .root
            .as_mut()
            .ok_or_else(|| CodecError::malformed("no root source stream"))?;
        if root.name != name {
            root.name = Cow::Owned(copy_string(budget, name)?);
        }
        Ok(())
    }

    pub(super) fn register(
        &mut self,
        budget: &DecodeBudget,
        space: SpaceId,
        name: &str,
        byte_len: u64,
        origin: Origin,
    ) -> Result<SpaceId, CodecError> {
        if name.is_empty() {
            return Err(CodecError::malformed("source stream name is empty"));
        }
        let parent_depth = match &origin {
            Origin::Alias { parent, extent } | Origin::Compressed { parent, extent } => {
                let parent = self.get(*parent)?;
                validate_extent(parent, *extent)?;
                match &origin {
                    Origin::Alias { .. } if extent.end.abs_diff(extent.start) != byte_len => {
                        return Err(CodecError::malformed(
                            "stored stream length differs from its source extent",
                        ));
                    }
                    Origin::Compressed { .. } if extent.start == extent.end && byte_len != 0 => {
                        return Err(CodecError::malformed(
                            "nonempty expansion has an empty compressed source extent",
                        ));
                    }
                    _ => {}
                }
                parent.depth
            }
            Origin::Composite(parts) => {
                let mut end = 0;
                let mut depth = 0;
                for part in parts {
                    let parent = self.get(part.parent)?;
                    validate_extent(parent, part.extent)?;
                    if part.start != end {
                        return Err(CodecError::malformed(
                            "composite source segments are not contiguous",
                        ));
                    }
                    end = end
                        .checked_add(part.extent.end.abs_diff(part.extent.start))
                        .ok_or_else(|| {
                            CodecError::malformed("composite source length overflows")
                        })?;
                    depth = depth.max(parent.depth);
                }
                if parts.is_empty() || end != byte_len {
                    return Err(CodecError::malformed(
                        "composite source segments do not cover the stream",
                    ));
                }
                depth
            }
        };
        if let Some(existing) = self
            .root
            .iter()
            .chain(&self.derived)
            .find(|stream| stream.name == name)
        {
            if existing.byte_len == byte_len && existing.origin.as_ref() == Some(&origin) {
                return Ok(existing.space);
            }
            return Err(CodecError::malformed(
                "source stream name has conflicting provenance",
            ));
        }
        if self
            .root
            .iter()
            .chain(&self.derived)
            .any(|stream| stream.space == space)
        {
            return Err(CodecError::malformed(
                "source address space already has provenance",
            ));
        }
        let depth = parent_depth
            .checked_add(1)
            .ok_or_else(|| budget.retained_allocation_failed(u64::MAX, "source stream depth"))?;
        // A source lineage is real decode nesting, bounded by the same caller
        // policy as parsing. No format-specific or implementation minimum exists.
        let _depth = budget.enter_levels(depth, "source stream lineage")?;
        let name = copy_string(budget, name)?;
        reserve(budget, &mut self.derived, 1)?;
        self.derived.push(Stream {
            space,
            name: Cow::Owned(name),
            byte_len,
            depth,
            origin: Some(origin),
        });
        Ok(space)
    }

    pub(super) fn register_composite(
        &mut self,
        budget: &DecodeBudget,
        space: SpaceId,
        name: &str,
        inputs: &[View<'_>],
        byte_len: u64,
    ) -> Result<SpaceId, CodecError> {
        let mut parts = Vec::new();
        reserve(budget, &mut parts, inputs.len())?;
        let mut start = 0u64;
        for input in inputs {
            let extent = ByteRange {
                start: u64_from_index(input.start()),
                end: u64_from_index(input.end()),
            };
            parts.push(Segment {
                start,
                parent: input.space(),
                extent,
            });
            start = start
                .checked_add(extent.end.abs_diff(extent.start))
                .ok_or_else(|| CodecError::malformed("composite source length overflows"))?;
        }
        self.register(budget, space, name, byte_len, Origin::Composite(parts))
    }

    pub(super) fn contains(
        &self,
        budget: &DecodeBudget,
        part: View<'_>,
        owner_stream: &str,
        owner: ByteRange,
    ) -> Result<bool, CodecError> {
        #[derive(Clone, Copy)]
        struct Extent {
            space: SpaceId,
            range: ByteRange,
        }

        let initial = Extent {
            space: part.space(),
            range: ByteRange {
                start: u64_from_index(part.start()),
                end: u64_from_index(part.end()),
            },
        };
        let mut pending = Vec::new();
        reserve(budget, &mut pending, 1)?;
        pending.push(initial);
        let mut seen: Vec<Extent> = Vec::new();
        while let Some(current) = pending.pop() {
            let stream = self.get(current.space)?;
            validate_extent(stream, current.range)?;
            if stream.name == owner_stream {
                if current.range.start < owner.start || current.range.end > owner.end {
                    return Ok(false);
                }
                continue;
            }
            // Empty extents have no parent-byte ownership proof. Direct
            // same-stream containment above still handles empty parts exactly.
            if current.range.start == current.range.end {
                return Ok(false);
            }
            if seen.iter().any(|old| {
                old.space == current.space
                    && old.range.start <= current.range.start
                    && old.range.end >= current.range.end
            }) {
                continue;
            }
            reserve(budget, &mut seen, 1)?;
            seen.push(current);
            let mut push = |space, range| -> Result<(), CodecError> {
                reserve(budget, &mut pending, 1)?;
                pending.push(Extent { space, range });
                Ok(())
            };
            match &stream.origin {
                None => return Ok(false),
                Some(Origin::Alias { parent, extent }) => {
                    let range = translate(current.range.start..current.range.end, 0, extent.start)?;
                    push(
                        *parent,
                        ByteRange {
                            start: range.start,
                            end: range.end,
                        },
                    )?;
                }
                Some(Origin::Compressed { parent, extent }) => push(*parent, *extent)?,
                Some(Origin::Composite(parts)) => {
                    for part in parts {
                        let end = part
                            .start
                            .checked_add(part.extent.end.abs_diff(part.extent.start))
                            .ok_or_else(|| {
                                CodecError::malformed("composite segment end overflows")
                            })?;
                        let start = current.range.start.max(part.start);
                        let end = current.range.end.min(end);
                        if start < end {
                            let range = translate(start..end, part.start, part.extent.start)?;
                            push(
                                part.parent,
                                ByteRange {
                                    start: range.start,
                                    end: range.end,
                                },
                            )?;
                        }
                    }
                }
            }
        }
        Ok(true)
    }

    pub(super) fn snapshot(&self, budget: &DecodeBudget) -> Result<Vec<SourceStream>, CodecError> {
        let mut streams = Vec::new();
        reserve(
            budget,
            &mut streams,
            self.derived
                .len()
                .checked_add(usize::from(self.root.is_some()))
                .ok_or_else(|| budget.retained_allocation_failed(u64::MAX, "source inventory"))?,
        )?;
        for stream in self.root.iter().chain(&self.derived) {
            let origin = match &stream.origin {
                None => SourceOrigin::Root,
                Some(Origin::Alias { parent, extent }) => SourceOrigin::Alias {
                    parent: copy_string(budget, self.name(*parent)?)?,
                    extent: *extent,
                },
                Some(Origin::Compressed { parent, extent }) => SourceOrigin::Expanded {
                    parent: copy_string(budget, self.name(*parent)?)?,
                    extent: *extent,
                },
                Some(Origin::Composite(parts)) => {
                    let mut segments = Vec::new();
                    reserve(budget, &mut segments, parts.len())?;
                    for part in parts {
                        let end = part
                            .start
                            .checked_add(part.extent.end.abs_diff(part.extent.start))
                            .ok_or_else(|| {
                                CodecError::malformed("composite segment end overflows")
                            })?;
                        segments.push(SourceSegment {
                            child: ByteRange {
                                start: part.start,
                                end,
                            },
                            parent: copy_string(budget, self.name(part.parent)?)?,
                            extent: part.extent,
                        });
                    }
                    SourceOrigin::Composite { segments }
                }
            };
            streams.push(SourceStream {
                name: copy_string(budget, &stream.name)?,
                byte_len: stream.byte_len,
                origin,
            });
        }
        streams.sort_unstable_by(|a, b| a.name.cmp(&b.name));
        Ok(streams)
    }

    pub(super) fn project(
        &self,
        budget: &DecodeBudget,
        space: SpaceId,
        range: Range<u64>,
        mut visit: impl FnMut(SpaceId, Range<u64>) -> Result<bool, CodecError>,
    ) -> Result<(), CodecError> {
        // Validate the maximum path against the same caller budget before any
        // recursive traversal. Existing parents make this graph acyclic.
        let _depth = budget.enter_levels(self.get(space)?.depth, "source stream lineage")?;
        self.project_inner(space, range, &mut visit)
    }

    fn project_inner(
        &self,
        mut space: SpaceId,
        mut range: Range<u64>,
        visit: &mut impl FnMut(SpaceId, Range<u64>) -> Result<bool, CodecError>,
    ) -> Result<(), CodecError> {
        loop {
            let stream = self.get(space)?;
            validate_extent(
                stream,
                ByteRange {
                    start: range.start,
                    end: range.end,
                },
            )?;
            // A fully covered range already projected its ancestors in this
            // transaction. This also bounds traversal of shared DAG ancestors.
            if range.is_empty() || !visit(space, range.clone())? {
                return Ok(());
            }
            match &stream.origin {
                None => return Ok(()),
                Some(Origin::Alias { parent, extent }) => {
                    range = translate(range, 0, extent.start)?;
                    space = *parent;
                }
                Some(Origin::Compressed { parent, extent }) => {
                    range = extent.start..extent.end;
                    space = *parent;
                }
                Some(Origin::Composite(parts)) => {
                    for part in parts {
                        let end = part
                            .start
                            .checked_add(part.extent.end.abs_diff(part.extent.start))
                            .ok_or_else(|| {
                                CodecError::malformed("composite segment end overflows")
                            })?;
                        let start = range.start.max(part.start);
                        let end = range.end.min(end);
                        if start < end {
                            self.project_inner(
                                part.parent,
                                translate(start..end, part.start, part.extent.start)?,
                                visit,
                            )?;
                        }
                    }
                    return Ok(());
                }
            }
        }
    }
}

fn translate(range: Range<u64>, from: u64, to: u64) -> Result<Range<u64>, CodecError> {
    range
        .start
        .checked_sub(from)
        .and_then(|start| to.checked_add(start))
        .zip(
            range
                .end
                .checked_sub(from)
                .and_then(|end| to.checked_add(end)),
        )
        .map(|(start, end)| start..end)
        .ok_or_else(|| CodecError::malformed("source stream coordinates overflow"))
}

fn validate_extent(stream: &Stream, extent: ByteRange) -> Result<(), CodecError> {
    if extent.start > extent.end || extent.end > stream.byte_len {
        return Err(CodecError::malformed(
            "source extent escapes its registered stream",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
