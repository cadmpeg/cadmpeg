// SPDX-License-Identifier: Apache-2.0
//! Independent Appendix C STEP framing; no codec parser or semantic reads.

use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View, u64_from_index};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

use super::{FrameParent, FramedPart};

#[derive(Debug)]
struct Token {
    bytes: Range<usize>,
    text: String,
}
impl Token {
    fn is(&self, text: &str) -> bool {
        self.text == text
    }
}

fn space(byte: u8) -> bool {
    byte == b' ' || byte.is_ascii_control()
}

fn comment_end(bytes: &[u8], at: usize, draft: bool) -> Option<usize> {
    let pair = bytes.get(at..at.checked_add(2)?)?;
    let close: &[u8] = if pair == b"/*" {
        b"*/"
    } else if draft && pair == b"!*" {
        b"*!"
    } else {
        return None;
    };
    let start = at.checked_add(2)?;
    bytes
        .get(start..)?
        .windows(2)
        .position(|pair| pair == close)
        .and_then(|length| start.checked_add(length)?.checked_add(2))
}

fn trivia(bytes: &[u8], mut at: usize, draft: bool) -> usize {
    loop {
        if bytes.get(at).is_some_and(|byte| space(*byte)) {
            at += 1;
        } else if bytes
            .get(at..)
            .is_some_and(|part| part.starts_with(b"\\N\\") || part.starts_with(b"\\F\\"))
        {
            at += 3;
        } else if let Some(end) = comment_end(bytes, at, draft) {
            at = end;
        } else {
            return at;
        }
    }
}

fn tokens(bytes: &[u8], draft: bool) -> Vec<Token> {
    let mut result = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        at = trivia(bytes, at, draft);
        let Some(&first) = bytes.get(at) else { break };
        // An unfinished comment consumes the remainder; its contents are not tokens.
        if bytes.get(at..).is_some_and(|part| part.starts_with(b"/*"))
            || (draft && bytes.get(at..).is_some_and(|part| part.starts_with(b"!*")))
        {
            break;
        }
        let start = at;
        let mut text = String::new();
        if matches!(first, b'\'' | b'"' | b'<') {
            let close = if first == b'<' { b'>' } else { first };
            at += 1;
            while let Some(&byte) = bytes.get(at) {
                at += 1;
                if byte == close {
                    if first == b'\'' && bytes.get(at) == Some(&b'\'') {
                        at += 1;
                    } else {
                        break;
                    }
                }
            }
            text.push(char::from(first));
        } else if first.is_ascii_alphanumeric() || first == b'_' {
            while let Some(&byte) = bytes.get(at) {
                if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                    text.push(char::from(byte));
                    at += 1;
                } else if byte.is_ascii_control() {
                    at += 1;
                } else {
                    break;
                }
            }
        } else {
            text.push(char::from(first));
            at += 1;
        }
        result.push(Token {
            bytes: start..at,
            text,
        });
    }
    result
}

fn marker(tokens: &[Token], at: usize, name: &str) -> Option<usize> {
    (tokens.get(at)?.is(name) && tokens.get(at.checked_add(1)?)?.is(";"))
        .then_some(at.checked_add(1)?)
}

fn opening(bytes: &[u8]) -> Option<(bool, usize, usize)> {
    let standard = tokens(bytes, false);
    if marker(&standard, 0, "ISO-10303-21").is_some() {
        return Some((false, 0, standard[1].bytes.end));
    }
    let draft = tokens(bytes, true);
    if marker(&draft, 0, "STEP").is_some() {
        return Some((true, 0, draft[1].bytes.end));
    }
    // A draft preamble is exactly an identifier and one complete comment.
    let &first = bytes.first()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let mut end = 1;
    while bytes
        .get(end)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
    {
        end += 1;
    }
    while bytes.get(end).is_some_and(|byte| space(*byte)) {
        end += 1;
    }
    let end = comment_end(bytes, end, true)?;
    let mut magic = end;
    while bytes.get(magic).is_some_and(|byte| space(*byte)) {
        magic += 1;
    }
    let candidate = draft.iter().position(|token| token.bytes.start == magic)?;
    let semi = marker(&draft, candidate, "STEP")?;
    Some((true, magic, draft[semi].bytes.end))
}

fn absolute(view: View<'_>, range: Range<usize>) -> Result<Range<u64>, String> {
    if range.start > range.end || range.end > view.window().len() {
        return Err("STEP range lies outside source view".into());
    }
    let start = view
        .start()
        .checked_add(range.start)
        .ok_or("STEP offset overflow")?;
    let end = view
        .start()
        .checked_add(range.end)
        .ok_or("STEP offset overflow")?;
    Ok(u64_from_index(start)..u64_from_index(end))
}

fn push<'a>(
    ctx: &DecodeContext<'_>,
    view: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    level: &str,
    parent: FrameParent,
    extent: Range<usize>,
    framing: Vec<Range<usize>>,
    dispatch: Vec<Range<usize>>,
) -> Result<usize, String> {
    let bounds = absolute(view, extent)?;
    let start = usize::try_from(bounds.start).map_err(|_| "STEP start exceeds usize")?;
    let end = usize::try_from(bounds.end).map_err(|_| "STEP end exceeds usize")?;
    let child = view
        .child(start, end)
        .ok_or("STEP child extent is not contained")?;
    let framing = framing
        .into_iter()
        .map(|range| absolute(view, range))
        .collect::<Result<Vec<_>, _>>()?;
    let dispatch = dispatch
        .into_iter()
        .map(|range| absolute(view, range))
        .collect::<Result<Vec<_>, _>>()?;
    let level =
        PartLevel::parse(level).ok_or_else(|| format!("missing Appendix C level {level}"))?;
    let frame = FramedPart::new(ctx, level, child, parent, framing, dispatch)?;
    let index = out.len();
    out.push(frame);
    Ok(index)
}

/// Frame an actual standalone STEP stream, appending parents before children.
///
/// No opening envelope means no parts. Once an opening is present, cut-short
/// exchanges, sections and final records retain their actual available extent.
/// Unknown entity names and invalid content do not prevent framing.
///
/// # Errors
/// Returns invalid source provenance or a range admission failure.
pub fn frame<'a>(
    ctx: &DecodeContext<'_>,
    view: View<'a>,
    parent: FrameParent,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let bytes = view.window();
    let Some((draft, start, open_end)) = opening(bytes) else {
        return Ok(());
    };
    if start > 0 {
        push(
            ctx,
            view,
            out,
            "step.draft_preamble",
            parent,
            0..start,
            vec![],
            vec![],
        )?;
    }
    let all = tokens(bytes, draft);
    let open = all
        .iter()
        .position(|token| token.bytes.end == open_end)
        .ok_or("STEP opening terminator disappeared")?;
    let close_name = if draft { "ENDSTEP" } else { "END-ISO-10303-21" };
    let close = (open + 1..all.len()).find(|&index| marker(&all, index, close_name).is_some());
    let body_end = close.map_or(bytes.len(), |index| all[index].bytes.start);
    let exchange_end = close.map_or(bytes.len(), |index| {
        trivia(bytes, all[index + 1].bytes.end, draft)
    });
    let mut framing = vec![all[open - 1].bytes.start..open_end];
    if let Some(close) = close {
        framing.push(all[close].bytes.start..all[close + 1].bytes.end);
    }
    let exchange = push(
        ctx,
        view,
        out,
        "step.exchange",
        parent,
        start..exchange_end,
        framing,
        vec![all[open - 1].bytes.clone()],
    )?;
    let limit = close.unwrap_or(all.len());
    let mut at = open + 1;
    while at < limit {
        let kind = match all[at].text.as_str() {
            "HEADER" => Some(("step.header_section", "step.header_record")),
            "ANCHOR" => Some(("step.anchor_section", "step.anchor_entry")),
            "REFERENCE" => Some(("step.reference_section", "step.reference_entry")),
            "DATA" => Some(("step.data_section", "step.simple_entity_instance")),
            _ => None,
        };
        let Some((level, record_level)) = kind else {
            at += 1;
            continue;
        };
        let Some(open_semi) = (at + 1..limit).find(|&i| all[i].is(";")) else {
            break;
        };
        let end = (open_semi + 1..limit).find(|&i| marker(&all, i, "ENDSEC").is_some());
        let records_end = end.unwrap_or(limit);
        let record_extent_end = end.map_or(body_end, |i| all[i].bytes.start);
        let section_end = end.map_or(body_end, |i| all[i + 1].bytes.end);
        let mut framing = if level == "step.data_section" {
            vec![all[at].bytes.clone(), all[open_semi].bytes.clone()]
        } else {
            vec![all[at].bytes.start..all[open_semi].bytes.end]
        };
        if let Some(end) = end {
            framing.push(all[end].bytes.start..all[end + 1].bytes.end);
        }
        let dispatch = if level == "step.data_section" && open_semi > at + 1 {
            vec![all[at + 1].bytes.start..all[open_semi - 1].bytes.end]
        } else {
            vec![]
        };
        let section = push(
            ctx,
            view,
            out,
            level,
            FrameParent::Part(exchange),
            all[at].bytes.start..section_end,
            framing,
            dispatch,
        )?;
        let mut record_at = open_semi + 1;
        while record_at < records_end {
            let semi = (record_at..records_end).find(|&i| all[i].is(";"));
            let stop = semi.map_or(records_end, |i| i + 1);
            let extent_end = semi.map_or(record_extent_end, |i| all[i].bytes.end);
            record(
                ctx,
                view,
                out,
                section,
                record_level,
                &all[record_at..stop],
                extent_end,
            )?;
            record_at = stop;
        }
        at = end.map_or(limit, |i| i + 2);
    }
    if close.is_some() {
        signatures(ctx, view, out, parent, exchange_end, draft)?;
    }
    Ok(())
}

fn record<'a>(
    ctx: &DecodeContext<'_>,
    view: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    parent: usize,
    kind: &str,
    tokens: &[Token],
    end: usize,
) -> Result<(), String> {
    let Some(first) = tokens.first() else {
        return Ok(());
    };
    let mut framing = Vec::new();
    if let Some(semi) = tokens.last().filter(|token| token.is(";")) {
        framing.push(semi.bytes.clone());
    }
    let mut dispatch = Vec::new();
    let mut level = kind;
    if kind == "step.header_record" {
        dispatch.push(first.bytes.clone());
        if first.is("FILE_SCHEMA") {
            dispatch.extend(
                tokens
                    .iter()
                    .skip(1)
                    .filter(|token| token.is("'"))
                    .map(|token| token.bytes.clone()),
            );
        } else if first.is("FILE_DESCRIPTION") {
            let mut depth = 0_usize;
            let mut argument = 0;
            let mut selected = None;
            for token in tokens.iter().skip(1) {
                if token.is("(") {
                    depth += 1;
                } else if token.is(")") {
                    if let Some(next) = depth.checked_sub(1) {
                        depth = next;
                    }
                } else if token.is(",") && depth == 1 {
                    argument += 1;
                    continue;
                }
                if depth == 1 && argument == 1 && !token.is(")") && !token.is(";") {
                    selected = Some(selected.map_or(token.bytes.clone(), |range: Range<usize>| {
                        range.start..token.bytes.end
                    }));
                }
            }
            dispatch.extend(selected);
        }
    } else if let Some(eq) = tokens.iter().position(|token| token.is("=")) {
        framing.push(first.bytes.start..tokens[eq].bytes.end);
        if kind == "step.reference_entry" {
            dispatch.push(first.bytes.start..first.bytes.start + 1);
        }
        if kind == "step.simple_entity_instance" {
            if tokens.get(eq + 1).is_some_and(|token| token.is("(")) {
                level = "step.complex_entity_instance";
                framing.push(tokens[eq + 1].bytes.clone());
                let mut depth = 0_usize;
                for (index, token) in tokens.iter().enumerate().skip(eq + 1) {
                    if token.is("(") {
                        depth += 1;
                    } else if token.is(")") {
                        if let Some(next) = depth.checked_sub(1) {
                            depth = next;
                        }
                        if depth == 0 {
                            framing.push(token.bytes.clone());
                        }
                    } else if depth == 1 && tokens.get(index + 1).is_some_and(|next| next.is("(")) {
                        let start = if index > 0 && tokens[index - 1].is("!") {
                            tokens[index - 1].bytes.start
                        } else {
                            token.bytes.start
                        };
                        dispatch.push(start..token.bytes.end);
                    }
                }
            } else if let Some(name) = tokens.get(eq + 1) {
                let end = if name.is("!") {
                    tokens
                        .get(eq + 2)
                        .map_or(name.bytes.end, |word| word.bytes.end)
                } else {
                    name.bytes.end
                };
                dispatch.push(name.bytes.start..end);
            }
        }
    } else if kind == "step.simple_entity_instance" || kind == "step.reference_entry" {
        framing.push(first.bytes.clone());
    }
    framing.sort_by_key(|range| range.start);
    push(
        ctx,
        view,
        out,
        level,
        FrameParent::Part(parent),
        first.bytes.start..end,
        framing,
        dispatch,
    )?;
    Ok(())
}

fn signature_end(bytes: &[u8], mut at: usize, draft: bool) -> Option<(Range<usize>, usize)> {
    let mut boundary = true;
    while at < bytes.len() {
        let next = trivia(bytes, at, draft);
        if next > at {
            at = next;
            boundary = true;
            continue;
        }
        if boundary {
            let mut cursor = at;
            let mut matches = true;
            for expected in b"ENDSEC" {
                while bytes
                    .get(cursor)
                    .is_some_and(|byte| byte.is_ascii_control())
                {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(expected) {
                    matches = false;
                    break;
                }
                cursor += 1;
            }
            if matches {
                cursor = trivia(bytes, cursor, draft);
                if bytes.get(cursor) == Some(&b';') {
                    let end = cursor.checked_add(1)?;
                    return Some((at..end, trivia(bytes, end, draft)));
                }
            }
        }
        at += 1;
        boundary = false;
    }
    None
}

fn signatures<'a>(
    ctx: &DecodeContext<'_>,
    view: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    parent: FrameParent,
    mut at: usize,
    draft: bool,
) -> Result<(), String> {
    let bytes = view.window();
    while at < bytes.len() {
        let tail = tokens(&bytes[at..], draft);
        if marker(&tail, 0, "SIGNATURE").is_none() {
            break;
        }
        let start = at + tail[0].bytes.start;
        let opening_end = at + tail[1].bytes.end;
        let close = signature_end(bytes, opening_end, draft);
        let end = close.as_ref().map_or(bytes.len(), |(_, end)| *end);
        let mut framing = vec![start..opening_end];
        if let Some((close, _)) = close {
            framing.push(close);
        }
        push(
            ctx,
            view,
            out,
            "step.signature_section",
            parent,
            start..end,
            framing,
            vec![],
        )?;
        at = end;
    }
    Ok(())
}

/// Frame the actual content of the ZIP entry named `ISO-10303.p21`.
///
/// The ZIP framer supplies both the real payload view and the original entry
/// name view. No copy, decompression, or address-space registration happens here.
///
/// # Errors
/// Returns a wrong entry name or invalid source provenance.
pub fn frame_zip_root<'a>(
    ctx: &DecodeContext<'_>,
    view: View<'a>,
    entry_name: View<'a>,
    zip_parent_index: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    if entry_name.window() != b"ISO-10303.p21" {
        return Err("STEP ZIP root entry name is not ISO-10303.p21".into());
    }
    let level = PartLevel::parse("step.zip_root_member").ok_or("missing STEP ZIP-root level")?;
    let wrapper = FramedPart::new(
        ctx,
        level,
        view,
        FrameParent::Part(zip_parent_index),
        vec![],
        vec![],
    )?
    .with_external_dispatch(ctx, vec![entry_name])?;
    let index = out.len();
    out.push(wrapper);
    frame(ctx, view, FrameParent::Host(index), out)
}

/// One edited source interval, in the original view's absolute coordinates.
pub struct Edit<'a> {
    /// The interval to replace, in the source view's actual address space.
    pub range: Range<u64>,
    /// Replacement content. It may have a different length.
    pub replacement: &'a [u8],
}

/// Rebuild STEP after content edits, retaining its actual independent framing.
///
/// STEP has no content length or checksum to recompute. Nonoverlapping edits
/// are spliced into the original stream. Crossing an original framing marker
/// is refused rather than inventing an envelope or repairing semantic content.
/// The deliberately damaged dispatch and content bytes are preserved exactly.
///
/// # Errors
/// Returns invalid source provenance, an absent STEP envelope, overlapping edits,
/// an out-of-view edit, or an edit that removes independent framing.
pub fn rebuild(
    ctx: &DecodeContext<'_>,
    view: View<'_>,
    edits: &[Edit<'_>],
) -> Result<Vec<u8>, String> {
    let mut frames = Vec::new();
    frame(ctx, view, FrameParent::File, &mut frames)?;
    if !frames
        .iter()
        .any(|part| part.level().as_str() == "step.exchange")
    {
        return Err("cannot rebuild STEP without an actual exchange envelope".into());
    }
    let mut edits = edits.iter().collect::<Vec<_>>();
    edits.sort_by_key(|edit| edit.range.start);
    let mut cursor = u64_from_index(view.start());
    let end = u64_from_index(view.end());
    let mut result = Vec::new();
    for edit in &edits {
        if edit.range.start < cursor || edit.range.end < edit.range.start || edit.range.end > end {
            return Err("STEP edit overlaps another edit or lies outside the source".into());
        }
        for part in &frames {
            if part.framing().iter().any(|range| {
                edit.range.start < range.end && range.start < edit.range.end
                    || edit.range.is_empty()
                        && range.start < edit.range.start
                        && edit.range.start < range.end
            }) {
                return Err(format!(
                    "STEP edit crosses {} framing",
                    part.level().as_str()
                ));
            }
        }
        let relative = |value: u64| {
            usize::try_from(value - u64_from_index(view.start()))
                .map_err(|_| "STEP source index exceeds usize".to_owned())
        };
        let unchanged = view
            .window()
            .get(relative(cursor)?..relative(edit.range.start)?)
            .ok_or("STEP unchanged source interval is absent")?;
        result.extend_from_slice(unchanged);
        result.extend_from_slice(edit.replacement);
        cursor = edit.range.end;
    }
    let remaining = usize::try_from(cursor - u64_from_index(view.start()))
        .map_err(|_| "STEP remaining source index exceeds usize")?;
    result.extend_from_slice(
        view.window()
            .get(remaining..)
            .ok_or("STEP remaining source is absent")?,
    );
    let Some((draft, _, _)) = opening(&result) else {
        return Err("STEP edit makes the envelope unenclosable".into());
    };
    let visible = tokens(&result, draft);
    for part in &frames {
        for range in part.framing() {
            let start = rebuilt_position(range.start, false, view, &edits)?;
            let end = rebuilt_position(range.end, true, view, &edits)?;
            if !visible.iter().any(|token| token.bytes.start == start)
                || !visible.iter().any(|token| token.bytes.end == end)
            {
                return Err(format!(
                    "STEP edit hides {} framing in content",
                    part.level().as_str()
                ));
            }
        }
    }
    Ok(result)
}

fn rebuilt_position(
    position: u64,
    end: bool,
    view: View<'_>,
    edits: &[&Edit<'_>],
) -> Result<usize, String> {
    let mut result = i128::from(position - u64_from_index(view.start()));
    for edit in edits {
        if edit.range.end < position
            || edit.range.end == position && (!end || !edit.range.is_empty())
        {
            result += i128::try_from(edit.replacement.len())
                .map_err(|_| "STEP replacement length exceeds source coordinate width")?
                - i128::from(edit.range.end - edit.range.start);
        }
    }
    usize::try_from(result).map_err(|_| "STEP rebuilt offset exceeds usize".into())
}

#[cfg(test)]
mod tests;
