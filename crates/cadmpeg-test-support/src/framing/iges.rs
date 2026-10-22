// SPDX-License-Identifier: Apache-2.0
//! Fixed-ASCII IGES cards and Directory fields from Appendix C.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_ir::source_fidelity::parts::PartLevel;

use super::{FrameParent, FramedPart};
use cards::{Card, Kind};

mod cards;
mod global;
mod rebuild;

pub use global::frame_global;
pub use rebuild::{rebuild, Edit};

/// Append actual physical ASCII cards, sections and Directory fields.
///
/// This slice covers nine Appendix C levels. Use [`frame_global`] for Global
/// fields. Parameter fields, compressed ASCII and binary IGES are not
/// implemented here. A cut-short
/// card needs its actual column-73 marker; an earlier cut is an unmarked line.
/// Overlong marked lines that do not meet the packed-card rule have no frame.
/// This scan copies no source bytes and attributes no semantic reads.
///
/// # Errors
/// Returns unsupported representation, invalid provenance,
/// arithmetic overflow or a caller-budget/allocation refusal.
pub fn frame<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    ctx.capture_part_extent(source)
        .map_err(|error| error.to_string())?;
    let cards = cards::scan(ctx, source.window())?;
    let mut at = 0;
    while at < cards.len() {
        let card = cards[at];
        match card.kind {
            Kind::Card(kind @ (b'S' | b'D' | b'P')) => {
                let mut end = at + 1;
                while end < cards.len() && cards[end].kind == card.kind {
                    end += 1;
                }
                if kind != b'S' || card.start == 0 {
                    section(ctx, source, &cards[at..end], kind, out)?;
                }
                at = end;
            }
            Kind::Card(b'T') => {
                push(
                    ctx,
                    source,
                    out,
                    "iges.terminate_card",
                    FrameParent::File,
                    card.start..card.end,
                    (
                        vec![card.start..card.start + 32, card.start + 72..card.end],
                        vec![card.start + 72..card.start + 73],
                    ),
                )?;
                at += 1;
            }
            Kind::Unmarked | Kind::PostTerminate => {
                let level = if card.kind == Kind::Unmarked {
                    "iges.unmarked_line"
                } else {
                    "iges.post_terminate_line"
                };
                let framing = if card.body_end < card.end {
                    vec![card.body_end..card.end]
                } else {
                    Vec::new()
                };
                push(
                    ctx,
                    source,
                    out,
                    level,
                    FrameParent::File,
                    card.start..card.end,
                    (framing, Vec::new()),
                )?;
                at += 1;
            }
            _ => at += 1,
        }
    }
    Ok(())
}

fn section<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    cards: &[Card],
    kind: u8,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let first = cards.first().ok_or("IGES section has no cards")?;
    let last = cards.last().ok_or("IGES section has no final card")?;
    let mut framing = Vec::new();
    let mut dispatch = Vec::new();
    for card in cards {
        append(ctx, &mut framing, card.start + 72..card.end)?;
        append(ctx, &mut dispatch, card.start + 72..card.start + 73)?;
    }
    let level = match kind {
        b'S' => "iges.start_section",
        b'D' => "iges.directory_section",
        b'P' => "iges.parameter_section",
        _ => return Err("invalid physical IGES section marker".into()),
    };
    let parent = push(
        ctx,
        source,
        out,
        level,
        FrameParent::File,
        first.start..last.end,
        (framing, dispatch),
    )?;
    if kind == b'S' {
        for card in cards {
            push(
                ctx,
                source,
                out,
                "iges.start_card",
                FrameParent::Part(parent),
                card.start..card.end,
                (vec![card.start + 72..card.end], Vec::new()),
            )?;
        }
    } else if kind == b'D' {
        for pair in cards.chunks(2) {
            directory(ctx, source, pair, parent, out)?;
        }
    }
    Ok(())
}

fn directory<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    cards: &[Card],
    section: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let first = cards
        .first()
        .ok_or("IGES Directory Entry has no first card")?;
    let last = cards
        .last()
        .ok_or("IGES Directory Entry has no final card")?;
    let mut framing = Vec::new();
    let mut dispatch = Vec::new();
    for (index, card) in cards.iter().enumerate() {
        let locator = if index == 0 { 8 } else { 24 };
        append(
            ctx,
            &mut framing,
            card.start + locator..card.start + locator + 8,
        )?;
        append(ctx, &mut framing, card.start + 72..card.end)?;
        append(ctx, &mut dispatch, card.start..card.start + 8)?;
        if index == 1 {
            append(ctx, &mut dispatch, card.start + 32..card.start + 40)?;
        }
    }
    let parent = push(
        ctx,
        source,
        out,
        "iges.directory_entry",
        FrameParent::Part(section),
        first.start..last.end,
        (framing, dispatch),
    )?;
    for (index, card) in cards.iter().enumerate() {
        for field in 0..9 {
            let extent = card.start + field * 8..card.start + (field + 1) * 8;
            let framing = if (index == 0 && field == 1) || (index == 1 && field == 3) {
                vec![extent.clone()]
            } else {
                Vec::new()
            };
            let dispatch = if field == 0 || index == 1 && field == 4 {
                vec![extent.clone()]
            } else {
                Vec::new()
            };
            push(
                ctx,
                source,
                out,
                "iges.directory_field",
                FrameParent::Part(parent),
                extent,
                (framing, dispatch),
            )?;
        }
    }
    Ok(())
}

fn push<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
    level: &str,
    parent: FrameParent,
    extent: Range<usize>,
    (framing, dispatch): (Vec<Range<usize>>, Vec<Range<usize>>),
) -> Result<usize, String> {
    let absolute = |offset: usize| {
        source
            .start()
            .checked_add(offset)
            .ok_or_else(|| "IGES source offset overflows".to_owned())
    };
    let view = source
        .child(absolute(extent.start)?, absolute(extent.end)?)
        .ok_or("IGES part is outside its actual source")?;
    let convert = |ranges: Vec<Range<usize>>| {
        let mut result = Vec::new();
        for range in ranges {
            append(
                ctx,
                &mut result,
                u64_from_index(absolute(range.start)?)..u64_from_index(absolute(range.end)?),
            )?;
        }
        Ok::<_, String>(result)
    };
    let level = PartLevel::parse(level).ok_or("missing IGES Appendix C level")?;
    let part = FramedPart::new(
        ctx,
        level,
        view,
        parent,
        convert(framing)?,
        convert(dispatch)?,
    )?;
    let index = out.len();
    append(ctx, out, part)?;
    Ok(index)
}

fn append<T>(ctx: &DecodeContext<'_>, out: &mut Vec<T>, item: T) -> Result<(), String> {
    ctx.retain_bytes(
        u64_from_index(size_of::<T>()),
        "IGES physical frame storage",
    )
    .map_err(|error| error.to_string())?;
    out.try_reserve(1).map_err(|error| error.to_string())?;
    out.push(item);
    Ok(())
}

#[cfg(test)]
mod tests;
