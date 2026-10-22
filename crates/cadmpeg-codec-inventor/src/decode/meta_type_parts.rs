// SPDX-License-Identifier: Apache-2.0
//! Metadata type values constructed in their genuine source record scopes.

use crate::layout::meta_type_descriptor;
use crate::native::MetaTypeRecord;
use crate::rse::SegmentMeta;
use cadmpeg_core::decode::{DecodeContext, SourceOrigin, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};
use std::ops::Range;

fn level(name: &'static str) -> Result<PartLevel, CodecError> {
    PartLevel::parse(name)
        .ok_or_else(|| CodecError::malformed("metadata part level absent from Appendix C"))
}

fn interval(ctx: &DecodeContext<'_>, source: View<'_>) -> Result<Range<u64>, CodecError> {
    let capture = ctx.capture_part_extent(source)?;
    Ok(capture.offset()..capture.offset() + capture.byte_len())
}

pub(crate) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    token: &str,
    meta: &SegmentMeta<'a>,
) -> Result<Option<Vec<Option<crate::records::TypeDescriptor>>>, CodecError> {
    let streams = ctx.source_streams()?;
    let source = ctx.capture_part_extent(meta.source)?;
    let compressed = ctx.capture_part_extent(meta.compressed)?;
    let expanded = ctx.capture_part_extent(meta.body)?;
    let origin = streams
        .iter()
        .find(|stream| stream.name() == expanded.stream());
    if source.stream() != compressed.stream()
        || compressed.offset() < source.offset()
        || compressed.offset() + compressed.byte_len() != source.offset() + source.byte_len()
        || !origin.is_some_and(|stream| {
            expanded.offset() == 0
                && expanded.byte_len() == stream.byte_len()
                && matches!(stream.origin(), SourceOrigin::Expanded { parent, extent }
                    if parent == compressed.stream()
                        && extent.start == compressed.offset()
                        && extent.end == compressed.offset() + compressed.byte_len())
        })
    {
        return Err(CodecError::malformed(
            "metadata body does not belong to its actual compressed member",
        ));
    }
    ctx.decode_part(
        output,
        meta.source,
        PartSpec::new(level("cfb.stream")?, Vec::new()),
        |member| {
            ctx.decode_part(
                member,
                meta.compressed,
                PartSpec::new(level("inventor.meta_body")?, Vec::new()),
                |body| {
                    let frame = crate::records::frame_type_table(meta.body)?;
                    let framing = vec![
                        interval(ctx, frame.count_source)?,
                        interval(ctx, frame.span_source)?,
                    ];
                    ctx.decode_part(
                        body,
                        frame.extent,
                        PartSpec::new(level("inventor.meta_forward_section")?, framing),
                        |table| {
                            if frame.count > 256 {
                                return Err(CodecError::malformed(
                                    "Inventor type table contains more than 256 descriptors",
                                ));
                            }
                            let mut admitted = ctx.alloc_filled(
                                frame.count,
                                None,
                                "allocate Inventor admitted source types",
                            )?;
                            let mut entries = frame.payload;
                            for index in 0..frame.count {
                                let source = crate::reader::take_child(
                                    &mut entries,
                                    meta_type_descriptor::LEN,
                                    "type descriptor",
                                )?;
                                let slot = index;
                                let index = u32::try_from(index).map_err(|_| {
                                    CodecError::malformed(
                                        "Inventor metadata type index exceeds u32",
                                    )
                                })?;
                                let id = format!("inventor:rse:meta-type#{token}-{index}");
                                let record = ctx.decode_part(
                                    table,
                                    source,
                                    PartSpec::new(level("inventor.type_descriptor")?, Vec::new()),
                                    |record| {
                                        ctx.record(&id, source, || {
                                            let value = crate::records::parse_type_descriptor(
                                                source, index,
                                            )?;
                                            record.insert_native(
                                                ctx,
                                                "meta_types",
                                                MetaTypeRecord {
                                                    id: id.clone(),
                                                    token: token.to_owned(),
                                                    index: value.index,
                                                    type_id: super::hex(&value.id),
                                                    fields: value.fields,
                                                },
                                            )?;
                                            Ok(value)
                                        })
                                    },
                                )?;
                                admitted[slot] = record;
                            }
                            Ok(admitted)
                        },
                    )
                },
            )
        },
    )
    .map(|body| body.flatten().flatten())
}

pub(crate) fn given_up(
    ctx: &DecodeContext<'_>,
    output: &cadmpeg_ir::source_fidelity::parts::PartOutput,
    source: Option<cadmpeg_core::decode::View<'_>>,
) -> Result<bool, CodecError> {
    let Some(source) = source else {
        return Ok(false);
    };
    let capture = ctx.capture_part_extent(source)?;
    let parts = output.fidelity().parts();
    let Some((member_id, _)) = parts.iter().find(|(_, part)| {
        part.level().as_str() == "cfb.stream"
            && part.stream() == capture.stream()
            && part.offset() == capture.offset()
            && part.byte_len() == capture.byte_len()
    }) else {
        return Ok(false);
    };
    for (id, part) in parts {
        if !matches!(
            part.state(),
            cadmpeg_ir::source_fidelity::parts::PartState::GivenUp {}
        ) {
            continue;
        }
        let mut ancestor = Some(id);
        while let Some(id) = ancestor {
            if id == member_id {
                return Ok(true);
            }
            ancestor = parts.get(id).and_then(|part| part.parent());
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests;
