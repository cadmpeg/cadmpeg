// SPDX-License-Identifier: Apache-2.0
//! `AmDc` record-body parts and staged occurrence records.

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::source_fidelity::parts::PartOutput;

use super::{AssemblyOccurrence, AssemblyPlacement};
use crate::native::{
    AssemblyGroundingRecord, AssemblyOccurrenceRecord, AssemblyPlacementRecord,
    AssemblyPlacementRecordWire, DesignViewRepresentationRecord, DesignViewVisibilityOffRecord,
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
        token.clone_into(&mut occurrence.segment_token);
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
        token.clone_into(&mut placement.segment_token);
        placement.record_ordinal = frame.ordinal;
        let native = AssemblyPlacementRecord::try_from(
            AssemblyPlacementRecordWire::from_placement(&placement),
        )
        .map_err(CodecError::malformed)?;
        stage.insert_native(ctx, "assembly_placements", native)?;
        Ok(placement)
    })
}

pub(super) fn grounding<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &str,
    frame: &RseRecordFrame<'a>,
) -> Result<(), CodecError> {
    crate::records::source_admission::decode_body(ctx, output, expanded, token, frame, |stage| {
        let mut grounding = super::parse_grounding(frame.payload)?;
        token.clone_into(&mut grounding.segment_token);
        grounding.record_ordinal = frame.ordinal;
        stage.insert_native(
            ctx,
            "assembly_groundings",
            AssemblyGroundingRecord::from_grounding(&grounding),
        )
    })?;
    Ok(())
}

pub(super) fn design_view_representation<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &str,
    frame: &RseRecordFrame<'a>,
) -> Result<(), CodecError> {
    crate::records::source_admission::decode_body(ctx, output, expanded, token, frame, |stage| {
        let mut representation = super::parse_design_view_representation(ctx, frame.payload)?;
        token.clone_into(&mut representation.segment_token);
        representation.record_ordinal = frame.ordinal;
        stage.insert_native(
            ctx,
            "design_view_representations",
            DesignViewRepresentationRecord::from_representation(&representation),
        )
    })?;
    Ok(())
}

pub(super) fn visibility_off<'a>(
    ctx: &DecodeContext<'a>,
    output: &mut PartOutput,
    expanded: View<'a>,
    token: &str,
    frame: &RseRecordFrame<'a>,
) -> Result<(), CodecError> {
    crate::records::source_admission::decode_body(ctx, output, expanded, token, frame, |stage| {
        let mut state = super::parse_visibility_off(frame.payload)?;
        token.clone_into(&mut state.segment_token);
        state.record_ordinal = frame.ordinal;
        stage.insert_native(
            ctx,
            "design_view_visibility",
            DesignViewVisibilityOffRecord::from_state(&state),
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
