// SPDX-License-Identifier: Apache-2.0
use std::ops::Range;

use cadmpeg_core::decode::{DecodeContext, View};

use super::{append, cards, global, push, Card, FrameParent, FramedPart, Kind};

struct Entry {
    range: Range<usize>,
    owner: usize,
    entity_type: usize,
    start: Option<usize>,
    count: Option<usize>,
}

struct OwnerRun {
    owner: usize,
    range: Range<usize>,
}

struct Plan {
    entry: usize,
    range: Range<usize>,
}

/// Append Parameter Data records and ordinary fields from their source owners.
///
/// Directory fields 2 and 14 locate card ranges. An unusable declaration uses
/// the first contiguous run of actual owner back-pointers. Conflicting owners
/// and overlapping declarations are rejected. Columns 1–64 form registered
/// composites; counted strings shield delimiters and preserve source seams.
/// Existing physical Parameter section frames are reused, or appended if absent.
/// This slice requires one contiguous run of each Global, Directory and
/// Parameter section. A Macro Definition retains its proved card extent and
/// type dispatch; its terminal delimiter and statement children remain unframed.
///
/// # Errors
/// Returns unsupported or malformed framing, invalid provenance, or a caller
/// budget/allocation refusal. No marker scan recovers a failed field.
pub fn frame_parameters<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let source_capture = ctx
        .capture_part_extent(source)
        .map_err(|error| error.to_string())?;
    let cards = cards::scan(ctx, source.window())?;
    let parameters = run(&cards, b'P')?;
    if parameters.is_empty() {
        return Ok(());
    }
    let globals = run(&cards, b'G')?;
    let global = global::logical_stream(ctx, source, globals, 72, "global")?;
    let (parameter, record, _, _) = global::declarations(global.window())?;
    let entries = entries(ctx, source, run(&cards, b'D')?)?;
    let plans = ownership(ctx, source, parameters, &entries)?;
    let parent = parameter_section(ctx, source, parameters, &source_capture, out)?;
    for plan in plans {
        record_frames(
            ctx,
            source,
            &parameters[plan.range],
            &entries[plan.entry],
            (parameter, record),
            parent,
            out,
        )?;
    }
    Ok(())
}

fn run(cards: &[Card], marker: u8) -> Result<&[Card], String> {
    let Some(start) = cards
        .iter()
        .position(|card| card.kind == Kind::Card(marker))
    else {
        return Ok(&[]);
    };
    let end = start
        + cards[start..]
            .iter()
            .take_while(|card| card.kind == Kind::Card(marker))
            .count();
    if cards[end..]
        .iter()
        .any(|card| card.kind == Kind::Card(marker))
    {
        return Err("IGES Parameter framing requires contiguous section cards".into());
    }
    Ok(&cards[start..end])
}

fn number(bytes: &[u8]) -> Option<usize> {
    let start = bytes.iter().position(|byte| *byte != b' ')?;
    let end = bytes.iter().rposition(|byte| *byte != b' ')? + 1;
    let bytes = bytes[start..end]
        .strip_prefix(b"+")
        .unwrap_or(&bytes[start..end]);
    if bytes.is_empty() || bytes.iter().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    bytes.iter().try_fold(0_usize, |value, byte| {
        value
            .checked_mul(10)?
            .checked_add(usize::from(*byte - b'0'))
    })
}

fn entries(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    cards: &[Card],
) -> Result<Vec<Entry>, String> {
    if cards.is_empty() {
        return Err("IGES Parameter records have no Directory owners".into());
    }
    let mut entries = Vec::new();
    for (index, pair) in cards.chunks(2).enumerate() {
        let first = pair[0];
        let bytes = source.window();
        let entity_type = number(&bytes[first.start..first.start + 8])
            .ok_or("IGES Directory entity type is unreadable")?;
        let count = if let Some(second) = pair.get(1) {
            if number(&bytes[second.start..second.start + 8]) != Some(entity_type) {
                return Err("IGES Directory repeated entity type disagrees".into());
            }
            number(&bytes[second.start + 24..second.start + 32])
        } else {
            None
        };
        append(
            ctx,
            &mut entries,
            Entry {
                range: first.start..pair.last().ok_or("IGES Directory pair is empty")?.end,
                owner: index
                    .checked_mul(2)
                    .and_then(|owner| owner.checked_add(1))
                    .ok_or("IGES Directory owner index overflow")?,
                entity_type,
                start: number(&bytes[first.start + 8..first.start + 16]),
                count,
            },
        )?;
    }
    Ok(entries)
}

fn back_pointer(source: View<'_>, card: Card) -> Option<usize> {
    let field = &source.window()[card.start + 64..card.start + 72];
    if field[0] != b' ' {
        return None;
    }
    let start = field[1..].iter().position(|byte| *byte != b' ')? + 1;
    let digits = &field[start..];
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    number(digits).filter(|value| *value > 0 && value % 2 == 1)
}

fn ownership(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    cards: &[Card],
    entries: &[Entry],
) -> Result<Vec<Plan>, String> {
    let plans = locations(ctx, source, cards, entries)?;
    for pair in plans.windows(2) {
        if pair[0].range.end > pair[1].range.start {
            return Err("IGES Directory Parameter ranges overlap".into());
        }
    }
    for plan in &plans {
        let owner = entries[plan.entry].owner;
        for pointer in cards[plan.range.clone()]
            .iter()
            .filter_map(|card| back_pointer(source, *card))
        {
            if pointer != owner
                && entries
                    .binary_search_by_key(&pointer, |entry| entry.owner)
                    .is_ok()
            {
                return Err(
                    "IGES declared Parameter range conflicts with another Directory owner".into(),
                );
            }
        }
    }
    Ok(plans)
}

fn locations(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    cards: &[Card],
    entries: &[Entry],
) -> Result<Vec<Plan>, String> {
    let mut runs = Vec::new();
    let mut pointers = Vec::new();
    for card in cards {
        append(ctx, &mut pointers, back_pointer(source, *card))?;
    }
    let mut at = 0;
    while at < pointers.len() {
        let start = at;
        let owner = pointers[at];
        while at < pointers.len() && pointers[at] == owner {
            at += 1;
        }
        if let Some(owner) = owner {
            append(
                ctx,
                &mut runs,
                OwnerRun {
                    owner,
                    range: start..at,
                },
            )?;
        }
    }
    runs.sort_unstable_by_key(|run| (run.owner, run.range.start));
    let mut plans = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.entity_type == 0 && entry.count == Some(0) {
            continue;
        }
        let declared = entry
            .start
            .zip(entry.count)
            .filter(|(start, count)| *start > 0 && *count > 0)
            .and_then(|(start, count)| {
                let start = start - 1;
                let end = start.checked_add(count)?;
                (end <= cards.len()).then_some(start..end)
            });
        let range = match declared {
            Some(range) => range,
            None => {
                let at = runs.partition_point(|run| run.owner < entry.owner);
                runs.get(at)
                    .filter(|run| run.owner == entry.owner)
                    .ok_or("IGES Directory declaration and owner census locate no Parameter cards")?
                    .range
                    .clone()
            }
        };
        append(
            ctx,
            &mut plans,
            Plan {
                entry: index,
                range,
            },
        )?;
    }
    plans.sort_unstable_by_key(|plan| (plan.range.start, plan.range.end, plan.entry));
    Ok(plans)
}

fn parameter_section<'a>(
    ctx: &DecodeContext<'_>,
    source: View<'a>,
    parameters: &[Card],
    source_capture: &cadmpeg_core::decode::PartCapture,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<usize, String> {
    let first = parameters
        .first()
        .ok_or("IGES Parameter section has no cards")?;
    let last = parameters
        .last()
        .ok_or("IGES Parameter section has no final card")?;
    let start = source.start() + first.start;
    let end = source.start() + last.end;
    let parent = match out.iter().position(|part| {
        part.level().as_str() == "iges.parameter_section"
            && part.capture().stream() == source_capture.stream()
            && part.view().start() == start
            && part.view().end() == end
    }) {
        Some(parent) => {
            ctx.capture_part_extent(out[parent].view())
                .map_err(|error| error.to_string())?;
            parent
        }
        None => {
            let parent = out.len();
            super::section(ctx, source, parameters, b'P', out)?;
            parent
        }
    };
    Ok(parent)
}

fn record_frames<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cards: &[Card],
    entry: &Entry,
    (parameter, record): (u8, u8),
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<(), String> {
    let stream = global::logical_stream(ctx, source, cards, 64, "parameter")?;
    let bytes = stream.window();
    let entity_type = global::field(bytes, 0, parameter, Some(record), 64)?;
    if entity_type.header.is_some()
        || number(&bytes[entity_type.value.clone()]) != Some(entry.entity_type)
    {
        return Err("IGES Parameter entity type disagrees with Directory owner".into());
    }
    let type_range = entity_type.value.clone();
    let mut fields = Vec::new();
    let record_end = if entry.entity_type == 306 {
        None
    } else {
        let mut cursor = entity_type.extent.end;
        append(ctx, &mut fields, entity_type)?;
        while bytes[cursor - 1] != record {
            let field = global::field(bytes, cursor, parameter, Some(record), 64)?;
            if field.header.is_some() && field.value.is_empty() {
                return Err("IGES Parameter Hollerith count must be positive".into());
            }
            cursor = field.extent.end;
            append(ctx, &mut fields, field)?;
        }
        Some(cursor)
    };
    let mut framing = Vec::new();
    let mut dispatch = Vec::new();
    for card in cards {
        append(ctx, &mut framing, card.start + 64..card.end)?;
    }
    if let Some(cursor) = record_end {
        global::physical_ranges(ctx, cards, cursor - 1..cursor, &mut framing, 64)?;
    }
    global::physical_ranges(ctx, cards, type_range, &mut dispatch, 64)?;
    framing.sort_unstable_by_key(|range| (range.start, range.end));
    let first = cards.first().ok_or("IGES Parameter record has no cards")?;
    let last = cards
        .last()
        .ok_or("IGES Parameter record has no final card")?;
    let parent = push(
        ctx,
        source,
        out,
        "iges.parameter_record",
        FrameParent::Part(parent),
        first.start..last.end,
        (framing, dispatch),
    )?;
    for field in fields {
        let mut framing = Vec::new();
        if let Some(header) = field.header {
            append(ctx, &mut framing, header)?;
        }
        append(ctx, &mut framing, field.extent.end - 1..field.extent.end)?;
        let dispatch = if field.extent.start == 0 {
            let mut dispatch = Vec::new();
            append(ctx, &mut dispatch, field.value)?;
            dispatch
        } else {
            Vec::new()
        };
        push(
            ctx,
            stream,
            out,
            "iges.parameter_field",
            FrameParent::Part(parent),
            field.extent,
            (framing, dispatch),
        )?;
    }
    Ok(())
}

mod claims;

pub use claims::{frame_parameter_claims, ParameterClaim};

#[cfg(test)]
mod tests;
