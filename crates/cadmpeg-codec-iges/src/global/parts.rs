// SPDX-License-Identifier: Apache-2.0
//! Global value parsing over genuine card payload composites.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};
use cadmpeg_ir::StreamName;

use super::{parse_raw_with, resolve, ResolvedGlobal, Value, ValueFrame, ValueKind};
use crate::card::{CardScan, PhysicalLine, Section};

/// One Global card: its absolute start, and its end after the line ending.
/// Columns 1-72 hold Global data.
#[derive(Clone, Copy)]
pub(crate) struct GlobalCard {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn parse(
    scan: &CardScan<'_>,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
) -> Result<(ResolvedGlobal, Vec<LossNote>), CodecError> {
    let source = scan
        .source_view()
        .ok_or_else(|| super::malformed("source view is absent"))?;
    let mut cards = Vec::new();
    for (_, card) in scan.section(Section::Global) {
        append_global_item(
            ctx,
            &mut cards,
            GlobalCard {
                start: absolute(source, card.offset)?,
                end: physical_end(source, card)?,
            },
        )?;
    }
    for pair in cards.windows(2) {
        if pair[0].end != pair[1].start {
            return super::parse(scan);
        }
    }
    parse_cards(ctx, source, &cards, output)
}

/// Parse contiguous Global cards as one `iges.global_section` part with one
/// `iges.global_field` part per field from field 3. A given-up section leaves
/// every Global value absent; a field whose end is unknown gives up the rest
/// of the Global stream, and the later fields are absent.
pub(crate) fn parse_cards(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    cards: &[GlobalCard],
    output: &mut PartOutput,
) -> Result<(ResolvedGlobal, Vec<LossNote>), CodecError> {
    let first = cards
        .first()
        .ok_or_else(|| super::malformed("section is missing"))?;
    let last = cards
        .last()
        .ok_or_else(|| super::malformed("section is missing"))?;
    let extent = source
        .child(first.start, last.end)
        .ok_or_else(|| super::malformed("section exceeds its source"))?;
    let mut inputs = Vec::new();
    for card in cards {
        let payload = source
            .child(card.start, card.start + 72)
            .ok_or_else(|| super::malformed("card payload exceeds its source"))?;
        append_global_item(ctx, &mut inputs, payload)?;
    }
    let stream = ctx.concat_views(&inputs)?;
    let name = StreamName::try_from(format!("iges/global/{}/{}", extent.start(), extent.end()))
        .map_err(|_| super::malformed("stream name is empty"))?;
    let field_spec = || crate::reader::part("iges.global_field").in_stream(name.clone());
    let parsed = ctx.decode_part(
        output,
        extent,
        crate::reader::part("iges.global_section"),
        |children| {
            let raw = parse_raw_with(stream.window(), |start, frame| {
                let extent = stream
                    .child(start, frame.next)
                    .ok_or_else(|| super::malformed("field exceeds its payload stream"))?;
                Ok(ctx
                    .decode_part(children, extent, field_spec(), |_| {
                        read_value(ctx, stream, frame)
                    })?
                    .unwrap_or(Value::Omitted))
            })?;
            if let Some(unframed) = &raw.unframed {
                let rest = stream
                    .child(unframed.offset, stream.end())
                    .ok_or_else(|| super::malformed("field exceeds its payload stream"))?;
                ctx.decode_part(
                    children,
                    rest,
                    field_spec(),
                    |_| -> Result<(), CodecError> {
                        Err(super::malformed(format!(
                            "field {} has no end: {}",
                            unframed.field + 1,
                            unframed.error
                        )))
                    },
                )?;
            }
            Ok(resolve(raw))
        },
    )?;
    Ok(parsed.unwrap_or_else(super::given_up))
}

fn read_value(
    ctx: &DecodeContext<'_>,
    stream: View<'_>,
    frame: &ValueFrame,
) -> Result<Value, CodecError> {
    if matches!(frame.kind, ValueKind::Omitted) {
        return Ok(Value::Omitted);
    }
    let length = frame.payload.len();
    ctx.retain_bytes(u64_from_index(length), "IGES Global value")?;
    let mut payload = ctx.alloc_filled(length, 0u8, "IGES Global value")?;
    let source = stream
        .child(frame.payload.start, frame.payload.end)
        .ok_or_else(|| super::malformed("field payload is unreadable"))?;
    payload.copy_from_slice(source.window());
    Ok(frame.owned_value(payload))
}

fn absolute(source: View<'_>, offset: u64) -> Result<usize, CodecError> {
    usize::try_from(offset)
        .ok()
        .and_then(|offset| source.start().checked_add(offset))
        .ok_or_else(|| super::malformed("source offset overflows"))
}

fn physical_end(source: View<'_>, card: &PhysicalLine) -> Result<usize, CodecError> {
    absolute(source, card.offset)?
        .checked_add(card.payload.len())
        .and_then(|end| end.checked_add(card.line_ending().len()))
        .ok_or_else(|| super::malformed("card extent overflows"))
}

/// Push parser working storage. It is not retained bytes
/// (`docs/decode-resource-caps.md`, "Retained bytes"); the input length bounds
/// it.
fn append_global_item<T>(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<T>,
    value: T,
) -> Result<(), CodecError> {
    let bytes = u64_from_index(size_of::<T>());
    out.try_reserve(1).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "IGES Global parser storage",
        )
    })?;
    out.push(value);
    Ok(())
}

#[cfg(test)]
mod tests;
