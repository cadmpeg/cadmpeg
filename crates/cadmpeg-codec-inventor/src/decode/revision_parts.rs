// SPDX-License-Identifier: Apache-2.0
//! Native revision records built from their actual CFB stream.

use crate::database::RevisionPayload;
use crate::native::{RevisionPayloadForm, RevisionRecord};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

pub(super) fn stage<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    source: View<'a>,
) -> Result<usize, CodecError> {
    let name = || crate::rse::REVISION_STREAM.to_owned();
    Ok(ctx
        .decode_part(
            output,
            source,
            super::part_spec("cfb.stream", source, name)?,
            |stream| {
                let mut formed = 0;
                crate::database::frame_revisions(source, |ordinal, extent| {
                    let id = format!("inventor:rse:revision#{ordinal}");
                    if ctx
                        .decode_part(
                            stream,
                            extent,
                            super::part_spec("inventor.revision_record", extent, name)?,
                            |record| {
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
                                            RevisionPayload::Short(_) => RevisionPayloadForm::Short,
                                            RevisionPayload::Long(_) => RevisionPayloadForm::Long,
                                        },
                                    },
                                )
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
