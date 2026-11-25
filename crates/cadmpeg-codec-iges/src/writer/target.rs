// SPDX-License-Identifier: Apache-2.0
//! IGES target planning and retained-image replay.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    carried_source_losses,
    target::{ResolvedTarget, ResolvedWrite},
    ArenaCoverage, Consumption, EncodeInput, ExportBody, WritePath,
};
use cadmpeg_ir::report::export::SynthesisCause;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::loss::IgesLossCode;

pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    match resolved.target() {
        // Off-catalog same-format source: only a verbatim replay can honor it.
        ResolvedTarget::Preserved { .. } => {
            match replay_bytes(input.ir, input.fidelity, input.limits)? {
                Replay::Replayed { bytes } => Ok(replayed_body(input, bytes)),
                Replay::Declined(reason) => Err(resolved.unavailable(match reason {
                    DeclineReason::Rejected(reason) => {
                        format!(
                            "{reason}; the semantic writer cannot synthesize the inherited dialect"
                        )
                    }
                    DeclineReason::SourceImageUnavailable => {
                        "its retained source image is unavailable for byte replay and the \
                     semantic writer cannot synthesize it"
                            .to_owned()
                    }
                })),
            }
        }
        ResolvedTarget::Explicit { index, .. }
        | ResolvedTarget::Inherited { index, .. }
        | ResolvedTarget::Default { index, .. } => {
            let version = crate::IgesVersion::ALL[*index];
            if resolved.preserves_source() {
                replay_or_synthesize(input, version)
            } else {
                synthesized_body(input, version, Synthesis::from_resolution(input, resolved))
            }
        }
    }
}

/// A same-format source at the resolved target: replay its retained image when
/// it is intact, otherwise regenerate and report why replay was declined.
fn replay_or_synthesize(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
) -> Result<ExportBody, CodecError> {
    let replay_failure = match replay_bytes(input.ir, input.fidelity, input.limits)? {
        Replay::Replayed { bytes } => return Ok(replayed_body(input, bytes)),
        Replay::Declined(reason) => reason,
    };
    synthesized_body(input, version, Synthesis::ReplayDeclined(replay_failure))
}

fn replayed_body(input: EncodeInput<'_>, bytes: Vec<u8>) -> ExportBody {
    super::body(
        bytes,
        WritePath::VerbatimReplay,
        // Replay runs only while the document digest matches its decode baseline.
        ArenaCoverage::Complete,
        carried_source_losses(input.fidelity, &WritePath::VerbatimReplay),
        "preserved source container replayed verbatim",
        super::counts_for_ir(input.ir),
    )
}

fn synthesized_body(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
    cause: Synthesis,
) -> Result<ExportBody, CodecError> {
    let (consumption, loss, synthesis_cause) = cause.into_fidelity();
    let mut losses = loss.into_iter().collect::<Vec<_>>();
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(input.ir)?;
    let synthesis =
        super::degrade::synthesize_dropping(&recovered.document, version, input.limits)?;
    losses.extend(synthesis.losses.clone());
    if !recovered.sampled_agreements.is_empty() {
        losses.push(IgesLossCode::EdgePcurveAgreementSampled.note(format!(
            "{} recovered edge(s) have coedge curves compared at five parameters; agreement between samples is not established",
            recovered.sampled_agreements.len()
        )));
    }
    if !recovered.sampled_fits.is_empty() {
        losses.push(IgesLossCode::EdgePcurveSampledToBspline.note(format!(
            "{} recovered edge(s) were fitted as B-splines with sampled residual at most {} model length units",
            recovered.sampled_fits.len(), recovered.residual_limit
        )));
    }
    let mut body = super::body(
        synthesis.bytes,
        WritePath::Synthesized { consumption },
        ArenaCoverage::Declared(super::SYNTHESIS_COVERAGE),
        losses,
        "IGES Fixed ASCII container regenerated from supported neutral geometry",
        synthesis.counts,
    );
    body.synthesis_cause = synthesis_cause;
    Ok(body)
}

enum Synthesis {
    Fresh,
    Displaced(String),
    ReplayDeclined(DeclineReason),
}

impl Synthesis {
    fn from_resolution(input: EncodeInput<'_>, resolved: &ResolvedWrite<'_>) -> Self {
        if let Some(message) = resolved.displacement_message() {
            Self::Displaced(message)
        } else if resolved.source_preservation_eligible()
            && !source_record_available(input.fidelity)
        {
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable)
        } else {
            Self::Fresh
        }
    }

    /// How fidelity was consumed, the loss the writer charges, and the cause
    /// the report records for a same-format source.
    fn into_fidelity(
        self,
    ) -> (
        Consumption,
        Option<cadmpeg_ir::report::loss::LossNote>,
        Option<SynthesisCause>,
    ) {
        match self {
            Self::Fresh => (Consumption::NotConsumed, None, None),
            Self::Displaced(message) => (
                Consumption::NotConsumed,
                Some(IgesLossCode::SourceDialectDisplaced.note(message.clone())),
                Some(SynthesisCause::PatchDeclined { reason: message }),
            ),
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable) => (
                Consumption::Degraded {
                    reason: "preserved IGES source image is unavailable".into(),
                },
                Some(IgesLossCode::PreservedSourceUnavailable.note(
                    "preserved IGES source image is unavailable; semantic regeneration is required",
                )),
                Some(SynthesisCause::PatchDeclined {
                    reason: "the source fidelity holds no IGES source image".into(),
                }),
            ),
            Self::ReplayDeclined(DeclineReason::Rejected(reason)) => (
                Consumption::Degraded {
                    reason: reason.clone(),
                },
                None,
                Some(SynthesisCause::PatchDeclined { reason }),
            ),
        }
    }
}

fn source_record_available(fidelity: Option<&SourceFidelity>) -> bool {
    fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .is_some()
}

enum Replay {
    Replayed { bytes: Vec<u8> },
    Declined(DeclineReason),
}

enum DeclineReason {
    SourceImageUnavailable,
    Rejected(String),
}

impl Replay {
    fn declined_for_record(
        record: Option<&cadmpeg_ir::RetainedSourceRecord>,
        reason: impl Into<String>,
    ) -> Self {
        if record.is_some() {
            Self::Declined(DeclineReason::Rejected(reason.into()))
        } else {
            Self::Declined(DeclineReason::SourceImageUnavailable)
        }
    }
}

fn replay_bytes(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<Replay, CodecError> {
    let (Some(_), Some(fidelity)) = (
        ir.source
            .as_ref()
            .filter(|source| source.format() == crate::dialect::FORMAT),
        fidelity,
    ) else {
        return Ok(Replay::Declined(DeclineReason::SourceImageUnavailable));
    };
    let record = fidelity.retained_record(crate::SOURCE_IMAGE_ID);
    if fidelity.edited(ir, limits)? {
        return Ok(Replay::declined_for_record(
            record,
            "decoded model no longer matches the preserved IGES source digest; byte replay skipped",
        ));
    }
    let Some(record) = record else {
        return Ok(Replay::Declined(DeclineReason::SourceImageUnavailable));
    };
    let Some(data) = record.data() else {
        return Err(CodecError::Malformed(
            "retained IGES source image has no bytes".into(),
        ));
    };
    Ok(Replay::Replayed {
        bytes: data.to_vec(),
    })
}

#[cfg(test)]
mod tests;
