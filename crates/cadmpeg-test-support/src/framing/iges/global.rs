// SPDX-License-Identifier: Apache-2.0
use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};

use super::{append, cards, push, Card, FrameParent, FramedPart, Kind};

pub(super) struct Field {
    pub(super) extent: Range<usize>,
    pub(super) header: Option<Range<usize>>,
    pub(super) value: Range<usize>,
}

/// Append Global sections and fields from actual Fixed-ASCII cards.
///
/// This adds `iges.global_section` and `iges.global_field` independently of
/// [`super::frame`]. The logical stream joins only columns 1–72 of consecutive
/// Global cards, with registered source segments. Delimiter declarations and
/// Hollerith lengths determine boundaries; payload bytes are not read as
/// semantic content. An invalid field stops this scan without marker recovery.
///
/// # Errors
/// Returns malformed framing, unsupported representation, invalid provenance,
/// or a caller-budget/allocation refusal.
pub fn frame_global<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    ctx.capture_part_extent(source)
        .map_err(|error| error.to_string())?;
    let cards = cards::scan(ctx, source.window())?;
    let mut at = 0;
    while at < cards.len() {
        if cards[at].kind != Kind::Card(b'G') {
            at += 1;
            continue;
        }
        let start = at;
        while at < cards.len() && cards[at].kind == Kind::Card(b'G') {
            at += 1;
        }
        section(ctx, source, &cards[start..at], out)?;
    }
    Ok(())
}

fn section<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cards: &[Card],
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let first = cards.first().ok_or("IGES Global section has no cards")?;
    let last = cards.last().ok_or("IGES Global section has no cards")?;
    let mut framing = Vec::new();
    let mut dispatch = Vec::new();
    for card in cards {
        append(ctx, &mut framing, card.start + 72..card.end)?;
        append(ctx, &mut dispatch, card.start + 72..card.start + 73)?;
    }
    let stream = logical_stream(ctx, source, cards, 72, "global")?;
    let bytes = stream.window();
    let mut fields = Vec::new();
    let (parameter, record, declarations_end, early) = declarations(bytes)?;
    let record_end = if early {
        declarations_end + 1
    } else {
        let mut cursor = declarations_end;
        loop {
            let next = field(bytes, cursor, parameter, Some(record), 72)?;
            cursor = next.extent.end;
            let ended = bytes[cursor - 1] == record;
            append(ctx, &mut fields, next)?;
            if ended {
                break cursor;
            }
        }
    };
    physical_ranges(ctx, cards, 0..declarations_end, &mut framing, 72)?;
    physical_ranges(ctx, cards, record_end - 1..record_end, &mut framing, 72)?;
    for (index, field) in fields.iter().enumerate() {
        if (4..=8).contains(&index) || index == 20 {
            physical_ranges(ctx, cards, field.value.clone(), &mut dispatch, 72)?;
        }
    }
    framing.sort_unstable_by_key(|range| (range.start, range.end));
    dispatch.sort_unstable_by_key(|range| (range.start, range.end));
    let parent = push(
        ctx,
        source,
        out,
        "iges.global_section",
        FrameParent::File,
        first.start..last.end,
        (framing, dispatch),
    )?;
    for field in fields {
        let mut framing = Vec::new();
        if let Some(header) = field.header {
            append(ctx, &mut framing, header)?;
        }
        append(ctx, &mut framing, field.extent.end - 1..field.extent.end)?;
        push(
            ctx,
            stream,
            out,
            "iges.global_field",
            FrameParent::Part(parent),
            field.extent,
            (framing, Vec::new()),
        )?;
    }
    Ok(())
}

pub(super) fn logical_stream<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cards: &[Card],
    width: usize,
    kind: &str,
) -> Result<View<'a>, String> {
    let first = cards.first().ok_or("IGES logical stream has no cards")?;
    let last = cards.last().ok_or("IGES logical stream has no cards")?;
    let mut payloads = Vec::new();
    for card in cards {
        let start = source
            .start()
            .checked_add(card.start)
            .ok_or("IGES logical source offset overflow")?;
        let end = start
            .checked_add(width)
            .ok_or("IGES logical payload offset overflow")?;
        let payload = source
            .child(start, end)
            .ok_or("IGES card payload is truncated")?;
        append(ctx, &mut payloads, payload)?;
    }
    ctx.concat_named_views(
        &payloads,
        &format!(
            "iges/{kind}/{}/{}/{}",
            ctx.capture_part_extent(source)
                .map_err(|error| error.to_string())?
                .stream(),
            source.start() + first.start,
            source.start() + last.end
        ),
    )
    .map_err(|error| error.to_string())
}

pub(super) fn declarations(bytes: &[u8]) -> Result<(u8, u8, usize, bool), String> {
    let start = skip_spaces(bytes, 0);
    if bytes.get(start) == Some(&b';') && bytes[start + 1..].iter().all(|byte| *byte == b' ') {
        return Ok((b',', b';', start, true));
    }
    let (parameter, cursor) = parameter_delimiter(bytes, start)?;
    let second = field(bytes, cursor, parameter, None, 72)?;
    let record = match second.header {
        Some(_) if second.value.len() == 1 => bytes[second.value.start],
        None if second.value.is_empty() => b';',
        _ => return Err("IGES Global record delimiter is not one Hollerith byte".into()),
    };
    if prohibited(parameter) || prohibited(record) {
        return Err("IGES Global delimiter is prohibited".into());
    }
    Ok((parameter, record, second.extent.end, false))
}

pub(super) fn physical_ranges(
    ctx: &DecodeContext<'_>,
    cards: &[Card],
    logical: Range<usize>,
    out: &mut Vec<Range<usize>>,
    width: usize,
) -> Result<(), String> {
    let mut at = logical.start;
    while at < logical.end {
        let card = cards
            .get(at / width)
            .ok_or("IGES Global range exceeds cards")?;
        let count = (width - at % width).min(logical.end - at);
        append(
            ctx,
            out,
            card.start + at % width..card.start + at % width + count,
        )?;
        at += count;
    }
    Ok(())
}

fn skip_spaces(bytes: &[u8], mut at: usize) -> usize {
    while bytes.get(at) == Some(&b' ') {
        at += 1;
    }
    at
}

fn prohibited(byte: u8) -> bool {
    byte.is_ascii_control()
        || !byte.is_ascii()
        || byte.is_ascii_digit()
        || matches!(byte, b' ' | b'+' | b'-' | b'.' | b'D' | b'E' | b'H')
}

fn hollerith(
    bytes: &[u8],
    start: usize,
    width: usize,
) -> Result<Option<(Range<usize>, Range<usize>)>, String> {
    let mut at = start;
    let mut count = Some(0_usize);
    while let Some(byte) = bytes.get(at).filter(|byte| byte.is_ascii_digit()) {
        count = count
            .and_then(|count| count.checked_mul(10))
            .and_then(|count| count.checked_add(usize::from(*byte - b'0')));
        at += 1;
    }
    if at == start || !matches!(bytes.get(at), Some(b'H' | b'h')) {
        return Ok(None);
    }
    let header_end = at + 1;
    if start / width != at / width {
        return Err("IGES Hollerith header crosses a card".into());
    }
    let end = header_end
        .checked_add(count.ok_or("IGES Hollerith count overflow")?)
        .filter(|end| *end <= bytes.len())
        .ok_or("IGES Hollerith payload is truncated")?;
    Ok(Some((start..header_end, header_end..end)))
}

fn parameter_delimiter(bytes: &[u8], start: usize) -> Result<(u8, usize), String> {
    if bytes.get(start) == Some(&b',') {
        return Ok((b',', start + 1));
    }
    let Some((_, value)) = hollerith(bytes, start, 72)? else {
        return Err("IGES Global parameter delimiter is not Hollerith".into());
    };
    if value.len() != 1 || bytes.get(value.end) != Some(&bytes[value.start]) {
        return Err("IGES Global parameter delimiter does not end its declaration".into());
    }
    Ok((bytes[value.start], value.end + 1))
}

pub(super) fn field(
    bytes: &[u8],
    start: usize,
    parameter: u8,
    record: Option<u8>,
    width: usize,
) -> Result<Field, String> {
    let value_start = skip_spaces(bytes, start);
    let delimiter = |byte: u8| byte == parameter || record == Some(byte);
    if bytes.get(value_start).copied().is_some_and(delimiter) {
        return Ok(Field {
            extent: start..value_start + 1,
            header: None,
            value: value_start..value_start,
        });
    }
    if let Some((header, value)) = hollerith(bytes, value_start, width)? {
        let end = skip_spaces(bytes, value.end);
        if !bytes.get(end).copied().is_some_and(delimiter) {
            return Err("IGES Hollerith value has no following delimiter".into());
        }
        return Ok(Field {
            extent: start..end + 1,
            header: Some(header),
            value,
        });
    }
    let mut end = value_start;
    while let Some(byte) = bytes.get(end) {
        if delimiter(*byte) {
            if value_start / width != end / width {
                return Err("IGES atom and delimiter cross a card".into());
            }
            return Ok(Field {
                extent: start..end + 1,
                header: None,
                value: value_start..end,
            });
        }
        end += 1;
    }
    Err("IGES field has no terminating delimiter".into())
}

#[cfg(test)]
mod tests;
