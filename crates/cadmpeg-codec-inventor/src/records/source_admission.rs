// SPDX-License-Identifier: Apache-2.0
//! `RSe` record-body parts.
use super::RseRecordFrame;
use crate::native::RseRecordRecord;
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::{DecodeParts, PartOutput};

/// The located body: selector, payload and trailing length. The extended
/// record trailer is a separate sibling and is not in this view.
pub(crate) fn body<'a>(
    expanded: View<'a>,
    frame: &RseRecordFrame<'a>,
) -> Result<View<'a>, CodecError> {
    if frame.payload.location().space != expanded.location().space {
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
    expanded
        .child(start, end)
        .ok_or_else(|| CodecError::malformed("assembly record body is outside its expanded bulk"))
}

pub(crate) fn decode_body<'a, T>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &str,
    frame: &RseRecordFrame<'a>,
    decode: impl FnOnce(&mut PartOutput) -> Result<T, CodecError>,
) -> Result<Option<T>, CodecError> {
    let extent = body(expanded, frame)?;
    if frame.trailer.start() != extent.end() {
        return Err(CodecError::malformed(
            "assembly trailer is not the actual adjacent sibling",
        ));
    }
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
    // The frame stays readable when the semantic body cannot form a record.
    output.insert_native(
        ctx,
        "rse_records",
        RseRecordRecord::from_frame(token, frame)?,
    )?;
    let spec = crate::decode::part_spec("inventor.rse_record_body", extent, || {
        crate::decode::expanded_bulk_stream(token)
    })?;
    ctx.decode_part(output, extent, spec, decode)
}
