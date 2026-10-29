// SPDX-License-Identifier: Apache-2.0
//! Counted Binary entity parsing with original-source failure ownership.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, ResourceDimension, ResourceLimit, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

use super::{
    counted_record, finish_parameter_record, malformed, normalize_with_records, parameter_prefix,
    parse_directory_record, positive_pointer, read_group, source_sequence, value_groups,
    BinaryDirectory, BinaryIdentities, BinaryParameter, BinarySections, BinaryValue,
    NormalizedBinary, PrimitiveLengths, SECTION_HEADER_WIDTH,
};

/// The Global fields of a Binary file (IGES 5.3 Appendix H.2.3).
const GLOBAL_FIELD_COUNT: usize = 24;

mod admission;
mod dependencies;
mod group;

use dependencies::Dependencies;

pub(crate) fn normalize(
    source: View<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(NormalizedBinary, PartOutput), CodecError> {
    let mut output = PartOutput::new(cadmpeg_ir::CodecFormat::Iges);
    let bytes = normalize_with_records(
        source,
        ctx,
        &mut output,
        |sections, global, output| {
            let dependencies = Dependencies::inspect(sections, global, ctx)?;
            let mut directory = read_directory(source, sections, ctx, output, &dependencies)?;
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
                output,
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

/// Read the Global values. The G section is one `iges.binary_section` part
/// and each value group is one `iges.binary_global_value` part. A group whose
/// values do not parse gives up the rest of the payload: its fields and every
/// later field are absent. Bytes after the last field give up alone.
pub(super) fn read_global(
    source: View<'_>,
    sections: &BinarySections<'_>,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
) -> Result<Vec<BinaryValue>, CodecError> {
    let payload = sections.global;
    let start = payload
        .start()
        .checked_sub(SECTION_HEADER_WIDTH)
        .ok_or_else(|| malformed("Binary Global header offset underflows"))?;
    let end = sections
        .directory
        .start()
        .checked_sub(SECTION_HEADER_WIDTH)
        .ok_or_else(|| malformed("Binary Directory header offset underflows"))?;
    let extent = source
        .child(start, end)
        .ok_or_else(|| malformed("Binary Global section exceeds its source"))?;
    let absent = || {
        ctx.alloc_filled(
            GLOBAL_FIELD_COUNT,
            BinaryValue::Default,
            "IGES Binary Global values",
        )
    };
    let values = ctx.decode_part(
        output,
        extent,
        crate::reader::part("iges.binary_section"),
        |children| {
            let mut values = absent()?;
            let mut field = 0;
            let child = |range: std::ops::Range<usize>| {
                payload
                    .start()
                    .checked_add(range.start)
                    .zip(payload.start().checked_add(range.end))
                    .and_then(|(start, end)| payload.child(start, end))
                    .ok_or_else(|| malformed("Binary Global value exceeds its section"))
            };
            let failed = value_groups(payload.window(), sections.lengths, |range| {
                let group = child(range.clone())?;
                let parsed = ctx.decode_part(
                    children,
                    group,
                    crate::reader::part("iges.binary_global_value"),
                    |_| {
                        let group = read_group(&payload.window()[range.clone()], sections.lengths)?;
                        if group.len() > GLOBAL_FIELD_COUNT.checked_sub(field).unwrap_or_default() {
                            return Err(malformed(
                                "a Binary Global value group exceeds the Global fields",
                            ));
                        }
                        Ok(group)
                    },
                )?;
                let count = parsed.as_ref().map_or(1, Vec::len);
                for (slot, value) in values
                    .iter_mut()
                    .skip(field)
                    .zip(parsed.into_iter().flatten())
                {
                    *slot = value;
                }
                field = field
                    .checked_add(count)
                    .ok_or_else(|| malformed("Binary Global field ordinal overflows"))?;
                Ok(())
            })?;
            if let Some((offset, error)) = failed {
                let rest = child(offset..payload.window().len())?;
                ctx.decode_part(
                    children,
                    rest,
                    crate::reader::part("iges.binary_global_value"),
                    |_| -> Result<(), CodecError> { Err(error) },
                )?;
            }
            Ok(values)
        },
    )?;
    match values {
        Some(values) => Ok(values),
        None => absent(),
    }
}

fn read_directory(
    source: View<'_>,
    sections: &BinarySections<'_>,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
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
    let result = ctx.decode_part(
        output,
        extent,
        crate::reader::part("iges.binary_section"),
        |children| read_records(payload, sections.lengths, ctx, children, dependencies),
    )?;
    Ok(result.unwrap_or_default())
}

fn read_records(
    payload: View<'_>,
    lengths: PrimitiveLengths,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
    dependencies: &Dependencies,
) -> Result<Vec<BinaryDirectory>, CodecError> {
    let mut records = Vec::new();
    let mut cursor = 0;
    let mut ordinal = 0;
    while cursor < payload.window().len() {
        let (offset, extent, body) = counted_record(payload, cursor, "Directory")?;
        let sequence = source_sequence(ordinal)?;
        ordinal = ordinal
            .checked_add(1)
            .ok_or_else(|| malformed("Binary Directory ordinal overflows"))?;
        if let Some(record) = ctx.decode_part(
            output,
            extent,
            crate::reader::part("iges.binary_directory_record"),
            |values| {
                let mut record = parse_directory_record(offset, sequence, body, lengths)?;
                record.admit_directory_fields()?;
                dependencies.directory(&record)?;
                admit_optional_fields(ctx, body, &mut record, values)?;
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

/// Each optional Directory field is one `iges.binary_directory_value` part.
/// A field that is not an integer is given up alone; its record stays.
fn admit_optional_fields(
    ctx: &DecodeContext<'_>,
    body: View<'_>,
    record: &mut BinaryDirectory,
    output: &mut PartOutput,
) -> Result<(), CodecError> {
    let mut previous = None;
    for (slot, name) in super::OPTIONAL_DIRECTORY_FIELDS {
        let range = record.value_extents[slot.slot()].clone();
        // A Default value has no bytes; a shared repeated value has one part,
        // at its first field.
        if range.is_empty() || previous.as_ref() == Some(&range) {
            continue;
        }
        previous = Some(range.clone());
        let value = body
            .start()
            .checked_add(range.start)
            .zip(body.start().checked_add(range.end))
            .and_then(|(start, end)| body.child(start, end))
            .ok_or_else(|| malformed("Binary Directory value exceeds its record"))?;
        if ctx
            .decode_part(
                output,
                value,
                crate::reader::part("iges.binary_directory_value"),
                |_| record.admit_optional_field(slot, name),
            )?
            .is_none()
        {
            record.give_up_optional_field(slot);
        }
    }
    Ok(())
}

fn read_parameters(
    source: View<'_>,
    sections: &BinarySections<'_>,
    directory: &[BinaryDirectory],
    directory_by_offset: &BTreeMap<u32, usize>,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
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
    let result = ctx.decode_part(
        output,
        extent,
        crate::reader::part("iges.binary_section"),
        |children| {
            let mut records = Vec::new();
            let mut cursor = 0;
            while cursor < payload.window().len() {
                let (offset, extent, body) = counted_record(payload, cursor, "Parameter")?;
                let prefix = parameter_prefix(body, sections.lengths);
                let record = ctx.decode_part(
                    children,
                    extent,
                    crate::reader::part("iges.binary_parameter_record"),
                    |values| {
                        let record = finish_parameter_record(offset, prefix?, directory_by_offset)?;
                        let pointer =
                            positive_pointer(record.directory_pointer, "Parameter Directory")?;
                        let index = directory_by_offset
                            .get(&pointer)
                            .ok_or_else(|| malformed("Binary Parameter owner is absent"))?;
                        let owner = &directory[*index];
                        check_owner(&record, owner, offset, dependencies)?;
                        let lexing = Lexing {
                            directory,
                            directory_by_offset,
                            dependencies,
                            global,
                            body,
                        };
                        if !admission::allows_handoff(&record, owner, dependencies) {
                            // An unresolved reference gives up this record
                            // (#215 §4.5; #222). Its values are lexed here, as
                            // an admitted record's are; no record is assembled
                            // again from its normalized cards.
                            return lex_values(record, owner, &lexing, ctx, values);
                        }
                        handoff(record, owner, &lexing, ctx, values)
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

/// The decode state that lexing one Binary Parameter record reads.
struct Lexing<'a, 'source> {
    directory: &'a [BinaryDirectory],
    directory_by_offset: &'a BTreeMap<u32, usize>,
    dependencies: &'a Dependencies,
    global: &'a crate::global::ResolvedGlobal,
    /// The record body: the bytes after its 4-byte count.
    body: View<'source>,
}

fn handoff(
    mut record: BinaryParameter,
    owner: &BinaryDirectory,
    lexing: &Lexing<'_, '_>,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
) -> Result<BinaryParameter, CodecError> {
    group::keep_failed_members(
        &mut record,
        owner,
        lexing.directory,
        lexing.directory_by_offset,
        lexing.dependencies,
    )?;
    lex_values(record, owner, lexing, ctx, output)
}

/// Render and lex one Parameter record, and read each value group after its
/// Directory pointer as one `iges.binary_parameter_value` part.
///
/// A group whose values do not parse, or whose rendered literal is not
/// admitted, gives up only its part. Each slot of a given-up group is `null`;
/// the record and its other values stay (#215 rule 1; #222).
fn lex_values(
    mut record: BinaryParameter,
    owner: &BinaryDirectory,
    lexing: &Lexing<'_, '_>,
    ctx: &DecodeContext<'_>,
    output: &mut PartOutput,
) -> Result<BinaryParameter, CodecError> {
    let lines = super::parameter_lines(&mut record, lexing.directory, lexing.directory_by_offset)?;
    let (mut content, failed) = crate::parameter::admitted::lex_lines(
        owner.source_sequence,
        record.entity_type,
        &lines,
        lexing.global,
        ctx,
    )?;
    // Token 0 is the entity type; token `k` renders value `k - 1`.
    for field in failed {
        let group = field
            .index
            .checked_sub(1)
            .and_then(|value| {
                record
                    .groups
                    .iter_mut()
                    .find(|group| group.values.contains(&value))
            })
            .ok_or_else(|| {
                malformed(format!(
                    "Parameter field {} is outside every value group: {}",
                    field.index,
                    field.defect.describe()
                ))
            })?;
        if group.failure.is_none() {
            group.failure = Some(malformed(format!(
                "Parameter value {}: {}",
                field.index,
                field.defect.describe()
            )));
        }
    }
    for group in &mut record.groups {
        let start = lexing.body.start().checked_add(group.bytes.start);
        let end = lexing.body.start().checked_add(group.bytes.end);
        let extent = start
            .zip(end)
            .and_then(|(start, end)| lexing.body.child(start, end))
            .ok_or_else(|| malformed("Binary Parameter value exceeds its record"))?;
        let failure = group.failure.take();
        let kept = ctx.decode_part(
            output,
            extent,
            crate::reader::part("iges.binary_parameter_value"),
            |_| match failure {
                Some(error) => Err(error),
                None => Ok(()),
            },
        )?;
        if kept.is_none() && record.entity_type != 306 {
            let first = group
                .values
                .start
                .checked_add(1)
                .ok_or_else(|| malformed("Binary Parameter field ordinal overflows"))?;
            let last = group
                .values
                .end
                .checked_add(1)
                .ok_or_else(|| malformed("Binary Parameter field ordinal overflows"))?;
            content.give_up_fields(first..last);
        }
    }
    record.admitted = Some(super::AdmittedBinaryParameter { lines, content });
    Ok(record)
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
