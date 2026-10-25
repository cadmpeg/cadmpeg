// SPDX-License-Identifier: Apache-2.0
//! Counted string records, whose text lines cannot be used as resync markers.

use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};

use super::{emit, entry, Tokens};
use crate::framing::FramedPart;

/// Source grammar selected by the actual XML carrier or side member.
#[derive(Clone, Copy, Debug)]
pub enum StringRecords {
    /// Root StringHasher character content.
    Document {
        /// Record count selected by the root StringHasher XML attribute.
        count: usize,
    },
    /// Shape-owned StringHasher character content.
    Shape {
        /// Record count selected by the shape-owned XML value attribute.
        count: usize,
    },
    /// StringTableStart header and its following records.
    SideEntry,
}

/// Frame a named side member, preserving its real source carrier as dispatch.
pub fn frame_string_table_entry<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    file_attribute: View<'a>,
    zip_parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let parent = entry(
        ctx,
        source,
        file_attribute,
        zip_parent,
        "freecad.string_table_entry",
        out,
    )?;
    frame_string_records(ctx, source, StringRecords::SideEntry, parent, out)
}

/// Append the string boundaries proved by flags and decimal text-line counts.
/// Unsupported IDs and references do not affect these lexical boundaries.
pub fn frame_string_records<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    layout: StringRecords,
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let mut tokens = Tokens::new(source);
    let (count, name) = match layout {
        StringRecords::Document { count } => (count, "freecad.string_hasher_record"),
        StringRecords::Shape { count } => (count, "freecad.shape_string_hasher_record"),
        StringRecords::SideEntry => {
            tokens.skip();
            let start = tokens.position;
            let _magic = tokens.token()?;
            let version = tokens.token()?;
            let count = tokens.token()?;
            let separator = tokens.skip();
            if separator.is_empty() {
                return Err("string table header has no closing whitespace".into());
            }
            emit(
                ctx,
                &tokens,
                start..tokens.position,
                "freecad.string_table_header",
                parent,
                vec![count.clone(), tokens.position - 1..tokens.position],
                vec![version.clone()],
                out,
            )?;
            if tokens.text(version)? != "v1" {
                return Err("string table version does not prove a record layout".into());
            }
            (tokens.count(count)?, "freecad.string_table_record")
        }
    };
    if count > source.window().len() {
        return Err("string record count exceeds source extent".into());
    }
    for _ in 0..count {
        tokens.skip();
        let start = tokens.position;
        let (framing, dispatch, complete) = record(&mut tokens)?;
        emit(
            ctx,
            &tokens,
            start..tokens.position,
            name,
            parent,
            framing,
            dispatch,
            out,
        )?;
        if !complete {
            return Err("string record is cut short at its containing source end".into());
        }
    }
    tokens.skip();
    if tokens.position != tokens.bytes().len() {
        return Err("unassigned string table tail".into());
    }
    Ok(())
}

fn record(tokens: &mut Tokens<'_>) -> Result<(Vec<Range<usize>>, Vec<Range<usize>>, bool), String> {
    let header = tokens.token()?;
    let dots = tokens.bytes()[header.clone()]
        .iter()
        .enumerate()
        .filter_map(|(offset, byte)| (*byte == b'.').then_some(header.start + offset))
        .take(2)
        .collect::<Vec<_>>();
    let first = *dots.first().ok_or("string header has no flag delimiter")?;
    let flag_end = dots.get(1).copied().unwrap_or(header.end);
    let flags = u64::from_str_radix(tokens.text(first + 1..flag_end)?, 16)
        .map_err(|error| error.to_string())?;
    let mut framing = vec![first..dots.get(1).map_or(flag_end, |dot| dot + 1)];
    let mut dispatch = Vec::new();
    if tokens.bytes()[header.start] == b'-' {
        dispatch.push(header.start..header.start + 1);
    }
    dispatch.push(first + 1..flag_end);
    if flags & 8 == 0 {
        framing.push(tokens.skip());
        let count_start = tokens.position;
        while tokens
            .bytes()
            .get(tokens.position)
            .is_some_and(u8::is_ascii_digit)
        {
            tokens.position += 1;
        }
        let count_end = tokens.position;
        if count_start == count_end || tokens.bytes().get(count_end) != Some(&b':') {
            return Err("string payload has no decimal line count and colon".into());
        }
        let count = tokens.count(count_start..count_end)?;
        tokens.position += 1;
        framing.push(count_start..tokens.position);
        for _ in 0..=count {
            let Some(newline) = tokens.bytes()[tokens.position..]
                .iter()
                .position(|byte| *byte == b'\n')
            else {
                // #215 cut-short/end-extent interpretation: the intact header
                // locates one EOF tail, never a header-like line inside it.
                tokens.position = tokens.bytes().len();
                framing.retain(|range| !range.is_empty());
                return Ok((framing, dispatch, false));
            };
            tokens.position += newline + 1;
        }
        framing.push(tokens.position - 1..tokens.position);
    } else {
        let payload_count = usize::from(flags & 0x70 == 0) + usize::from(flags & 4 == 0);
        for _ in 0..payload_count {
            framing.push(tokens.skip());
            tokens.token()?;
        }
        let separator = tokens.skip();
        if separator.is_empty() {
            return Err("postfixed string has no closing whitespace".into());
        }
        framing.push(separator);
    }
    framing.retain(|range| !range.is_empty());
    Ok((framing, dispatch, true))
}
