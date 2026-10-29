// SPDX-License-Identifier: Apache-2.0
//! Bulk frames whose governing metadata type table was not fully admitted.

use super::{BulkEnvelope, SegmentMeta};
use crate::decode::part_spec;
use crate::records::{RseRecordFrame, RseRecordTable};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

/// Frame the bulk records of a segment whose type table was not fully
/// admitted. A record whose descriptor was given up is not formed; a record
/// that cannot be typed for another reason is given up.
pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    bulk: &BulkEnvelope<'a>,
    meta: &SegmentMeta<'a>,
    token: &str,
    version: Option<u8>,
) -> Result<Option<(RseRecordTable<'a>, usize)>, CodecError> {
    let stream = || format!("RSeStorage/B{token}");
    let expanded = || crate::decode::expanded_bulk_stream(token);
    let staged = ctx.decode_part(
        output,
        bulk.source,
        part_spec("cfb.stream", bulk.source, stream)?,
        |stream_stage| {
            let header = bulk
                .source
                .child(bulk.source.start(), bulk.compressed.start())
                .ok_or_else(|| {
                    CodecError::malformed("bulk header does not end at its actual member")
                })?;
            ctx.decode_part(
                stream_stage,
                header,
                part_spec("inventor.bulk_header", header, stream)?,
                |_| {
                    let mut view = header;
                    let prefix = crate::reader::array::<16>(&mut view, "bulk envelope prefix")?;
                    let form = crate::reader::u16(&mut view, "bulk envelope form")?;
                    if prefix != bulk.prefix || form != bulk.form.value() {
                        return Err(CodecError::malformed("bulk framing differs from its actual source header"));
                    }
                    Ok(())
                },
            )?;
            ctx.decode_part(
                stream_stage,
                bulk.compressed,
                part_spec("inventor.bulk_body", bulk.compressed, stream)?,
                |body| {
                    let version = version.ok_or_else(|| {
                        CodecError::malformed("bulk framing requires the actual registry version")
                    })?;
                    let admitted = meta.source_types.as_deref().ok_or_else(|| CodecError::malformed("metadata has no source-admitted descriptor facts"))?;
                    let mut records = Vec::new();
                    if admitted.iter().any(Option::is_some) {
                        let capacity = meta.tables.blocks.len();
                        let bytes = cadmpeg_core::decode::u64_from_index(capacity).checked_mul(cadmpeg_core::decode::u64_from_index(size_of::<RseRecordFrame>())).ok_or_else(|| CodecError::malformed("source-framed record storage overflows"))?;
                        records.try_reserve_exact(capacity).map_err(|_| cadmpeg_core::decode::ResourceLimit::allocation_failed(cadmpeg_core::decode::ResourceDimension::RetainedBytes, ctx.policy().limits.max_retained_bytes, bytes, "allocate Inventor source-framed records"))?;
                    }
                    let framing = crate::records::frame_bulk_sources(
                        bulk.expanded, &meta.tables, version,
                        |selector, block, source| {
                            let extent = source.body;
                            let trailer = source.trailer;
                            let index = usize::from(selector.to_le_bytes()[0]);
                            match admitted.get(index).copied() {
                                Some(Some(descriptor)) => match source.into_typed(block, selector, descriptor) {
                                    Ok(record) => records.push(record),
                                    Err(error) => {
                                        ctx.decode_part(body, extent, part_spec("inventor.rse_record_body", extent, expanded)?, |_| Err::<(), _>(error))?;
                                    }
                                },
                                Some(None) => {}
                                None => {
                                    ctx.decode_part(body, extent, part_spec("inventor.rse_record_body", extent, expanded)?, |_| Err::<(), _>(CodecError::malformed("RSe record selector names no metadata type descriptor")))?;
                                }
                            }
                            if !trailer.window().is_empty() {
                                ctx.decode_part(body, trailer, part_spec("inventor.rse_record_trailer", trailer, expanded)?, |_| Ok(()))?;
                            }
                            Ok(())
                        },
                    )?;
                    ctx.decode_part(
                        body,
                        framing.stream_trailer,
                        part_spec("inventor.bulk_stream_trailer", framing.stream_trailer, expanded)?,
                        |_| Ok(()),
                    )?;
                    Ok((RseRecordTable { records, stream_trailer: framing.stream_trailer }, framing.record_count))
                },
            )
        },
    )?
    .flatten();
    Ok(staged)
}

#[cfg(test)]
mod tests;
