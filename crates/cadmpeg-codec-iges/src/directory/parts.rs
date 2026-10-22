// SPDX-License-Identifier: Apache-2.0
//! Source-bounded Directory parsing through the shared parser part runner.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};

use super::{parse_pair, DirectoryDefect, DirectoryEntry, GlobalTable};
use crate::card::{CardScan, PhysicalLine, ScannedLine, Section};

pub(crate) fn parse(
    scan: &CardScan<'_>,
    global: GlobalTable,
    ctx: &DecodeContext<'_>,
    output: &mut ParserPartOutput,
) -> Result<Vec<DirectoryEntry>, CodecError> {
    let source = scan
        .source_view()
        .ok_or_else(|| CodecError::malformed("Directory source view is absent"))?;
    ctx.capture_part_extent(source)?;
    let mut entries = Vec::new();
    let mut at = 0;
    while at < scan.lines.len() {
        if !matches!(
            scan.lines[at],
            ScannedLine::Card {
                section: Section::Directory,
                ..
            }
        ) {
            at += 1;
            continue;
        }
        let start = at;
        while at < scan.lines.len()
            && matches!(
                scan.lines[at],
                ScannedLine::Card {
                    section: Section::Directory,
                    ..
                }
            )
        {
            at += 1;
        }
        let lines = &scan.lines[start..at];
        let extent = source_extent(source, lines)?;
        let mut framing = Vec::new();
        for line in lines {
            envelope(ctx, source, line.physical(), &mut framing)?;
        }
        let spec = part_spec("iges.directory_section", framing)?;
        let parsed = ctx.decode_parser_scope(output, extent, spec, |children| {
            let mut entries = Vec::new();
            for pair in lines.chunks(2) {
                ctx.charge_entities(1, "iges_directory_entries")?;
                let extent = source_extent(source, pair)?;
                let mut framing = Vec::new();
                envelope(ctx, source, pair[0].physical(), &mut framing)?;
                let first_start = absolute(source, pair[0].physical().offset)?;
                append_parser_item(
                    ctx,
                    &mut framing,
                    u64_from_index(first_start + 8)..u64_from_index(first_start + 16),
                )?;
                if let Some(second) = pair.get(1) {
                    envelope(ctx, source, second.physical(), &mut framing)?;
                    let second_start = absolute(source, second.physical().offset)?;
                    append_parser_item(
                        ctx,
                        &mut framing,
                        u64_from_index(second_start + 24)..u64_from_index(second_start + 32),
                    )?;
                }
                framing.sort_unstable_by_key(|range| (range.start, range.end));
                let spec = part_spec("iges.directory_entry", framing)?;
                if let Some(entry) = ctx.decode_parser_part(children, extent, spec, || {
                    let ScannedLine::Card {
                        sequence,
                        line: first,
                        ..
                    } = &pair[0]
                    else {
                        return Err(CodecError::malformed("Directory card is absent"));
                    };
                    let Some(second) = pair.get(1) else {
                        return Err(CodecError::malformed(DirectoryDefect::UnpairedCard));
                    };
                    parse_pair(*sequence, first, second.physical(), global)
                        .map_err(CodecError::malformed)
                })? {
                    append_parser_item(ctx, &mut entries, entry)?;
                }
            }
            Ok(entries)
        })?;
        if let Some(parsed) = parsed {
            for entry in parsed {
                append_parser_item(ctx, &mut entries, entry)?;
            }
        }
    }
    Ok(entries)
}

fn absolute(source: View<'_>, offset: u64) -> Result<usize, CodecError> {
    usize::try_from(offset)
        .ok()
        .and_then(|offset| source.start().checked_add(offset))
        .ok_or_else(|| CodecError::malformed("Directory source offset overflows"))
}

fn end(source: View<'_>, line: &PhysicalLine) -> Result<usize, CodecError> {
    absolute(source, line.offset)?
        .checked_add(line.payload.len())
        .and_then(|end| end.checked_add(line.line_ending().len()))
        .ok_or_else(|| CodecError::malformed("Directory card extent overflows"))
}

fn source_extent<'a>(source: View<'a>, lines: &[ScannedLine]) -> Result<View<'a>, CodecError> {
    let first = lines
        .first()
        .ok_or_else(|| CodecError::malformed("Directory part has no cards"))?;
    let last = lines
        .last()
        .ok_or_else(|| CodecError::malformed("Directory part has no cards"))?;
    source
        .child(
            absolute(source, first.physical().offset)?,
            end(source, last.physical())?,
        )
        .ok_or_else(|| CodecError::malformed("Directory cards exceed their actual source"))
}

fn envelope(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    line: &PhysicalLine,
    out: &mut Vec<Range<u64>>,
) -> Result<(), CodecError> {
    let start = absolute(source, line.offset)?
        .checked_add(72)
        .ok_or_else(|| CodecError::malformed("Directory envelope offset overflows"))?;
    append_parser_item(
        ctx,
        out,
        u64_from_index(start)..u64_from_index(end(source, line)?),
    )
}

fn part_spec(name: &str, framing: Vec<Range<u64>>) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(name)
        .ok_or_else(|| CodecError::malformed("Directory part level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, framing))
}

fn append_parser_item<T>(
    ctx: &DecodeContext<'_>,
    out: &mut Vec<T>,
    value: T,
) -> Result<(), CodecError> {
    let bytes = u64_from_index(size_of::<T>());
    ctx.retain_bytes(bytes, "IGES Directory parser storage")?;
    out.try_reserve(1).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "IGES Directory parser storage",
        )
    })?;
    out.push(value);
    Ok(())
}

#[cfg(test)]
mod tests;
