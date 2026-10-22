// SPDX-License-Identifier: Apache-2.0
//! Physical Parameter extents and the actual Directory entries that claim them.

use cadmpeg_core::decode::{DecodeContext, View};

use super::{
    append, cards, entries, global, locations, number, parameter_section, push, run, Card,
    FrameParent, FramedPart,
};

/// An actual Directory Entry and its single physical Parameter frame.
#[derive(Debug)]
pub struct ParameterClaim<'a> {
    directory: View<'a>,
    record: usize,
}

impl<'a> ParameterClaim<'a> {
    /// Original paired or unpaired Directory cards that supply this claim.
    pub fn directory(&self) -> View<'a> {
        self.directory
    }

    /// Index in the caller's frame inventory, shared by equal physical claims.
    pub fn record(&self) -> usize {
        self.record
    }
}

/// Frame physical Parameter records independently of Directory ownership validity.
///
/// Uses the same declared card extents and real back-pointer fallback as
/// [`super::frame_parameters`]. Equal claims share one frame; inconsistent owner
/// pointers do not erase a proved extent. Unequal overlapping claims are refused
/// before appending frames. Returned claims retain their actual Directory Views.
/// The record delimiter is framed only when its lexical boundary is proved;
/// malformed fields retain the physical record without guessed field children.
/// This supports contiguous Fixed ASCII sections only.
///
/// # Errors
/// Returns an ambiguous or absent extent, unsupported physical representation,
/// invalid source provenance, or caller resource refusal. Existing frames remain.
pub fn frame_parameter_claims<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<Vec<ParameterClaim<'a>>, String> {
    let capture = ctx
        .capture_part_extent(source)
        .map_err(|error| error.to_string())?;
    let cards = cards::scan(ctx, source.window())?;
    let parameters = run(&cards, b'P')?;
    if parameters.is_empty() {
        return Ok(Vec::new());
    }
    let global = global::logical_stream(ctx, source, run(&cards, b'G')?, 72, "global")?;
    let (parameter, record, _, _) = global::declarations(global.window())?;
    let entries = entries(ctx, source, run(&cards, b'D')?)?;
    let plans = locations(ctx, source, parameters, &entries)?;
    for pair in plans.windows(2) {
        if pair[0].range != pair[1].range && pair[0].range.end > pair[1].range.start {
            return Err(
                "IGES unequal Parameter claims overlap; only their section is bounded".into(),
            );
        }
    }
    let parent = parameter_section(ctx, source, parameters, &capture, out)?;
    let mut previous = None;
    let mut claims = Vec::new();
    for plan in plans {
        let index = match &previous {
            Some((range, index)) if range == &plan.range => *index,
            _ => {
                let index = physical_record(
                    ctx,
                    source,
                    &parameters[plan.range.clone()],
                    (parameter, record),
                    parent,
                    out,
                )?;
                previous = Some((plan.range.clone(), index));
                index
            }
        };
        let range = &entries[plan.entry].range;
        let start = source
            .start()
            .checked_add(range.start)
            .ok_or("IGES Directory claim start overflows")?;
        let end = source
            .start()
            .checked_add(range.end)
            .ok_or("IGES Directory claim end overflows")?;
        let directory = source
            .child(start, end)
            .ok_or("IGES Directory claim lies outside its source")?;
        append(
            ctx,
            &mut claims,
            ParameterClaim {
                directory,
                record: index,
            },
        )?;
    }
    Ok(claims)
}

fn physical_record<'a>(
    ctx: &DecodeContext<'a>,
    source: View<'a>,
    cards: &[Card],
    (parameter, record): (u8, u8),
    parent: usize,
    out: &mut Vec<FramedPart<'a>>,
) -> Result<usize, String> {
    let stream = global::logical_stream(ctx, source, cards, 64, "parameter")?;
    let bytes = stream.window();
    let mut framing = Vec::new();
    let mut dispatch = Vec::new();
    for card in cards {
        append(ctx, &mut framing, card.start + 64..card.end)?;
    }
    if let Ok(first) = global::field(bytes, 0, parameter, Some(record), 64) {
        global::physical_ranges(ctx, cards, first.value.clone(), &mut dispatch, 64)?;
        if first.header.is_none() && number(&bytes[first.value.clone()]) != Some(306) {
            let mut cursor = first.extent.end;
            while bytes[cursor - 1] != record {
                let Ok(field) = global::field(bytes, cursor, parameter, Some(record), 64) else {
                    break;
                };
                if field.header.is_some() && field.value.is_empty() {
                    break;
                }
                cursor = field.extent.end;
            }
            if bytes[cursor - 1] == record {
                global::physical_ranges(ctx, cards, cursor - 1..cursor, &mut framing, 64)?;
            }
        }
    }
    framing.sort_unstable_by_key(|range| (range.start, range.end));
    let first = cards.first().ok_or("IGES Parameter claim has no cards")?;
    let last = cards
        .last()
        .ok_or("IGES Parameter claim has no final card")?;
    push(
        ctx,
        source,
        out,
        "iges.parameter_record",
        FrameParent::Part(parent),
        first.start..last.end,
        (framing, dispatch),
    )
}
