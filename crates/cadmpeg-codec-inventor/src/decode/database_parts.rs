// SPDX-License-Identifier: Apache-2.0
//! Source-owned RSe database records.

use crate::native::DatabaseRecord;
use crate::rse::DatabaseDescriptor;
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    descriptor: &DatabaseDescriptor<'a>,
) -> Result<bool, CodecError> {
    let source = descriptor
        .source
        .ok_or_else(|| CodecError::malformed("held database has no source stream"))?;
    ctx.capture_part_extent(source)?;
    let stream_level = PartLevel::parse("cfb.stream")
        .ok_or_else(|| CodecError::malformed("CFB stream is absent from Appendix C"))?;
    Ok(ctx
        .decode_part(
            output,
            source,
            PartSpec::new(stream_level, Vec::new()),
            |stream_stage| {
                let count = cadmpeg_core::decode::View::u32_le_at(source.window(), 52)
                    .ok_or_else(|| CodecError::malformed("RSe database has no note count"))?;
                let end = 56_u64 + 2 * u64::from(count);
                if end != u64_from_index(source.window().len()) {
                    return Err(CodecError::malformed(
                        "RSe database note extent differs from its CFB stream",
                    ));
                }
                let level = PartLevel::parse("inventor.rse_database").ok_or_else(|| {
                    CodecError::malformed("RSe database is absent from Appendix C")
                })?;
                let start = u64_from_index(source.start());
                let id = format!("inventor:rse:database#v{}", descriptor.band.value());
                ctx.decode_part(
                    stream_stage,
                    source,
                    PartSpec::new(level, vec![start + 52..start + 56]),
                    |database_stage| {
                        ctx.record(&id, source, || {
                            let database = crate::database::parse_database_source(ctx, source)?;
                            database_stage.insert_native(
                                ctx,
                                "databases",
                                DatabaseRecord {
                                    id: id.clone(),
                                    band: descriptor.band.value(),
                                    database_id: super::hex(&database.id),
                                    schema: database.schema.value(),
                                    created_by: super::version_record(database.created_by),
                                    created_filetime: database.created_filetime,
                                    saved_by: super::version_record(database.saved_by),
                                    saved_filetime: database.saved_filetime,
                                    note: database.note,
                                },
                            )
                        })
                    },
                )
                .map(|formed| formed.is_some())
            },
        )?
        .unwrap_or(false))
}

#[cfg(test)]
mod tests;
