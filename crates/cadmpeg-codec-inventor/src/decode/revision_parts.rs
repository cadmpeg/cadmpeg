// SPDX-License-Identifier: Apache-2.0
//! Native revision records built from their actual CFB stream.

use crate::database::RevisionPayload;
use crate::native::{RevisionPayloadForm, RevisionRecord};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec};

pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    source: View<'a>,
) -> Result<usize, CodecError> {
    ctx.capture_part_extent(source)?;
    let stream_level = PartLevel::parse("cfb.stream")
        .ok_or_else(|| CodecError::malformed("CFB stream is absent from Appendix C"))?;
    let record_level = PartLevel::parse("inventor.revision_record")
        .ok_or_else(|| CodecError::malformed("revision record is absent from Appendix C"))?;
    Ok(ctx
        .decode_part(
            output,
            source,
            PartSpec::new(stream_level, Vec::new()),
            |stream| {
                let mut formed = 0;
                crate::database::frame_revisions(source, |ordinal, extent, kind, selector| {
                    let kind = ctx.capture_part_extent(kind)?;
                    let mut framing = vec![kind.offset()..kind.offset() + kind.byte_len()];
                    if let Some(selector) = selector {
                        let selector = ctx.capture_part_extent(selector)?;
                        framing.push(selector.offset()..selector.offset() + selector.byte_len());
                    }
                    let id = format!("inventor:rse:revision#{ordinal}");
                    if ctx
                        .decode_part(
                            stream,
                            extent,
                            PartSpec::new(record_level, framing),
                            |record| {
                                ctx.record(&id, extent, || {
                                    let value = crate::database::parse_revision_source(extent)?;
                                    record.insert_native(
                                        ctx,
                                        "revisions",
                                        RevisionRecord {
                                            id: id.clone(),
                                            ordinal,
                                            revision_id: super::hex(&value.id),
                                            flags: value.flags,
                                            kind: value.kind,
                                            payload_form: match value.payload {
                                                RevisionPayload::None => RevisionPayloadForm::None,
                                                RevisionPayload::Short(_) => {
                                                    RevisionPayloadForm::Short
                                                }
                                                RevisionPayload::Long(_) => {
                                                    RevisionPayloadForm::Long
                                                }
                                            },
                                        },
                                    )
                                })
                            },
                        )?
                        .is_some()
                    {
                        formed += 1;
                    }
                    Ok(())
                })?;
                Ok(formed)
            },
        )?
        .unwrap_or(0))
}

#[cfg(test)]
mod tests;
