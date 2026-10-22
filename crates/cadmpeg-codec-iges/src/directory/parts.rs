// SPDX-License-Identifier: Apache-2.0
//! Source-bounded Directory parsing through the shared parser part runner.

use std::collections::BTreeSet;
use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};

use super::{admit_fields_with, fields, DirectoryDefect, DirectoryEntry, GlobalTable};
use crate::card::{CardScan, PhysicalLine, ScannedLine, Section};

mod metadata;

pub(crate) fn parse(
    scan: &CardScan<'_>,
    global: GlobalTable,
    ctx: &DecodeContext<'_>,
    output: &mut ParserPartOutput,
) -> Result<DirectoryParts, CodecError> {
    let source = scan
        .source_view()
        .ok_or_else(|| CodecError::malformed("Directory source view is absent"))?;
    ctx.capture_part_extent(source)?;
    let parameter_locator = crate::parameter::source_locator::SourceLocator::new(scan, ctx)?;
    let mut entries = Vec::new();
    let mut given_up = Vec::new();
    let mut unreadable_metadata = BTreeSet::new();
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
            let mut given_up = Vec::new();
            let mut unreadable_metadata = BTreeSet::new();
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
                match ctx.decode_parser_scope(children, extent, spec, |fields_output| {
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
                    let id = format!("iges:entity:directory#{sequence}");
                    // Parameter parsing owns the primary capture; retain only
                    // this Directory parser's attempted native identity.
                    let primary_capture = ctx.read_transaction()?;
                    let parsed = ctx.record(&id, extent, || {
                        let mut failed_fields = BTreeSet::new();
                        let entry = admit_fields_with(
                            *sequence,
                            first.offset,
                            fields(first),
                            fields(second.physical()),
                            global,
                            |_field, name, number| {
                                let (card, field) = if number < 10 {
                                    (first, usize::from(number - 1))
                                } else {
                                    (second.physical(), usize::from(number - 11))
                                };
                                let start = absolute(source, card.offset)?
                                    .checked_add(field * 8)
                                    .ok_or_else(|| {
                                    CodecError::malformed("Directory field offset overflows")
                                })?;
                                let end = start.checked_add(8).ok_or_else(|| {
                                    CodecError::malformed("Directory field end overflows")
                                })?;
                                let field_source = source.child(start, end).ok_or_else(|| {
                                    CodecError::malformed("Directory field exceeds actual source")
                                })?;
                                let value = metadata::admit(
                                    ctx,
                                    field_source,
                                    name,
                                    number,
                                    global,
                                    fields_output,
                                )?;
                                if value.is_none() {
                                    failed_fields.insert((
                                        *sequence,
                                        if number == 19 { "subscript" } else { name },
                                    ));
                                }
                                Ok::<_, CodecError>(value)
                            },
                        )?;
                        parameter_locator.admit(&entry)?;
                        Ok((entry, failed_fields))
                    });
                    drop(primary_capture);
                    parsed
                })? {
                    Some((entry, failed_fields)) => {
                        append_parser_item(ctx, &mut entries, entry)?;
                        unreadable_metadata.extend(failed_fields);
                    }
                    None => append_given_up(ctx, &mut given_up, pair)?,
                }
            }
            Ok((entries, given_up, unreadable_metadata))
        })?;
        match parsed {
            Some((parsed, parsed_given_up, parsed_metadata)) => {
                unreadable_metadata.extend(parsed_metadata);
                for entry in parsed {
                    append_parser_item(ctx, &mut entries, entry)?;
                }
                for sequence in parsed_given_up {
                    append_parser_item(ctx, &mut given_up, sequence)?;
                }
            }
            // The section part covers every entry in it.
            None => {
                for pair in lines.chunks(2) {
                    append_given_up(ctx, &mut given_up, pair)?;
                }
            }
        }
    }
    Ok(DirectoryParts {
        entries,
        given_up,
        unreadable_metadata,
    })
}

/// The Directory Entries of a Directory parse and the given-up ones.
#[derive(Debug)]
pub(crate) struct DirectoryParts {
    pub(crate) entries: Vec<DirectoryEntry>,
    /// Optional fields whose failures already have a shared part home.
    pub(crate) unreadable_metadata: BTreeSet<(u32, &'static str)>,
    /// First-card sequence numbers of the given-up Directory Entries. A
    /// Parameter Data card whose back-pointer names one depends on that part.
    pub(crate) given_up: Vec<u32>,
}

fn append_given_up(
    ctx: &DecodeContext<'_>,
    given_up: &mut Vec<u32>,
    pair: &[ScannedLine],
) -> Result<(), CodecError> {
    match pair.first() {
        Some(ScannedLine::Card { sequence, .. }) => append_parser_item(ctx, given_up, *sequence),
        _ => Ok(()),
    }
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

/// Push parser working storage. It is not retained bytes
/// (`docs/decode-resource-caps.md`, "Retained bytes"); the entity charge per
/// Directory Entry and the input length bound it.
fn append_parser_item<T>(
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
            "IGES Directory parser storage",
        )
    })?;
    out.push(value);
    Ok(())
}

#[cfg(test)]
mod tests;
