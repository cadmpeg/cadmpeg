// SPDX-License-Identifier: Apache-2.0
//! Counted Binary entity parsing with original-source failure ownership.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, ParserPartOutput, PartLevel, PartSpec};

use super::{
    counted_record, finish_parameter_record, malformed, normalize_with_records, parameter_prefix,
    parse_directory_record, positive_pointer, source_sequence, BinaryDirectory, BinaryIdentities,
    BinaryParameter, BinarySections, NormalizedBinary, PrimitiveLengths, SECTION_HEADER_WIDTH,
};

mod admission;
mod dependencies;
mod group;

use dependencies::Dependencies;

pub(crate) fn normalize(
    source: View<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(NormalizedBinary, ParserPartOutput), CodecError> {
    ctx.require_source(source)?;
    let mut output = ParserPartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let bytes = normalize_with_records(
        source,
        ctx,
        |sections, global| {
            let dependencies = Dependencies::inspect(sections, global, ctx)?;
            let mut directory = read_directory(source, sections, ctx, &mut output, &dependencies)?;
            let mut directory_by_offset = BTreeMap::new();
            for (index, record) in directory.iter().enumerate() {
                ctx.retain_bytes(
                    u64_from_index(size_of::<(u32, usize)>()),
                    "IGES Binary owner index",
                )?;
                directory_by_offset.insert(record.offset, index);
            }
            let parameters = read_parameters(
                source,
                sections,
                &directory,
                &directory_by_offset,
                ctx,
                &mut output,
                &dependencies,
                global,
            )?;
            detach_absent_parameters(&mut directory, &parameters);
            Ok((directory, parameters))
        },
        |bytes, directory, admission| {
            let retained = directory
                .len()
                .checked_mul(size_of::<u32>())
                .ok_or_else(|| malformed("Binary semantic identity storage overflows"))?;
            ctx.retain_bytes(u64_from_index(retained), "IGES Binary semantic identities")?;
            let mut identities =
                ctx.alloc_filled(directory.len(), 0_u32, "IGES Binary semantic identities")?;
            for (key, record) in identities.iter_mut().zip(directory) {
                *key = record.source_sequence;
            }
            Ok(NormalizedBinary {
                bytes,
                identities: BinaryIdentities(identities),
                admission,
            })
        },
    )?;
    Ok((bytes, output))
}

fn read_directory(
    source: View<'_>,
    sections: &BinarySections<'_>,
    ctx: &DecodeContext<'_>,
    output: &mut ParserPartOutput,
    dependencies: &Dependencies,
) -> Result<Vec<BinaryDirectory>, CodecError> {
    let payload = sections.directory;
    let start = payload
        .start()
        .checked_sub(SECTION_HEADER_WIDTH)
        .ok_or_else(|| malformed("Binary Directory header offset underflows"))?;
    let end = sections
        .parameter
        .start()
        .checked_sub(SECTION_HEADER_WIDTH)
        .ok_or_else(|| malformed("Binary Parameter header offset underflows"))?;
    let extent = source
        .child(start, end)
        .ok_or_else(|| malformed("Binary Directory section exceeds its source"))?;
    let mut framing = Vec::new();
    append(
        ctx,
        &mut framing,
        u64_from_index(start)..u64_from_index(payload.start()),
    )?;
    if payload.end() < end {
        append(
            ctx,
            &mut framing,
            u64_from_index(payload.end())..u64_from_index(end),
        )?;
    }
    let result = ctx.decode_parser_scope(
        output,
        extent,
        spec("iges.binary_section", framing)?,
        |children| read_records(payload, sections.lengths, ctx, children, dependencies),
    )?;
    Ok(result.unwrap_or_default())
}

fn read_records(
    payload: View<'_>,
    lengths: PrimitiveLengths,
    ctx: &DecodeContext<'_>,
    output: &mut ParserPartOutput,
    dependencies: &Dependencies,
) -> Result<Vec<BinaryDirectory>, CodecError> {
    let mut records = Vec::new();
    let mut cursor = 0;
    let mut ordinal = 0;
    while cursor < payload.window().len() {
        let (offset, extent, body) = counted_record(payload, cursor, "Directory")?;
        let mut framing = Vec::new();
        append(
            ctx,
            &mut framing,
            u64_from_index(extent.start())..u64_from_index(body.start()),
        )?;
        let sequence = source_sequence(ordinal)?;
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| malformed("Binary Directory ordinal overflows"))?;
        if let Some(record) = ctx.decode_parser_part(
            output,
            extent,
            spec("iges.binary_directory_record", framing)?,
            || {
                let record = parse_directory_record(offset, sequence, body, lengths)?;
                record.admit_directory_fields()?;
                dependencies.directory(&record)?;
                Ok(record)
            },
        )? {
            append(ctx, &mut records, record)?;
        }
        cursor = extent
            .end()
            .checked_sub(payload.start())
            .ok_or_else(|| malformed("Binary Directory cursor underflows"))?;
    }
    Ok(records)
}

fn read_parameters(
    source: View<'_>,
    sections: &BinarySections<'_>,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
    ctx: &DecodeContext<'_>,
    output: &mut ParserPartOutput,
    dependencies: &Dependencies,
    global: &crate::global::ResolvedGlobal,
) -> Result<Vec<BinaryParameter>, CodecError> {
    let payload = sections.parameter;
    let start = payload
        .start()
        .checked_sub(SECTION_HEADER_WIDTH)
        .ok_or_else(|| malformed("Binary Parameter header offset underflows"))?;
    let end = sections
        .terminate
        .start()
        .checked_sub(SECTION_HEADER_WIDTH)
        .ok_or_else(|| malformed("Binary Terminate header offset underflows"))?;
    let extent = source
        .child(start, end)
        .ok_or_else(|| malformed("Binary Parameter section exceeds its source"))?;
    let mut framing = Vec::new();
    append(
        ctx,
        &mut framing,
        u64_from_index(start)..u64_from_index(payload.start()),
    )?;
    if payload.end() < end {
        append(
            ctx,
            &mut framing,
            u64_from_index(payload.end())..u64_from_index(end),
        )?;
    }
    let result = ctx.decode_parser_scope(
        output,
        extent,
        spec("iges.binary_section", framing)?,
        |children| {
            let mut records = Vec::new();
            let mut cursor = 0;
            while cursor < payload.window().len() {
                let (offset, extent, body) = counted_record(payload, cursor, "Parameter")?;
                let mut framing = Vec::new();
                append(
                    ctx,
                    &mut framing,
                    u64_from_index(extent.start())..u64_from_index(body.start()),
                )?;
                let prefix = parameter_prefix(body, sections.lengths);
                if let Ok(prefix) = &prefix {
                    if !prefix.pointer_extent.is_empty() {
                        let start = body
                            .start()
                            .checked_add(prefix.pointer_extent.start)
                            .ok_or_else(|| malformed("Binary pointer framing start overflows"))?;
                        let end = body
                            .start()
                            .checked_add(prefix.pointer_extent.end)
                            .ok_or_else(|| malformed("Binary pointer framing end overflows"))?;
                        append(
                            ctx,
                            &mut framing,
                            u64_from_index(start)..u64_from_index(end),
                        )?;
                    }
                }
                let record = ctx.decode_parser_part(
                    children,
                    extent,
                    spec("iges.binary_parameter_record", framing)?,
                    || {
                        if dependencies.source_line(offset) || dependencies.source_point(offset) {
                            let navigation = prefix?;
                            let pointer = positive_pointer(
                                navigation.directory_pointer,
                                "Parameter Directory",
                            )?;
                            let owner = directory_by_offset
                                .get(&pointer)
                                .and_then(|index| directory.get(*index))
                                .ok_or_else(|| malformed("Binary Parameter owner is absent"))?;
                            return ctx.record(
                                &format!("iges:entity:directory#{}", owner.source_sequence),
                                extent,
                                || {
                                    let record = finish_parameter_record(
                                        offset,
                                        if dependencies.source_point(offset) {
                                            super::source_point_prefix(body, sections.lengths)?
                                        } else {
                                            super::source_line_prefix(body, sections.lengths)?
                                        },
                                        directory_by_offset,
                                    )?;
                                    check_owner(&record, owner, offset, dependencies)?;
                                    handoff(
                                        record,
                                        owner,
                                        directory,
                                        directory_by_offset,
                                        dependencies,
                                        global,
                                        ctx,
                                    )
                                },
                            );
                        }
                        let record = finish_parameter_record(offset, prefix?, directory_by_offset)?;
                        let pointer =
                            positive_pointer(record.directory_pointer, "Parameter Directory")?;
                        let index = directory_by_offset
                            .get(&pointer)
                            .ok_or_else(|| malformed("Binary Parameter owner is absent"))?;
                        let owner = &directory[*index];
                        check_owner(&record, owner, offset, dependencies)?;
                        if !admission::allows_handoff(&record, owner, dependencies) {
                            // An unresolved reference gives up this record,
                            // the nearest part the decoder runs that holds the
                            // value (#215 §4.5; #222).
                            super::check_parameter_references(
                                &record,
                                directory,
                                directory_by_offset,
                            )?;
                            return Ok(record);
                        }
                        ctx.record(
                            &format!("iges:entity:directory#{}", owner.source_sequence),
                            extent,
                            || {
                                handoff(
                                    record,
                                    owner,
                                    directory,
                                    directory_by_offset,
                                    dependencies,
                                    global,
                                    ctx,
                                )
                            },
                        )
                    },
                )?;
                if let Some(record) = record {
                    append(ctx, &mut records, record)?;
                }
                cursor = extent
                    .end()
                    .checked_sub(payload.start())
                    .ok_or_else(|| malformed("Binary Parameter cursor underflows"))?;
            }
            Ok(records)
        },
    )?;
    Ok(result.unwrap_or_default())
}

/// A Directory record whose Parameter Data record is given up or absent keeps
/// its fields with no parameters, as in Fixed ASCII (#215 rule 1). Its
/// Parameter Data pointer is framing; the normalized image states no
/// Parameter Data for it.
fn detach_absent_parameters(directory: &mut [BinaryDirectory], parameters: &[BinaryParameter]) {
    // Parameter records are read in source order, so their offsets ascend.
    for record in directory {
        if record.parameter_pointer() != 0
            && !u32::try_from(record.parameter_pointer()).is_ok_and(|offset| {
                parameters
                    .binary_search_by_key(&offset, |parameter| parameter.offset)
                    .is_ok()
            })
        {
            record.detach_parameter_data();
        }
    }
}

fn check_owner(
    record: &BinaryParameter,
    owner: &BinaryDirectory,
    offset: u32,
    dependencies: &Dependencies,
) -> Result<(), CodecError> {
    if positive_pointer(owner.parameter_pointer(), "Directory Parameter Data")? != offset {
        return Err(malformed(
            "Binary Parameter and Directory offsets do not pair",
        ));
    }
    if owner.entity_type() != record.entity_type {
        return Err(malformed(
            "Binary Directory and Parameter entity types disagree",
        ));
    }
    dependencies.parameter(record, owner)
}

fn handoff(
    mut record: BinaryParameter,
    owner: &BinaryDirectory,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
    dependencies: &Dependencies,
    global: &crate::global::ResolvedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<BinaryParameter, CodecError> {
    group::prune_failed_members(
        &mut record,
        owner,
        directory,
        directory_by_offset,
        dependencies,
    )?;
    let lines = super::parameter_lines(&mut record, directory, directory_by_offset)?;
    let content = crate::parameter::admitted::parse_lines(
        owner.source_sequence,
        record.entity_type,
        &lines,
        global,
        ctx,
    )?;
    record.admitted = Some(super::AdmittedBinaryParameter { lines, content });
    Ok(record)
}

fn spec(name: &str, framing: Vec<std::ops::Range<u64>>) -> Result<PartSpec, CodecError> {
    let level = PartLevel::parse(name)
        .ok_or_else(|| malformed("Binary part level is absent from Appendix C"))?;
    Ok(PartSpec::new(level, framing))
}

fn append<T>(ctx: &DecodeContext<'_>, items: &mut Vec<T>, value: T) -> Result<(), CodecError> {
    let bytes = u64_from_index(size_of::<T>());
    ctx.retain_bytes(bytes, "IGES Binary parser output")?;
    items.try_reserve(1).map_err(|_| {
        ResourceLimit::allocation_failed(
            ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            bytes,
            "IGES Binary parser output",
        )
    })?;
    items.push(value);
    Ok(())
}

#[cfg(test)]
mod tests;
