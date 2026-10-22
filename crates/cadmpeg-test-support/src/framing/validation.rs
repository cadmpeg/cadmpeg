// SPDX-License-Identifier: Apache-2.0
//! Parent, source-provenance and nearest-owner coverage checks.

use std::collections::BTreeMap;
use std::ops::Range;

use cadmpeg_core::decode::{index_from_u64, DecodeContext, SourceOrigin, SourceStream, View};
use cadmpeg_ir::source_fidelity::parts::PartFraming;

use super::types::{same_extent, subtract_ranges};
use super::{FrameParent, FramedPart, PartParent, PartTable};

impl PartTable<'_> {
    /// Validate each real source view and its exact declared containment.
    pub fn validate(
        &self,
        ctx: &DecodeContext<'_>,
        frames: &[FramedPart<'_>],
    ) -> Result<(), String> {
        let streams = ctx.source_streams().map_err(|e| e.to_string())?;
        let mut siblings = BTreeMap::new();
        for (index, frame) in frames.iter().enumerate() {
            let actual = ctx
                .capture_part_extent(frame.view())
                .map_err(|e| e.to_string())?;
            if actual.stream() != frame.capture().stream()
                || actual.offset() != frame.capture().offset()
                || actual.byte_len() != frame.capture().byte_len()
            {
                return Err("frame source capture changed".into());
            }
            for view in frame.external_dispatch() {
                ctx.capture_part_extent(*view).map_err(|e| e.to_string())?;
            }
            let rule = self.rule(frame.level())?;
            let parent = match frame.parent() {
                FrameParent::File => {
                    let stream = find_stream(&streams, actual.stream())?;
                    if !rule.parents().contains(&PartParent::File)
                        || !matches!(stream.origin(), SourceOrigin::Root)
                    {
                        return Err(format!(
                            "{} is not a physical-file child",
                            frame.level().as_str()
                        ));
                    }
                    validate_siblings(&mut siblings, frame)?;
                    continue;
                }
                FrameParent::Part(parent) | FrameParent::Host(parent) => parent,
            };
            if parent >= index {
                return Err("frame parent must precede its child".into());
            }
            let enclosing = &frames[parent];
            if let Some(proof) = frame.xml_containment() {
                if !matches!(frame.parent(), FrameParent::Part(_))
                    || !same_extent(&proof.parent, enclosing.capture())
                {
                    return Err(
                        "physical frame parent is not the immediate source XML parent".into(),
                    );
                }
                if proof.owner >= index
                    || !rule.parents().contains(&PartParent::Level(
                        frames
                            .get(proof.owner)
                            .ok_or("missing XML declared owner")?
                            .level(),
                    ))
                {
                    return Err("XML declared owner has an undeclared level".into());
                }
                let mut ancestor = parent;
                loop {
                    if ancestor == proof.owner {
                        break;
                    }
                    let next = match frames[ancestor].parent() {
                        FrameParent::Part(next) | FrameParent::Host(next) if next < ancestor => {
                            next
                        }
                        _ => return Err("XML declared owner is not a physical ancestor".into()),
                    };
                    ancestor = next;
                }
            } else if let Some(proof) = frame.zip_containment() {
                if frame.level().as_str() != "zip.digital_signature"
                    || enclosing.level().as_str() != "zip.central_directory"
                    || !matches!(frame.parent(), FrameParent::Part(_))
                    || !same_extent(&proof.parent, enclosing.capture())
                    || proof.owner >= index
                {
                    return Err("ZIP signature has no matching physical directory parent".into());
                }
                let owner = frames
                    .get(proof.owner)
                    .ok_or("missing ZIP declared owner")?;
                if owner.level().as_str() != "zip.archive"
                    || !rule.parents().contains(&PartParent::Level(owner.level()))
                    || enclosing.parent() != FrameParent::Part(proof.owner)
                {
                    return Err(
                        "ZIP signature declared owner is not its actual archive ancestor".into(),
                    );
                }
            } else {
                match frame.parent() {
                    FrameParent::Part(_) => {
                        if !rule
                            .parents()
                            .contains(&PartParent::Level(enclosing.level()))
                        {
                            return Err(format!(
                                "undeclared parent for {}",
                                frame.level().as_str()
                            ));
                        }
                    }
                    FrameParent::Host(_) => {
                        if !rule.parents().contains(&PartParent::File)
                            || !mentions_level(
                                self.rule(enclosing.level())?.end_rule(),
                                frame.level().as_str(),
                            )
                        {
                            return Err("shared root has no declared embedding rule".into());
                        }
                    }
                    FrameParent::File => unreachable!(),
                }
            }
            let mapped = project_extent(
                actual.stream(),
                actual.offset()..actual.offset() + actual.byte_len(),
                enclosing.capture().stream(),
                &streams,
                0,
            )?;
            if mapped.is_empty()
                || mapped.iter().any(|range| {
                    range.start < enclosing.capture().offset()
                        || range.end > enclosing.capture().offset() + enclosing.capture().byte_len()
                })
            {
                return Err("child extent is not inside its proven source parent".into());
            }
            validate_siblings(&mut siblings, frame)?;
        }
        Ok(())
    }

    /// Content owned by this part rather than a finer reached child.
    pub fn owned_ranges(
        &self,
        ctx: &DecodeContext<'_>,
        frames: &[FramedPart<'_>],
        index: usize,
    ) -> Result<Vec<Range<u64>>, String> {
        self.validate(ctx, frames)?;
        let frame = frames.get(index).ok_or("missing owner frame")?;
        let streams = ctx.source_streams().map_err(|e| e.to_string())?;
        let mut excluded = frame.framing().to_vec();
        for child in frames {
            if matches!(child.parent(), FrameParent::Part(parent) | FrameParent::Host(parent) if parent == index)
            {
                excluded.extend(project_extent(
                    child.capture().stream(),
                    child.capture().offset()..child.capture().offset() + child.capture().byte_len(),
                    frame.capture().stream(),
                    &streams,
                    0,
                )?);
            }
        }
        Ok(subtract_ranges(
            frame.capture().offset()..frame.capture().offset() + frame.capture().byte_len(),
            &excluded,
        ))
    }

    /// Sequential failure reaches the containing part's end, including extent-only rows.
    pub fn failure_extent<'a>(
        &self,
        ctx: &DecodeContext<'_>,
        frames: &[FramedPart<'a>],
        index: usize,
    ) -> Result<View<'a>, String> {
        self.validate(ctx, frames)?;
        let frame = frames.get(index).ok_or("missing failed frame")?;
        if frame.level().framing() != PartFraming::Sequential {
            return Ok(frame.view());
        }
        let parent = match frame.parent() {
            FrameParent::Part(parent) | FrameParent::Host(parent) => &frames[parent],
            FrameParent::File => {
                return Err("sequential file child requires an enclosing file extent".into());
            }
        };
        let streams = ctx.source_streams().map_err(|e| e.to_string())?;
        let mapped = project_extent(
            frame.capture().stream(),
            frame.capture().offset()..frame.capture().offset() + frame.capture().byte_len(),
            parent.capture().stream(),
            &streams,
            0,
        )?;
        let start = mapped
            .iter()
            .map(|range| range.start)
            .min()
            .ok_or("unmapped sequential part")?;
        let start = index_from_u64(start).ok_or("part offset exceeds address capacity")?;
        parent
            .view()
            .child(start, parent.view().end())
            .ok_or_else(|| "sequential tail is outside its parent".into())
    }
}

fn mentions_level(text: &str, level: &str) -> bool {
    text.match_indices(level).any(|(offset, _)| {
        let prefix = &text[..offset];
        let suffix = &text[offset + level.len()..];
        !prefix.ends_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
            && !suffix.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
            && !suffix.strip_prefix('.').is_some_and(|tail| {
                tail.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
            })
    })
}

type Siblings<'a> = BTreeMap<(Option<usize>, &'a str), BTreeMap<u64, u64>>;

fn validate_siblings<'a>(
    siblings: &mut Siblings<'a>,
    frame: &'a FramedPart<'_>,
) -> Result<(), String> {
    let parent = match frame.parent() {
        FrameParent::File => None,
        FrameParent::Part(index) | FrameParent::Host(index) => Some(index),
    };
    let intervals = siblings
        .entry((parent, frame.capture().stream()))
        .or_default();
    let start = frame.capture().offset();
    let end = start + frame.capture().byte_len();
    if start == end {
        return Ok(());
    }
    if intervals
        .range(..=start)
        .next_back()
        .is_some_and(|(_, previous_end)| *previous_end > start)
        || intervals
            .range(start..)
            .next()
            .is_some_and(|(next_start, _)| *next_start < end)
        || intervals.contains_key(&start)
    {
        return Err("sibling frame extents overlap or duplicate".into());
    }
    intervals.insert(start, end);
    Ok(())
}

fn find_stream<'a>(streams: &'a [SourceStream], name: &str) -> Result<&'a SourceStream, String> {
    streams
        .iter()
        .find(|stream| stream.name() == name)
        .ok_or_else(|| format!("unregistered stream {name}"))
}

fn project_extent(
    source: &str,
    extent: Range<u64>,
    target: &str,
    streams: &[SourceStream],
    depth: usize,
) -> Result<Vec<Range<u64>>, String> {
    if source == target {
        return Ok(vec![extent]);
    }
    if depth >= streams.len() {
        return Err("source provenance contains a cycle".into());
    }
    let stream = find_stream(streams, source)?;
    match stream.origin() {
        SourceOrigin::Root => Err("source parent is not an ancestor of the child stream".into()),
        SourceOrigin::Alias {
            parent,
            extent: alias,
        } => {
            let start = alias
                .start
                .checked_add(extent.start)
                .ok_or("alias offset overflows")?;
            let end = alias
                .start
                .checked_add(extent.end)
                .ok_or("alias offset overflows")?;
            project_extent(parent, start..end, target, streams, depth + 1)
        }
        SourceOrigin::Expanded { parent, extent } => {
            project_extent(parent, extent.start..extent.end, target, streams, depth + 1)
        }
        SourceOrigin::Composite { segments } => {
            let mut mapped = Vec::new();
            for segment in segments {
                let start = extent.start.max(segment.child().start);
                let end = extent.end.min(segment.child().end);
                if start < end {
                    let start = segment.extent().start + (start - segment.child().start);
                    let end = segment.extent().start + (end - segment.child().start);
                    mapped.extend(project_extent(
                        segment.parent(),
                        start..end,
                        target,
                        streams,
                        depth + 1,
                    )?);
                }
            }
            if mapped.is_empty() {
                return Err("composite source extent has no provenance segment".into());
            }
            Ok(mapped)
        }
    }
}
