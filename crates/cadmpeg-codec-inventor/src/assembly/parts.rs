// SPDX-License-Identifier: Apache-2.0
//! Source-owned AmDc record bodies and staged occurrence records.

use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartLevel, PartOutput, PartSpec, PartState};

use super::AssemblyOccurrence;
use crate::native::{AssemblyOccurrenceRecord, RseRecordRecord};
use crate::records::RseRecordFrame;

/// Appendix C's located body includes selector and trailing length. The extended
/// record trailer is a separate sibling and is deliberately not in this view.
fn body<'a>(
    ctx: &DecodeContext<'a>,
    expanded: View<'a>,
    frame: &RseRecordFrame<'a>,
) -> Result<View<'a>, CodecError> {
    let payload_source = ctx.capture_part_extent(frame.payload)?;
    let expanded_source = ctx.capture_part_extent(expanded)?;
    if payload_source.stream() != expanded_source.stream() {
        return Err(CodecError::malformed(
            "assembly payload is not in its expanded bulk stream",
        ));
    }
    let start = frame
        .payload
        .start()
        .checked_sub(4)
        .ok_or_else(|| CodecError::malformed("assembly record has no preceding selector"))?;
    let end = frame
        .payload
        .end()
        .checked_add(4)
        .ok_or_else(|| CodecError::malformed("assembly record body extent overflows"))?;
    let extent = expanded.child(start, end).ok_or_else(|| {
        CodecError::malformed("assembly record body is outside its expanded bulk")
    })?;
    ctx.capture_part_extent(extent)?;
    Ok(extent)
}

pub(super) fn occurrence<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &str,
    frame: &RseRecordFrame<'a>,
) -> Result<Option<AssemblyOccurrence>, CodecError> {
    let extent = body(ctx, expanded, frame)?;
    let level = PartLevel::parse("inventor.rse_record_body")
        .ok_or_else(|| CodecError::malformed("RSe record body is absent from Appendix C"))?;
    let trailer = ctx.capture_part_extent(frame.trailer)?;
    let source = ctx.capture_part_extent(extent)?;
    if trailer.stream() != source.stream() || frame.trailer.start() != extent.end() {
        return Err(CodecError::malformed(
            "assembly trailer is not the actual adjacent sibling",
        ));
    }
    // The actual M-stream block declaration owns this framing summary. It
    // is independent of the B-stream semantic body, including versions with
    // no extended trailer. No artificial containing extent is introduced.
    ctx.capture_part_extent(frame.block_descriptor_source)?;
    let wrapper_id = format!("inventor:rse:record#{token}-{}", frame.ordinal);
    ctx.record(&wrapper_id, frame.block_descriptor_source, || {
        let mut declaration = frame.block_descriptor_source;
        let encoded = crate::reader::u32(&mut declaration, "assembly block-size declaration")?;
        if encoded & 0x8000_0000 == 0 || encoded & 0x7fff_ffff != frame.payload_len()? {
            return Err(CodecError::malformed(
                "assembly frame differs from its stored block declaration",
            ));
        }
        let mut header = extent
            .child(extent.start(), frame.payload.start())
            .ok_or_else(|| CodecError::malformed("assembly selector extent is invalid"))?;
        let selector = crate::reader::u32(&mut header, "assembly record selector")?;
        let mut tail = extent
            .child(frame.payload.end(), extent.end())
            .ok_or_else(|| CodecError::malformed("assembly trailing length extent is invalid"))?;
        let trailing = crate::reader::u32(&mut tail, "assembly trailing payload length")?;
        if selector != frame.selector || trailing != frame.trailing_payload_len()? {
            return Err(CodecError::malformed(
                "assembly frame differs from its actual body header",
            ));
        }
        // Framing and its independently framed trailer remain readable even
        // when the semantic body cannot form an occurrence.
        output.insert_native(
            ctx,
            "rse_records",
            RseRecordRecord::from_frame(token, frame)?,
        )?;
        ctx.decode_part(output, extent, PartSpec::new(level, Vec::new()), |stage| {
            let id = format!("inventor:assembly:occurrence#{token}-{}", frame.ordinal);
            ctx.record(&id, frame.payload, || {
                let mut occurrence = super::parse_occurrence(ctx, frame.payload)?;
                occurrence.segment_token = token.to_owned();
                occurrence.record_ordinal = frame.ordinal;
                stage.insert_native(
                    ctx,
                    "assembly_occurrences",
                    AssemblyOccurrenceRecord::from_parsed_occurrence(&occurrence),
                )?;
                Ok(occurrence)
            })
        })
    })
}

/// Late native projection may omit a body only with this actual reached-part
/// disposition. Successful wrappers already exist in the semantic stage.
pub(crate) fn disposition(
    ctx: &DecodeContext<'_>,
    output: &PartOutput,
    expanded: View<'_>,
    token: &str,
    frame: &RseRecordFrame<'_>,
) -> Result<Option<bool>, CodecError> {
    let extent = body(ctx, expanded, frame)?;
    let source = ctx.capture_part_extent(extent)?;
    let part = output.fidelity().parts().values().find(|part| {
        part.level().as_str() == "inventor.rse_record_body"
            && part.stream() == source.stream()
            && part.offset() == source.offset()
            && part.byte_len() == u64_from_index(extent.window().len())
    });
    let Some(part) = part else { return Ok(None) };
    match part.state() {
        PartState::GivenUp {} | PartState::Decoded {} => {
            let id = format!("inventor:rse:record#{token}-{}", frame.ordinal);
            let formed = output
                .native()
                .namespace("inventor")
                .and_then(|namespace| namespace.arenas().get("rse_records"))
                .is_some_and(|records| records.iter().any(|record| record.id() == id));
            if !formed {
                return Err(CodecError::malformed(
                    "decoded assembly body has no formed wrapper",
                ));
            }
            Ok(Some(matches!(part.state(), PartState::GivenUp {})))
        }
        PartState::Covered { .. } => Err(CodecError::malformed(
            "assembly body is covered without its containing assembly part",
        )),
    }
}

#[cfg(test)]
mod tests;
