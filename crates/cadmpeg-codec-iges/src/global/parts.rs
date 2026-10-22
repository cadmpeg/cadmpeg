// SPDX-License-Identifier: Apache-2.0
//! Global value parsing over genuine card payload composites.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};

use super::{frame_value, parse_raw_with, resolve, ResolvedGlobal, Value, ValueFrame, ValueKind};
use crate::card::{CardScan, PhysicalLine, Section};

pub(crate) fn parse(
    scan: &CardScan<'_>,
    ctx: &DecodeContext<'_>,
    output: &mut ParserPartOutput,
) -> Result<(ResolvedGlobal, Vec<LossNote>), CodecError> {
    let source = scan
        .source_view()
        .ok_or_else(|| super::malformed("source view is absent"))?;
    ctx.capture_part_extent(source)?;
    let mut cards = Vec::new();
    for (_, card) in scan.section(Section::Global) {
        append_global_item(ctx, &mut cards, card)?;
    }
    let first = cards
        .first()
        .ok_or_else(|| super::malformed("section is missing"))?;
    let last = cards
        .last()
        .ok_or_else(|| super::malformed("section is missing"))?;
    for pair in cards.windows(2) {
        if physical_end(source, pair[0])? != absolute(source, pair[1].offset)? {
            return super::parse(scan);
        }
    }
    let extent = source
        .child(absolute(source, first.offset)?, physical_end(source, last)?)
        .ok_or_else(|| super::malformed("section exceeds its source"))?;
    let mut inputs = Vec::new();
    let mut framing = Vec::new();
    for card in &cards {
        let start = absolute(source, card.offset)?;
        let payload = source
            .child(start, start + 72)
            .ok_or_else(|| super::malformed("card payload exceeds its source"))?;
        append_global_item(ctx, &mut inputs, payload)?;
        append_global_item(
            ctx,
            &mut framing,
            u64_from_index(start + 72)..u64_from_index(physical_end(source, card)?),
        )?;
    }
    let stream = ctx.concat_named_views(
        &inputs,
        &format!(
            "iges/global/{}/{}/{}",
            ctx.capture_part_extent(source)?.stream(),
            extent.start(),
            extent.end()
        ),
    )?;
    let bytes = stream.window();
    match section_framing(ctx, source, &cards, bytes, &mut framing) {
        Ok(()) => {}
        Err(CodecError::Malformed(_)) => {}
        Err(error) => return Err(error),
    }
    framing.sort_unstable_by_key(|range| (range.start, range.end));
    let admission = match parse_raw_with(bytes, |_, frame| Ok(frame.window_value(bytes))) {
        Ok(raw) => Some(resolve(raw).0),
        Err(CodecError::Malformed(_)) => None,
        Err(error) => return Err(error),
    };
    let section_spec = spec("iges.global_section", framing)?;
    let mut failure = None;
    let parsed = ctx.decode_parser_scope(output, extent, section_spec, |children| {
        let mut index = 2;
        let raw = parse_raw_with(bytes, |start, frame| {
            let semantic = admission.as_ref().is_some_and(|global| match index {
                super::FIELD_SENDER_PRODUCT => global.sender_product().is_some(),
                super::FIELD_FILE_NAME => global.native_file_name().is_some(),
                super::FIELD_UNITS_NAME => global.units_name().is_some(),
                super::FIELD_VERSION_FLAG => true,
                _ => false,
            });
            index += 1;
            let extent = stream
                .child(start, frame.next)
                .ok_or_else(|| super::malformed("field exceeds its payload stream"))?;
            let mut framing = Vec::new();
            if let Some(header) = &frame.header {
                append_global_item(
                    ctx,
                    &mut framing,
                    u64_from_index(header.start)..u64_from_index(header.end),
                )?;
            }
            append_global_item(
                ctx,
                &mut framing,
                u64_from_index(frame.next - 1)..u64_from_index(frame.next),
            )?;
            let spec = spec("iges.global_field", framing)?;
            let capture = ctx.capture_part_extent(extent)?;
            let id = format!(
                "iges:global-field:{}:{}:{}",
                capture.stream(),
                capture.offset(),
                capture.byte_len()
            );
            ctx.decode_parser_part(children, extent, spec, || {
                ctx.record(&id, extent, || read_value(ctx, stream, frame, semantic))
            })?
            .ok_or_else(|| super::malformed("field value was not decoded"))
        });
        match raw {
            Ok(raw) => Ok(resolve(raw)),
            Err(error) => {
                failure = Some(error.to_string());
                Err(error)
            }
        }
    })?;
    parsed.ok_or_else(|| {
        CodecError::malformed(
            failure.unwrap_or_else(|| "IGES Global section was not decoded".into()),
        )
    })
}

fn read_value(
    ctx: &DecodeContext<'_>,
    stream: View<'_>,
    frame: &ValueFrame,
    semantic: bool,
) -> Result<Value, CodecError> {
    if matches!(frame.kind, ValueKind::Omitted) {
        return Ok(Value::Omitted);
    }
    let length = frame.payload.len();
    ctx.retain_bytes(u64_from_index(length), "IGES Global value")?;
    let mut payload = ctx.alloc_filled(length, 0u8, "IGES Global value")?;
    let source = if semantic {
        stream
            .read_slice(frame.payload.clone())
            .ok_or_else(|| super::malformed("field payload is unreadable"))?
    } else {
        &stream.window()[frame.payload.clone()]
    };
    payload.copy_from_slice(source);
    Ok(frame.owned_value(payload))
}

fn section_framing(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    cards: &[&PhysicalLine],
    bytes: &[u8],
    framing: &mut Vec<Range<u64>>,
) -> Result<(), CodecError> {
    if bytes.iter().copied().filter(|byte| *byte != b' ').eq(*b";") {
        let terminal = bytes
            .iter()
            .position(|byte| *byte == b';')
            .ok_or_else(|| super::malformed("record delimiter is absent"))?;
        return physical_ranges(ctx, source, cards, terminal..terminal + 1, framing);
    }
    let (delimiter, cursor) = super::first_delimiter(bytes)?;
    let declaration_end = frame_value(bytes, cursor, delimiter, None, true)?.next;
    physical_ranges(ctx, source, cards, 0..declaration_end, framing)?;
    let mut terminal = None;
    parse_raw_with(bytes, |_, frame| {
        if frame.ended {
            terminal = Some(frame.next - 1);
        }
        Ok(Value::Omitted)
    })?;
    if let Some(terminal) = terminal {
        physical_ranges(ctx, source, cards, terminal..terminal + 1, framing)?;
    }
    Ok(())
}

fn physical_ranges(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    cards: &[&PhysicalLine],
    range: Range<usize>,
    out: &mut Vec<Range<u64>>,
) -> Result<(), CodecError> {
    let mut at = range.start;
    while at < range.end {
        let card = cards
            .get(at / 72)
            .ok_or_else(|| super::malformed("framing exceeds Global cards"))?;
        let start = absolute(source, card.offset)? + at % 72;
        let count = (72 - at % 72).min(range.end - at);
        append_global_item(
            ctx,
            out,
            u64_from_index(start)..u64_from_index(start + count),
        )?;
        at += count;
    }
    Ok(())
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

fn spec(name: &str, framing: Vec<Range<u64>>) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(name)
        .ok_or_else(|| super::malformed("part level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, framing))
}

fn append_global_item<T>(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<T>,
    value: T,
) -> Result<(), CodecError> {
    let bytes = u64_from_index(size_of::<T>());
    ctx.retain_bytes(bytes, "IGES Global parser storage")?;
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
