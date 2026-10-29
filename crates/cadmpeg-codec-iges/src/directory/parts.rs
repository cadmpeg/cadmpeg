// SPDX-License-Identifier: Apache-2.0
//! Source-bounded Directory parsing through the shared parser part runner.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

use super::{fields, DirectoryDefect, DirectoryEntry, GlobalTable};
use crate::card::{CardScan, PhysicalLine, ScannedLine, Section};

mod metadata;

pub(crate) fn parse(
    scan: &CardScan<'_>,
    global: GlobalTable,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
) -> Result<DirectoryParts, CodecError> {
    let source = scan
        .source_view()
        .ok_or_else(|| CodecError::malformed("Directory source view is absent"))?;
    let mut entries = Vec::new();
    let mut given_up = Vec::new();
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
        let spec = crate::reader::part("iges.directory_section");
        let parsed = ctx.decode_part(output, extent, spec, |children| {
            let mut entries = Vec::new();
            let mut given_up = Vec::new();
            for pair in lines.chunks(2) {
                ctx.charge_entities(1, "iges_directory_entries")?;
                let extent = source_extent(source, pair)?;
                let spec = crate::reader::part("iges.directory_entry");
                match ctx.decode_part(children, extent, spec, |fields_output| {
                    let ScannedLine::Card {
                        sequence,
                        line: first,
                        ..
                    } = &pair[0]
                    else {
                        return Err(CodecError::malformed("Directory card is absent"));
                    };
                    // The required fields decide whether the record forms. A
                    // given-up required field leaves no record to link to.
                    let required = admit_fields(
                        ctx,
                        source,
                        first,
                        pair.get(1).map(ScannedLine::physical),
                        global,
                        fields_output,
                        Fields::Required,
                    )?;
                    if pair.get(1).is_none() {
                        return Err(DirectoryDefect::UnpairedCard.into());
                    }
                    // A given-up required field: the record does not form, so
                    // it has no optional fields.
                    if (0..18).any(|slot| {
                        Fields::Required.holds(slot / 9 * 10 + slot % 9 + 1)
                            && required[slot].is_none()
                    }) {
                        return Ok(None);
                    }
                    let optional = admit_fields(
                        ctx,
                        source,
                        first,
                        pair.get(1).map(ScannedLine::physical),
                        global,
                        fields_output,
                        Fields::Optional,
                    )?;
                    form_entry(*sequence, first, &required, &optional, global)
                })? {
                    Some(Some(entry)) => append_parser_item(ctx, &mut entries, entry)?,
                    // A given-up entry, or an entry whose required field part
                    // is given up: no record is formed (#215 rule 1).
                    Some(None) | None => append_given_up(ctx, &mut given_up, pair)?,
                }
            }
            Ok((entries, given_up))
        })?;
        match parsed {
            Some((parsed, parsed_given_up)) => {
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
    Ok(DirectoryParts { entries, given_up })
}

/// Which Directory fields one pass admits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fields {
    /// The fields the record needs: 1-3, 7, 9, 11 and 14-18.
    Required,
    /// The independently interpretable metadata fields: 4-6, 8, 12, 13
    /// and 19.
    Optional,
}

impl Fields {
    const fn holds(self, number: usize) -> bool {
        let optional = matches!(number, 4 | 5 | 6 | 8 | 12 | 13 | 19);
        match self {
            Self::Required => !optional,
            Self::Optional => optional,
        }
    }
}

/// Admit one pass of Directory fields; each field is one
/// `iges.directory_field` part. The values are indexed by field slot
/// (card times nine plus column).
fn admit_fields(
    ctx: &DecodeContext<'_>,
    source: View<'_>,
    first: &PhysicalLine,
    second: Option<&PhysicalLine>,
    global: GlobalTable,
    output: &mut PartOutput,
    pass: Fields,
) -> Result<[Option<metadata::FieldValue>; 18], CodecError> {
    let mut values = [None; 18];
    for (card, line) in std::iter::once(first).chain(second).enumerate() {
        let start = absolute(source, line.offset)?;
        for field in 0..9 {
            let number = card * 10 + field + 1;
            if !pass.holds(number) {
                continue;
            }
            let begin = start
                .checked_add(field * 8)
                .ok_or_else(|| CodecError::malformed("Directory field offset overflows"))?;
            let end = begin
                .checked_add(8)
                .ok_or_else(|| CodecError::malformed("Directory field end overflows"))?;
            let view = source
                .child(begin, end)
                .ok_or_else(|| CodecError::malformed("Directory field exceeds actual source"))?;
            let number = u8::try_from(number)
                .map_err(|_| CodecError::malformed("Directory field number overflows"))?;
            values[card * 9 + field] = metadata::admit_field(ctx, view, number, global, output)?;
        }
    }
    Ok(values)
}

/// Form one Directory Entry record from its admitted fields. A required
/// field part that is given up leaves the record unformed, and the entry
/// part stays decoded (#215 rule 1). An unreadable optional field has no
/// value.
fn form_entry(
    sequence: u32,
    first: &PhysicalLine,
    required: &[Option<metadata::FieldValue>; 18],
    optional: &[Option<metadata::FieldValue>; 18],
    global: GlobalTable,
) -> Result<Option<DirectoryEntry>, CodecError> {
    let integer = |values: &[Option<metadata::FieldValue>; 18], slot: usize| match values[slot] {
        Some(metadata::FieldValue::Integer(value)) => Some(value),
        _ => None,
    };
    let raw = |slot: usize| match required[slot] {
        Some(metadata::FieldValue::Raw(bytes)) => Some(bytes),
        _ => None,
    };
    let status = match required[8] {
        Some(metadata::FieldValue::Status(status)) => Some(status),
        _ => None,
    };
    let (
        Some(entity_type),
        Some(parameter_start),
        Some(structure),
        Some(transform),
        Some(status),
        Some(repeated_type),
        Some(parameter_line_count),
        Some(form),
        Some(reserved_first),
        Some(reserved_second),
        Some(label),
    ) = (
        integer(required, 0),
        integer(required, 1),
        integer(required, 2),
        integer(required, 6),
        status,
        integer(required, 9),
        integer(required, 12),
        integer(required, 13),
        raw(14),
        raw(15),
        raw(16),
    )
    else {
        return Ok(None);
    };
    if entity_type != repeated_type {
        return Err(DirectoryDefect::RepeatedEntityTypeMismatch {
            declared: entity_type,
            repeated: repeated_type,
        }
        .into());
    }
    let status_field = fields(first)[8];
    Ok(Some(DirectoryEntry {
        source_offset: first.offset,
        sequence,
        entity_type,
        parameter_start,
        structure,
        line_font: integer(optional, 3),
        level: integer(optional, 4),
        view: integer(optional, 5),
        transform,
        label_display: integer(optional, 7),
        status,
        status_padding_recovered: !matches!(
            global,
            GlobalTable::Legacy | GlobalTable::V4_0 | GlobalTable::V5_0
        ) && status_field[0] == b' '
            && status_field.iter().any(u8::is_ascii_digit),
        line_weight: integer(optional, 10),
        color: integer(optional, 11),
        parameter_line_count,
        form,
        reserved: [reserved_first, reserved_second],
        label,
        subscript: integer(optional, 17),
    }))
}

/// The Directory Entries of a Directory parse and the given-up ones.
#[derive(Debug)]
pub(crate) struct DirectoryParts {
    pub(crate) entries: Vec<DirectoryEntry>,
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
