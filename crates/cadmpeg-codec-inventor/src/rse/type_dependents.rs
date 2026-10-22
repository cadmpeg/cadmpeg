// SPDX-License-Identifier: Apache-2.0
//! Physical bulk frames whose governing metadata type table was given up.

use super::{BulkEnvelope, SegmentMeta};
use crate::records::{RseRecordFrame, RseRecordTable};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

/// Appendix C `inventor.type_descriptor`: entry k is at section +4 + 28k,
/// after the u32 count.
const TYPE_COUNT_LEN: u64 = 4;

fn level(name: &'static str) -> Result<PartLevel, CodecError> {
    PartLevel::parse(name)
        .ok_or_else(|| CodecError::malformed("dependent bulk level absent from Appendix C"))
}

pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    bulk: &BulkEnvelope<'a>,
    meta: &SegmentMeta<'a>,
    token: &str,
    version: Option<u8>,
) -> Result<Option<(RseRecordTable<'a>, usize)>, CodecError> {
    let covering = covering_parts(ctx, output, meta)?;
    let mut withheld = Vec::new();
    let staged = ctx.decode_part(
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
                                None => match covering.get(index).cloned().flatten() {
                                    Some(part) => {
                                        withheld.try_reserve(1).map_err(|_| cadmpeg_core::decode::ResourceLimit::allocation_failed(cadmpeg_core::decode::ResourceDimension::RetainedBytes, ctx.policy().limits.max_retained_bytes, cadmpeg_core::decode::u64_from_index(size_of::<(String, String)>()), "allocate Inventor withheld record identities"))?;
                                        withheld.push((part, format!("inventor:rse:record#{token}-{}", block.ordinal())));
                                    }
                                    None => {
                                        ctx.decode_part(body, extent, PartSpec::new(level("inventor.rse_record_body")?, Vec::new()), |_| Err::<(), _>(CodecError::malformed("RSe record selector names no metadata type descriptor")))?;
                                    }
                                },
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
    )?
    .flatten();
    withheld.sort_unstable();
    for group in withheld.chunk_by(|left, right| left.0 == right.0) {
        output.drop_with_given_up(ctx, &group[0].0, group.iter().map(|(_, id)| id.clone()))?;
    }
    Ok(staged)
}

/// For each type-table index, the given-up part that covers its descriptor:
/// the descriptor's own `inventor.type_descriptor` part, or the
/// `inventor.meta_forward_section` part of the whole table. A record that
/// needs that descriptor is dropped with it (#215 §4.5 rules 2-3).
fn covering_parts(
    ctx: &DecodeContext<'_>,
    output: &PartOutput,
    meta: &SegmentMeta<'_>,
) -> Result<Vec<Option<String>>, CodecError> {
    let Ok(table) = crate::records::frame_type_table(meta.body) else {
        return Ok(Vec::new());
    };
    let section = ctx.capture_part_extent(table.extent)?;
    let mut covering =
        ctx.alloc_filled(table.count, None, "allocate Inventor covering type parts")?;
    let entry_len = cadmpeg_core::decode::u64_from_index(crate::layout::meta_type_descriptor::LEN);
    for (id, part) in output.fidelity().parts() {
        if !matches!(
            part.state(),
            cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
        ) || part.stream() != section.stream()
        {
            continue;
        }
        match part.level().as_str() {
            "inventor.meta_forward_section" if part.offset() == section.offset() => {
                for slot in &mut covering {
                    *slot = Some(id.as_str().to_owned());
                }
                return Ok(covering);
            }
            "inventor.type_descriptor" => {
                let index = section
                    .offset()
                    .checked_add(TYPE_COUNT_LEN)
                    .and_then(|first| part.offset().checked_sub(first))
                    .filter(|delta| delta % entry_len == 0)
                    .and_then(|delta| usize::try_from(delta / entry_len).ok());
                if let Some(slot) = index.and_then(|index| covering.get_mut(index)) {
                    *slot = Some(id.as_str().to_owned());
                }
            }
            _ => {}
        }
    }
    Ok(covering)
}

#[cfg(test)]
mod tests;
