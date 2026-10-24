// SPDX-License-Identifier: Apache-2.0
//! Compressed ASCII admission over genuine independently bounded Data records.

use std::ops::Range;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};

use super::{malformed, CARD_DATA_WIDTH, CARD_WIDTH, PARAMETER_DATA_WIDTH};
use crate::directory::DirectoryEntry;
use crate::global::ResolvedGlobal;

mod source;

pub(crate) struct Admission {
    pub(crate) global: ResolvedGlobal,
    pub(crate) losses: Vec<LossNote>,
    pub(crate) directory: Vec<DirectoryEntry>,
    pub(crate) parameters: crate::parameter::ParameterAssembly,
}

pub(crate) struct Normalized {
    pub(crate) bytes: Vec<u8>,
    pub(crate) parts: ParserPartOutput,
    pub(crate) admission: Admission,
}

pub(crate) fn normalize(
    source: View<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Normalized, CodecError> {
    ctx.capture_part_extent(source)?;
    let lines = source::lines(source, ctx)?;
    let mut payloads = Vec::new();
    for line in &lines {
        append(ctx, &mut payloads, line.payload.window())?;
    }
    let flag = payloads
        .first()
        .ok_or_else(|| malformed("flag record is missing"))?;
    if flag.len() != CARD_WIDTH || flag.get(72) != Some(&b'C') {
        return Err(malformed(
            "Compressed ASCII flag is not an80-column C record",
        ));
    }
    let mut cursor = 1;
    let start = cursor;
    while payloads
        .get(cursor)
        .is_some_and(|line| line.len() == CARD_WIDTH && line[72] == b'S')
    {
        cursor += 1;
    }
    if cursor == start {
        return Err(malformed("Start section is missing after the flag record"));
    }
    let global_begin = cursor;
    while payloads
        .get(cursor)
        .is_some_and(|line| line.len() == CARD_WIDTH && line[72] == b'G')
    {
        cursor += 1;
    }
    if cursor == global_begin {
        return Err(malformed("Global section is missing"));
    }
    let data_begin = cursor;
    let terminate = payloads
        .iter()
        .enumerate()
        .skip(data_begin)
        .find_map(|(at, line)| (line.len() == CARD_WIDTH && line[72] == b'T').then_some(at))
        .ok_or_else(|| malformed("Terminate section is missing"))?;
    let (parameter_delimiter, record_delimiter) =
        super::compressed_delimiters(&payloads[global_begin..data_begin])?;
    let mut global_views = Vec::new();
    for line in &lines[global_begin..data_begin] {
        append(
            ctx,
            &mut global_views,
            line.payload
                .child(line.payload.start(), line.payload.start() + CARD_DATA_WIDTH)
                .ok_or_else(|| malformed("Global payload exceeds source"))?,
        )?;
    }
    let global_stream = ctx.concat_named_views(
        &global_views,
        &format!(
            "iges/compressed/global/{}/{}",
            ctx.capture_part_extent(source)?.stream(),
            source.start()
        ),
    )?;
    let (global, losses) = crate::global::parse_stream(global_stream.window())?;
    let mut parts = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let mut decoded = Vec::new();
    if data_begin < terminate {
        let extent = source
            .child(
                lines[data_begin].extent.start(),
                lines[terminate].extent.start(),
            )
            .ok_or_else(|| malformed("Data section exceeds original source"))?;
        let section_framing = line_endings(ctx, &lines[data_begin..terminate])?;
        if let Some(records) = ctx.decode_parser_scope(
            &mut parts,
            extent,
            spec("iges.compressed_data_section", section_framing)?,
            |children| {
                let mut records = Vec::new();
                let mut previous = None;
                let mut at = data_begin;
                let mut expected = 1u32;
                let mut parameter_start = 1u32;
                while at < terminate {
                    let parsed = super::parse_directory_record(
                        &payloads[..terminate],
                        at,
                        record_delimiter,
                    )?;
                    if parsed.sequence != expected || parsed.sequence % 2 == 0 {
                        return Err(malformed(
                            "Data record sequence is not the next original odd sequence",
                        ));
                    }
                    expected = expected
                        .checked_add(2)
                        .ok_or_else(|| malformed("Directory sequence overflows"))?;
                    let mut specifier_framing = Vec::new();
                    let fields = source::Fields::apply(
                        previous,
                        &lines,
                        at,
                        &parsed,
                        record_delimiter,
                        ctx,
                        &mut specifier_framing,
                    )?;
                    let count = fields.count()?;
                    let end = parsed
                        .next
                        .checked_add(count)
                        .filter(|end| *end <= terminate)
                        .ok_or_else(|| {
                            malformed("Parameter Data lines end before the declared count")
                        })?;
                    let extent = source
                        .child(lines[at].extent.start(), lines[end - 1].extent.end())
                        .ok_or_else(|| malformed("Data record exceeds original source"))?;
                    let mut framing =
                        record_framing(ctx, &lines, at, end, &parsed, record_delimiter)?;
                    for range in specifier_framing {
                        append(ctx, &mut framing, range)?;
                    }
                    framing.sort_unstable_by_key(|range| (range.start, range.end));
                    previous = Some(fields);
                    if let Some(record) = ctx.decode_parser_scope(
                        children,
                        extent,
                        spec("iges.compressed_data_record", framing)?,
                        |specifiers| {
                            admit_optional_specifiers(ctx, &fields, specifiers)?;
                            ctx.record(
                                &format!("iges:entity:directory#{}", parsed.sequence),
                                extent,
                                || {
                                    let syntax_bytes = extent
                                        .window()
                                        .len()
                                        .checked_mul(3)
                                        .and_then(|size| {
                                            size.checked_add(16 * size_of::<std::rc::Rc<Vec<u8>>>())
                                        })
                                        .and_then(|size| {
                                            count
                                                .checked_mul(size_of::<Vec<u8>>())
                                                .and_then(|lines| size.checked_add(lines))
                                        })
                                        .ok_or_else(|| {
                                            malformed("Data callback storage overflows")
                                        })?;
                                    ctx.retain_bytes(
                                        u64_from_index(syntax_bytes),
                                        "IGES Compressed callback syntax",
                                    )?;
                                    let (entity, next) = super::parse_data_entity_with_fields(
                                        &payloads[..terminate],
                                        parsed.sequence,
                                        parsed.next,
                                        fields.values(ctx)?,
                                        parameter_delimiter,
                                        record_delimiter,
                                    )?;
                                    if next != end {
                                        return Err(malformed(
                                            "Data callback boundary differs from source framing",
                                        ));
                                    }
                                    let (first, second) = super::directory_fields(
                                        &entity,
                                        if count == 0 { 0 } else { parameter_start },
                                    )?;
                                    let entry = crate::directory::admit_fields(
                                        entity.sequence,
                                        u64_from_index(extent.start() - source.start()),
                                        first,
                                        second,
                                        global.global_table(),
                                    )
                                    .map_err(|error| malformed(error.to_string()))?;
                                    let parameter = crate::parameter::compressed::parse(
                                        &entry,
                                        &entity.parameter_lines,
                                        &global,
                                        ctx,
                                    )?
                                    .map(|(record, failed)| {
                                        admit_parameter_fields(
                                            ctx,
                                            extent,
                                            &lines[parsed.next..end],
                                            entry.entity_type,
                                            record,
                                            failed,
                                            specifiers,
                                        )
                                    })
                                    .transpose()?;
                                    Ok((entity, entry, parameter))
                                },
                            )
                        },
                    )? {
                        parameter_start = parameter_start
                            .checked_add(
                                u32::try_from(record.0.parameter_lines.len())
                                    .map_err(|_| malformed("Parameter count exceeds u32"))?,
                            )
                            .ok_or_else(|| malformed("Parameter sequence overflows"))?;
                        append(ctx, &mut records, record)?;
                    }
                    at = end;
                }
                Ok(records)
            },
        )? {
            decoded = records;
        }
    }
    let mut card_count = data_begin
        .checked_sub(start)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| malformed("Output header count overflows"))?;
    for (entity, _, _) in &decoded {
        card_count = card_count
            .checked_add(2)
            .and_then(|count| count.checked_add(entity.parameter_lines.len()))
            .ok_or_else(|| malformed("Output card count overflows"))?;
    }
    let mut output_len = card_count
        .checked_mul(CARD_WIDTH + 1)
        .ok_or_else(|| malformed("Output card bytes overflow"))?;
    for line in &lines[terminate + 1..] {
        output_len = output_len
            .checked_add(line.extent.window().len())
            .ok_or_else(|| malformed("Output suffix bytes overflow"))?;
    }
    ctx.retain_bytes(u64_from_index(output_len), "IGES expanded grammar bytes")?;
    let mut bytes = ctx.alloc_filled(output_len, 0u8, "IGES expanded grammar bytes")?;
    bytes.clear();
    for line in &payloads[start..global_begin] {
        super::append_source_card(&mut bytes, line, b'S')?;
    }
    for line in &payloads[global_begin..data_begin] {
        super::append_source_card(&mut bytes, line, b'G')?;
    }
    let mut directory = Vec::new();
    let mut grammar_sequence = 1u32;
    for (entity, entry, _) in &decoded {
        let (first, second) = super::directory_fields(
            entity,
            u32::try_from(entry.parameter_start)
                .map_err(|_| malformed("Parameter start exceeds u32"))?,
        )?;
        super::append_directory_fields(&mut bytes, first, second, grammar_sequence)?;
        grammar_sequence = grammar_sequence
            .checked_add(2)
            .ok_or_else(|| malformed("Directory sequence overflows"))?;
    }
    let mut parameter_sequence = 1u32;
    let mut parameters = Vec::new();
    for (entity, entry, parameter) in decoded {
        super::append_parameter_cards(&mut bytes, &entity, parameter_sequence)?;
        parameter_sequence = parameter_sequence
            .checked_add(
                u32::try_from(entity.parameter_lines.len())
                    .map_err(|_| malformed("Parameter count exceeds u32"))?,
            )
            .ok_or_else(|| malformed("Parameter sequence overflows"))?;
        append(ctx, &mut directory, entry)?;
        if let Some(parameter) = parameter {
            append(ctx, &mut parameters, parameter)?;
        }
    }
    let parameters = crate::parameter::finish_admitted(&directory, parameters, &global);
    let directory_count = directory
        .len()
        .checked_mul(2)
        .ok_or_else(|| malformed("Directory count overflows"))?;
    super::append_terminate(
        &mut bytes,
        global_begin - start,
        data_begin - global_begin,
        directory_count,
        usize::try_from(parameter_sequence - 1)
            .map_err(|_| malformed("Parameter count exceeds usize"))?,
    )?;
    for line in &lines[terminate + 1..] {
        bytes.extend_from_slice(line.extent.window());
    }
    Ok(Normalized {
        bytes,
        parts,
        admission: Admission {
            global,
            losses,
            directory,
            parameters,
        },
    })
}

fn line_endings(
    ctx: &DecodeContext<'_>,
    lines: &[source::Line<'_>],
) -> Result<Vec<Range<u64>>, CodecError> {
    let mut out = Vec::new();
    for line in lines {
        if line.payload.end() < line.extent.end() {
            append(
                ctx,
                &mut out,
                u64_from_index(line.payload.end())..u64_from_index(line.extent.end()),
            )?;
        }
    }
    Ok(out)
}

fn record_framing(
    ctx: &DecodeContext<'_>,
    lines: &[source::Line<'_>],
    start: usize,
    end: usize,
    parsed: &super::ParsedDirectoryRecord,
    delimiter: u8,
) -> Result<Vec<Range<u64>>, CodecError> {
    let mut out = line_endings(ctx, &lines[start..end])?;
    let first = lines[start].payload;
    let (_, sequence_end) = super::parse_sequence(first.window(), 1, "Data record")?;
    append(
        ctx,
        &mut out,
        u64_from_index(first.start())..u64_from_index(first.start() + sequence_end),
    )?;
    for (index, line) in lines.iter().enumerate().take(parsed.next).skip(start) {
        let begin = if index == start { sequence_end } else { 0 };
        if let Some(at) = line.payload.window()[begin..]
            .iter()
            .position(|byte| *byte == delimiter)
        {
            let at = line.payload.start() + begin + at;
            append(ctx, &mut out, u64_from_index(at)..u64_from_index(at + 1))?;
        }
    }
    out.sort_unstable_by_key(|range| (range.start, range.end));
    Ok(out)
}

/// Each optional Directory field that a record states is one
/// `iges.compressed_directory_specifier` part. A value that is not an integer
/// is given up alone; the Directory admission then states no value for it.
fn admit_optional_specifiers(
    ctx: &DecodeContext<'_>,
    fields: &source::Fields<'_>,
    output: &mut ParserPartOutput,
) -> Result<(), CodecError> {
    for (name, specifier, value_begin) in fields.optional_specifiers() {
        let value = specifier
            .child(value_begin, specifier.end())
            .ok_or_else(|| malformed("specifier value exceeds its specifier"))?;
        let framing = vec![u64_from_index(specifier.start())..u64_from_index(value_begin)];
        ctx.decode_parser_part(
            output,
            specifier,
            spec("iges.compressed_directory_specifier", framing)?,
            || {
                let text = value
                    .read_slice(value.start()..value.end())
                    .ok_or_else(|| malformed("specifier value is unreadable"))?;
                let field = crate::directory::render_field(text)?;
                crate::directory::integer(field, name).map_err(CodecError::from)?;
                Ok(())
            },
        )?;
    }
    Ok(())
}

/// Each Parameter Data field after the entity type is one
/// `iges.compressed_parameter_field` part. A field whose literal is not
/// admitted gives up only its part; its slot is absent.
fn admit_parameter_fields(
    ctx: &DecodeContext<'_>,
    record: View<'_>,
    lines: &[source::Line<'_>],
    entity_type: i64,
    parameter: crate::parameter::ParameterRecord,
    failed: Vec<crate::parameter::FailedField>,
    output: &mut ParserPartOutput,
) -> Result<crate::parameter::ParameterRecord, CodecError> {
    if entity_type == 306 {
        return Ok(parameter);
    }
    let mut failed = failed.into_iter().peekable();
    for (index, token) in parameter.tokens().iter().enumerate().skip(1) {
        let failure = failed.next_if(|field| field.index == index);
        let (span, header) = match (token, &failure) {
            (Some(token), _) => (
                token.span.clone(),
                match &token.value {
                    crate::parameter::TokenValue::String(value) => Some(
                        token
                            .span
                            .end
                            .checked_sub(value.as_ref().len())
                            .ok_or_else(|| {
                                malformed("Parameter Hollerith header exceeds its field")
                            })?,
                    ),
                    _ => None,
                },
            ),
            (None, Some(field)) => (field.span.clone(), None),
            (None, None) => continue,
        };
        let start = parameter_source_offset(lines, span.start)?;
        let delimiter = parameter_source_offset(lines, span.end)?;
        let end = delimiter
            .checked_add(1)
            .ok_or_else(|| malformed("Parameter field extent overflows"))?;
        let extent = record
            .child(start, end)
            .ok_or_else(|| malformed("Parameter field exceeds its Data record"))?;
        let mut framing = Vec::new();
        if let Some(header_end) = header {
            append(
                ctx,
                &mut framing,
                u64_from_index(start)..u64_from_index(parameter_source_offset(lines, header_end)?),
            )?;
        }
        for pair in lines.windows(2) {
            if start < pair[0].payload.end() && pair[1].payload.start() < end {
                append(
                    ctx,
                    &mut framing,
                    u64_from_index(pair[0].payload.end())..u64_from_index(pair[1].payload.start()),
                )?;
            }
        }
        append(
            ctx,
            &mut framing,
            u64_from_index(delimiter)..u64_from_index(end),
        )?;
        ctx.decode_parser_part(
            output,
            extent,
            spec("iges.compressed_parameter_field", framing)?,
            || match &failure {
                Some(field) => Err(malformed(format!(
                    "Parameter field {index}: {}",
                    field.defect.describe()
                ))),
                None => Ok(()),
            },
        )?;
    }
    Ok(parameter)
}

/// The source offset of a logical offset in the record's 64-column Parameter
/// stream. Each line holds columns 1-64 of its record stream; the columns
/// after its last byte are padding with no source bytes.
fn parameter_source_offset(
    lines: &[source::Line<'_>],
    logical: usize,
) -> Result<usize, CodecError> {
    let line = lines
        .get(logical / PARAMETER_DATA_WIDTH)
        .ok_or_else(|| malformed("Parameter field exceeds its lines"))?;
    let column = logical % PARAMETER_DATA_WIDTH;
    if column >= line.payload.window().len() {
        return Err(malformed("Parameter field byte is line padding"));
    }
    line.payload
        .start()
        .checked_add(column)
        .ok_or_else(|| malformed("Parameter field offset overflows"))
}

fn spec(name: &str, framing: Vec<Range<u64>>) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(name)
        .ok_or_else(|| malformed("Compressed part level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, framing))
}

fn append<T>(ctx: &DecodeContext<'_>, values: &mut Vec<T>, value: T) -> Result<(), CodecError> {
    let bytes = u64_from_index(size_of::<T>());
    ctx.retain_bytes(bytes, "IGES Compressed source staging")?;
    values.try_reserve(1).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "IGES Compressed source staging",
        )
    })?;
    values.push(value);
    Ok(())
}

#[cfg(test)]
mod tests;
