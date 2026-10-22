// SPDX-License-Identifier: Apache-2.0
//! Physical bulk frames whose governing metadata type table was given up.

use super::{BulkEnvelope, SegmentMeta};
use crate::records::{RseRecordFrame, RseRecordTable};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

fn level(name: &'static str) -> Result<PartLevel, CodecError> {
    PartLevel::parse(name)
        .ok_or_else(|| CodecError::malformed("dependent bulk level absent from Appendix C"))
}

pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    bulk: &BulkEnvelope<'a>,
    meta: &SegmentMeta<'a>,
    version: Option<u8>,
) -> Result<Option<(RseRecordTable<'a>, usize)>, CodecError> {
    ctx.decode_part(
        output,
        bulk.source,
        PartSpec::new(level("cfb.stream")?, Vec::new()),
        |stream| {
            let header = bulk
                .source
                .child(bulk.source.start(), bulk.compressed.start())
                .ok_or_else(|| {
                    CodecError::malformed("bulk header does not end at its actual member")
                })?;
            ctx.decode_part(
                stream,
                header,
                PartSpec::new(level("inventor.bulk_header")?, Vec::new()),
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
                stream,
                bulk.compressed,
                PartSpec::new(level("inventor.bulk_body")?, Vec::new()),
                |body| {
                    let version = version.ok_or_else(|| {
                        CodecError::malformed("bulk framing requires the actual registry version")
                    })?;
                    let admitted = meta.source_types.as_deref().ok_or_else(|| CodecError::malformed("metadata has no source-admitted descriptor facts"))?;
                    let mut records = Vec::new();
                    if admitted.iter().any(Option::is_some) {
                        let capacity = meta.tables.blocks.len();
                        let bytes = cadmpeg_core::decode::u64_from_index(capacity).checked_mul(cadmpeg_core::decode::u64_from_index(size_of::<RseRecordFrame>())).ok_or_else(|| CodecError::malformed("source-framed record storage overflows"))?;
                        ctx.retain_bytes(bytes, "retain Inventor source-framed records")?;
                        records.try_reserve_exact(capacity).map_err(|_| cadmpeg_core::decode::ResourceLimit::allocation_failed(cadmpeg_core::decode::ResourceDimension::RetainedBytes, ctx.policy().limits.max_retained_bytes, bytes, "allocate Inventor source-framed records"))?;
                    }
                    let framing = crate::records::frame_bulk_sources(
                        bulk.expanded, &meta.tables, version,
                        |selector, block, source| {
                            let extent = source.body;
                            let trailer = source.trailer;
                            let index = usize::from(selector.to_le_bytes()[0]);
                            let typed = admitted.get(index).copied().flatten().map(|descriptor| source.into_typed(block, selector, descriptor));
                            match typed {
                                Some(Ok(record)) => records.push(record),
                                Some(Err(error)) => {
                                    ctx.decode_part(body, extent, PartSpec::new(level("inventor.rse_record_body")?, Vec::new()), |_| Err::<(), _>(error))?;
                                }
                                None => {
                                    ctx.decode_part(body, extent, PartSpec::new(level("inventor.rse_record_body")?, Vec::new()), |_| Err::<(), _>(CodecError::malformed("RSe record requires its given-up metadata type table")))?;
                                }
                            }
                            if !trailer.window().is_empty() {
                                ctx.decode_part(body, trailer, PartSpec::new(level("inventor.rse_record_trailer")?, Vec::new()), |_| Ok(()))?;
                            }
                            Ok(())
                        },
                    )?;
                    let trailer = ctx.capture_part_extent(framing.stream_trailer)?;
                    ctx.decode_part(
                        body,
                        framing.stream_trailer,
                        PartSpec::new(
                            level("inventor.bulk_stream_trailer")?,
                            vec![trailer.offset()..trailer.offset() + 4],
                        ),
                        |_| Ok(()),
                    )?;
                    Ok((RseRecordTable { records, stream_trailer: framing.stream_trailer }, framing.record_count))
                },
            )
        },
    )
    .map(|member| member.flatten())
}

#[cfg(test)]
mod tests;
