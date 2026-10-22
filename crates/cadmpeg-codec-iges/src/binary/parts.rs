// SPDX-License-Identifier: Apache-2.0
//! Counted Binary entity parsing with original-source failure ownership.

use std::collections::{BTreeMap, BTreeSet};

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
    ctx.capture_part_extent(source)?;
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
            let (parameters, failed) = read_parameters(
                source,
                sections,
                &directory,
                &directory_by_offset,
                ctx,
                &mut output,
                &dependencies,
                global,
            )?;
            directory.retain(|record| {
                !u32::try_from(record.parameter_pointer())
                    .is_ok_and(|offset| failed.contains(&offset))
            });
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
) -> Result<(Vec<BinaryParameter>, BTreeSet<u32>), CodecError> {
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
    let mut failed = BTreeSet::new();
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
                        let mut record =
                            finish_parameter_record(offset, prefix?, directory_by_offset)?;
                        let pointer =
                            positive_pointer(record.directory_pointer, "Parameter Directory")?;
                        let index = directory_by_offset
                            .get(&pointer)
                            .ok_or_else(|| malformed("Binary Parameter owner is absent"))?;
                        let owner = &directory[*index];
                        if positive_pointer(owner.parameter_pointer(), "Directory Parameter Data")?
                            != offset
                        {
                            return Err(malformed(
                                "Binary Parameter and Directory offsets do not pair",
                            ));
                        }
                        if owner.entity_type() != record.entity_type {
                            return Err(malformed(
                                "Binary Directory and Parameter entity types disagree",
                            ));
                        }
                        dependencies.parameter(&record, owner)?;
                        if !admission::allows_handoff(&record, owner, dependencies) {
                            return Ok(record);
                        }
                        ctx.record(
                            &format!("iges:entity:directory#{}", owner.source_sequence),
                            extent,
                            || {
                                group::prune_failed_members(
                                    &mut record,
                                    owner,
                                    directory,
                                    directory_by_offset,
                                    dependencies,
                                )?;
                                let lines = super::parameter_lines(
                                    &mut record,
                                    directory,
                                    directory_by_offset,
                                )?;
                                let content = crate::parameter::admitted::parse_lines(
                                    owner.source_sequence,
                                    record.entity_type,
                                    &lines,
                                    global,
                                    ctx,
                                )?;
                                record.admitted =
                                    Some(super::AdmittedBinaryParameter { lines, content });
                                Ok(record)
                            },
                        )
                    },
                )?;
                match record {
                    Some(record) => append(ctx, &mut records, record)?,
                    None => {
                        insert_failed(ctx, &mut failed, offset)?;
                    }
                }
                cursor = extent
                    .end()
                    .checked_sub(payload.start())
                    .ok_or_else(|| malformed("Binary Parameter cursor underflows"))?;
            }
            Ok(records)
        },
    )?;
    match result {
        Some(records) => Ok((records, failed)),
        None => {
            failed.clear();
            for owner in directory {
                if let Ok(offset) = u32::try_from(owner.parameter_pointer()) {
                    if offset > 0 && u64::from(offset) <= u64_from_index(payload.window().len()) {
                        insert_failed(ctx, &mut failed, offset)?;
                    }
                }
            }
            Ok((Vec::new(), failed))
        }
    }
}

fn insert_failed(
    ctx: &DecodeContext<'_>,
    failed: &mut BTreeSet<u32>,
    offset: u32,
) -> Result<(), CodecError> {
    if !failed.contains(&offset) {
        ctx.retain_bytes(
            u64_from_index(size_of::<u32>()),
            "IGES Binary failed Parameter target",
        )?;
        failed.insert(offset);
    }
    Ok(())
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
