// SPDX-License-Identifier: Apache-2.0
//! `AmDc` record-body parts and staged occurrence records.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

use super::{AssemblyOccurrence, AssemblyPlacement};
use crate::native::{
    AssemblyOccurrenceRecord, AssemblyPlacementRecord, AssemblyPlacementRecordWire,
};
use crate::records::RseRecordFrame;

pub(super) fn occurrence<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &str,
    frame: &RseRecordFrame<'a>,
    version: u8,
) -> Result<Option<AssemblyOccurrence>, CodecError> {
    crate::records::source_admission::decode_body(ctx, output, expanded, token, frame, |stage| {
        let mut occurrence = super::parse_occurrence(ctx, frame.payload, version)?;
        occurrence.segment_token = token.to_owned();
        occurrence.record_ordinal = frame.ordinal;
        stage.insert_native(
            ctx,
            "assembly_occurrences",
            AssemblyOccurrenceRecord::from_parsed_occurrence(&occurrence),
        )?;
        Ok(occurrence)
    })
}

pub(super) fn placement<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &str,
    frame: &RseRecordFrame<'a>,
) -> Result<Option<AssemblyPlacement<'a>>, CodecError> {
    crate::records::source_admission::decode_body(ctx, output, expanded, token, frame, |stage| {
        let mut placement = super::parse_placement(ctx, frame.payload)?;
        placement.segment_token = token.to_owned();
        placement.record_ordinal = frame.ordinal;
        let native = AssemblyPlacementRecord::try_from(
            AssemblyPlacementRecordWire::from_placement(&placement),
        )
        .map_err(CodecError::malformed)?;
        stage.insert_native(ctx, "assembly_placements", native)?;
        Ok(placement)
    })
}

#[cfg(test)]
mod tests;
