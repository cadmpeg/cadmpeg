// SPDX-License-Identifier: Apache-2.0
use std::ops::Range;

use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, ResourceDimension, SourceOrigin, SourceStream, View,
};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

use super::Edit;
use crate::framing::iges::{append, frame, frame_global, frame_parameters};
use crate::framing::{FrameParent, FramedPart, PartTable};

/// An equal-length replacement of an actual Global/Parameter field subview.
pub struct FieldEdit<'a> {
    /// A genuine registered source subview contained by one reached field.
    pub source: View<'a>,
    /// Replacement bytes; source framing and grammar dispatch are protected.
    pub replacement: &'a [u8],
}

#[derive(Debug, PartialEq, Eq)]
struct Signature {
    level: PartLevel,
    parent: FrameParent,
    extent: Vec<Range<u64>>,
    framing: Vec<Range<u64>>,
    dispatch: Vec<Range<u64>>,
}

/// Rebuild equal-length Global/ordinary Parameter field content edits.
///
/// Each edit must identify a genuine field view in this session. Registered
/// one-to-one source segments map its bytes back to the physical cards. Counts,
/// delimiters, dispatch, DE/P locators, envelopes and unrelated bytes remain
/// unchanged. Reframing the output in an independent source session must yield
/// the same physical structural signature. Expanded streams cannot be inverted.
///
/// # Errors
/// Returns invalid/foreign source, unreached content, framing/dispatch edits,
/// overlapping or unequal-length edits, a changed grammar, or a caller-budget
/// refusal. Macro statement content and fragmented sections remain unsupported.
pub fn rebuild_fields<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    edits: &[FieldEdit<'_>],
) -> Result<Vec<u8>, String> {
    let source_capture = ctx
        .capture_part_extent(source)
        .map_err(|error| error.to_string())?;
    let parts = frames(ctx, source)?;
    let streams = ctx.source_streams().map_err(|error| error.to_string())?;
    let root = source_capture.stream();
    if !streams
        .iter()
        .any(|stream| stream.name() == root && matches!(stream.origin(), SourceOrigin::Root))
    {
        return Err("IGES field rebuilding needs a physical root view".into());
    }
    let mut protected = Vec::new();
    for part in &parts {
        for range in part.framing().iter().chain(part.dispatch()) {
            project(
                ctx,
                &streams,
                part.capture().stream(),
                range.clone(),
                root,
                0,
                &mut protected,
            )?;
        }
    }
    protected.sort_unstable_by_key(|range| (range.start, range.end));
    protected.dedup_by(|next, previous| {
        if next.start <= previous.end {
            previous.end = previous.end.max(next.end);
            true
        } else {
            false
        }
    });
    let mut physical = Vec::new();
    for edit in edits {
        let capture = ctx
            .capture_part_extent(edit.source)
            .map_err(|error| error.to_string())?;
        let end = capture
            .offset()
            .checked_add(capture.byte_len())
            .ok_or("IGES field edit overflows")?;
        if capture.byte_len() != u64_from_index(edit.replacement.len()) {
            return Err("IGES field edit changes length".into());
        }
        if !parts.iter().any(|part| {
            matches!(
                part.level().as_str(),
                "iges.global_field" | "iges.parameter_field"
            ) && part.capture().stream() == capture.stream()
                && part.capture().offset() <= capture.offset()
                && end <= part.capture().offset() + part.capture().byte_len()
                && !part
                    .framing()
                    .iter()
                    .any(|range| capture.offset() < range.end && range.start < end)
        }) {
            return Err("IGES edit is not wholly inside reached field content".into());
        }
        if edit
            .replacement
            .iter()
            .any(|byte| matches!(byte, b'\r' | b'\n'))
        {
            return Err("IGES field edit introduces a physical line ending".into());
        }
        let mut pieces = Vec::new();
        project(
            ctx,
            &streams,
            capture.stream(),
            capture.offset()..end,
            root,
            0,
            &mut pieces,
        )?;
        let mut at = 0_usize;
        for piece in pieces {
            if piece.start < source_capture.offset()
                || piece.end > source_capture.offset() + source_capture.byte_len()
                || protected
                    .iter()
                    .any(|range| piece.start < range.end && range.start < piece.end)
            {
                return Err("IGES field edit intersects source framing or grammar dispatch".into());
            }
            let length = usize::try_from(piece.end - piece.start)
                .map_err(|_| "IGES field edit exceeds usize")?;
            let next = at
                .checked_add(length)
                .filter(|next| *next <= edit.replacement.len())
                .ok_or("IGES field projection changes byte length")?;
            append(
                ctx,
                &mut physical,
                Edit {
                    range: piece,
                    replacement: &edit.replacement[at..next],
                },
            )?;
            at = next;
        }
        if at != edit.replacement.len() {
            return Err("IGES field projection omits source bytes".into());
        }
    }
    physical.sort_unstable_by_key(|edit| (edit.range.start, edit.range.end));
    for pair in physical.windows(2) {
        if pair[0].range.end > pair[1].range.start {
            return Err("IGES field edits overlap in the physical source".into());
        }
    }
    let expected = signatures(ctx, &streams, &parts, root, u64_from_index(source.start()))?;
    let result = super::apply_edits(ctx, source, &physical)?;
    validate_candidate(ctx, &result, &expected)?;
    Ok(result)
}

fn frames<'a>(ctx: &DecodeContext<'a>, source: View<'a>) -> Result<Vec<FramedPart<'a>>, String> {
    let mut parts = Vec::new();
    frame(ctx, source, &mut parts)?;
    frame_global(ctx, source, &mut parts)?;
    frame_parameters(ctx, source, &mut parts)?;
    PartTable::load()?.validate(ctx, &parts)?;
    Ok(parts)
}

fn project(
    ctx: &DecodeContext<'_>,
    streams: &[SourceStream],
    stream: &str,
    range: Range<u64>,
    root: &str,
    depth: usize,
    out: &mut Vec<Range<u64>>,
) -> Result<(), String> {
    if range.is_empty() {
        return Ok(());
    }
    if depth > streams.len() {
        return Err("IGES source projection contains a cycle".into());
    }
    let source = streams
        .iter()
        .find(|candidate| candidate.name() == stream)
        .ok_or("IGES source projection names an unknown stream")?;
    if range.end > source.byte_len() {
        return Err("IGES source projection exceeds stream bounds".into());
    }
    if stream == root {
        return append(ctx, out, range);
    }
    match source.origin() {
        SourceOrigin::Alias { parent, extent } => {
            let start = extent
                .start
                .checked_add(range.start)
                .ok_or("IGES alias offset overflows")?;
            let end = extent
                .start
                .checked_add(range.end)
                .ok_or("IGES alias offset overflows")?;
            project(ctx, streams, parent, start..end, root, depth + 1, out)
        }
        SourceOrigin::Composite { segments } => {
            let mut cursor = range.start;
            for segment in segments {
                let start = range.start.max(segment.child().start);
                let end = range.end.min(segment.child().end);
                if start < end {
                    if start != cursor {
                        return Err("IGES composite projection has a gap or overlap".into());
                    }
                    let parent_start = segment
                        .extent()
                        .start
                        .checked_add(start - segment.child().start)
                        .ok_or("IGES composite offset overflows")?;
                    let parent_end = parent_start
                        .checked_add(end - start)
                        .ok_or("IGES composite end overflows")?;
                    project(
                        ctx,
                        streams,
                        segment.parent(),
                        parent_start..parent_end,
                        root,
                        depth + 1,
                        out,
                    )?;
                    cursor = end;
                }
            }
            if cursor != range.end {
                return Err("IGES composite projection is incomplete".into());
            }
            Ok(())
        }
        SourceOrigin::Expanded { .. } => {
            Err("IGES field rebuilding cannot invert an expanded source".into())
        }
        SourceOrigin::Root => Err("IGES field source has a different physical root".into()),
    }
}

fn signatures(
    ctx: &DecodeContext<'_>,
    streams: &[SourceStream],
    parts: &[FramedPart<'_>],
    root: &str,
    base: u64,
) -> Result<Vec<Signature>, String> {
    let mut result = Vec::new();
    for part in parts {
        let mut extent = Vec::new();
        let mut framing = Vec::new();
        let mut dispatch = Vec::new();
        project(
            ctx,
            streams,
            part.capture().stream(),
            part.capture().offset()..part.capture().offset() + part.capture().byte_len(),
            root,
            0,
            &mut extent,
        )?;
        for (input, output) in [
            (part.framing(), &mut framing),
            (part.dispatch(), &mut dispatch),
        ] {
            for range in input {
                project(
                    ctx,
                    streams,
                    part.capture().stream(),
                    range.clone(),
                    root,
                    0,
                    output,
                )?;
            }
        }
        for range in extent.iter_mut().chain(&mut framing).chain(&mut dispatch) {
            range.start = range
                .start
                .checked_sub(base)
                .ok_or("IGES signature precedes source")?;
            range.end = range
                .end
                .checked_sub(base)
                .ok_or("IGES signature precedes source")?;
        }
        append(
            ctx,
            &mut result,
            Signature {
                level: part.level(),
                parent: part.parent(),
                extent,
                framing,
                dispatch,
            },
        )?;
    }
    Ok(result)
}

fn validate_candidate(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    expected: &[Signature],
) -> Result<(), String> {
    let allowance = ctx.retained_bytes_available();
    let mut policy = *ctx.policy();
    policy.limits.max_retained_bytes = allowance;
    let arena = DecodeArena::new();
    let (candidate, root) = DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .map_err(|error| error.to_string())?;
    let initial = candidate.retained_bytes_available();
    let checked = (|| {
        let parts = frames(&candidate, root)?;
        let streams = candidate
            .source_streams()
            .map_err(|error| error.to_string())?;
        let capture = candidate
            .capture_part_extent(root)
            .map_err(|error| error.to_string())?;
        let actual = signatures(&candidate, &streams, &parts, capture.stream(), 0)?;
        if actual != expected {
            return Err("IGES field edit changes structural framing or dispatch".into());
        }
        Ok(())
    })();
    let used = match candidate.resource_refusal() {
        Some(limit) if limit.dimension == ResourceDimension::RetainedBytes => limit.used,
        _ => initial
            .checked_sub(candidate.retained_bytes_available())
            .ok_or("IGES candidate retained allowance increased")?,
    };
    ctx.retain_bytes(used, "IGES candidate framing validation")
        .map_err(|error| error.to_string())?;
    checked
}

#[cfg(test)]
mod tests;
