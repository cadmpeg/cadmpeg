// SPDX-License-Identifier: Apache-2.0
//! IGES target planning and retained-image replay.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    carried_source_losses, target::ResolvedWrite, ArenaCoverage, Consumption, EncodeInput,
    ExportBody, WritePath,
};
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::loss::IgesLossCode;

/// Write the resolved version. A source of an off-catalog version is replayed
/// when its retained image is intact and otherwise written as the catalog
/// default (§3.4, Refusals, `Inherit`).
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let version = resolved
        .synthesized_row()
        .and_then(|(index, _)| crate::IgesVersion::ALL.get(index).copied());
    debug_assert!(
        version.is_some(),
        "the IGES catalog declares a default and is projected from IgesVersion::ALL"
    );
    let version = version.unwrap_or(crate::IgesVersion::V5_3);
    if resolved.preserves_source() {
        replay_or_synthesize(input, version)
    } else {
        synthesized_body(
            input,
            version,
            SynthesisCause::from_resolution(input, resolved),
        )
    }
}

/// A same-format source at the resolved target: replay its retained image when
/// it is intact, otherwise regenerate `version` and report why replay was
/// declined.
fn replay_or_synthesize(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
) -> Result<ExportBody, CodecError> {
    let replay_failure = match replay_bytes(input.ir, input.fidelity)? {
        Replay::Replayed { bytes } => return Ok(replayed_body(input, bytes)),
        Replay::Declined(reason) => reason,
    };
    synthesized_body(
        input,
        version,
        SynthesisCause::ReplayDeclined(replay_failure),
    )
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
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let (consumption, loss) = cause.into_fidelity();
    let mut losses = loss.into_iter().collect::<Vec<_>>();
    let recovered = cadmpeg_ir::codec::write::edge_recovery::recover_edge_curves(input.ir)?;
    let synthesis = super::degrade::synthesize_dropping(&recovered.document, version)?;
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
    Ok(super::body(
        synthesis.bytes,
        WritePath::Synthesized { consumption },
        ArenaCoverage::Declared(super::SYNTHESIS_COVERAGE),
        losses,
        "IGES Fixed ASCII container regenerated from supported neutral geometry",
        synthesis.counts,
    ))
}

enum SynthesisCause {
    Fresh,
    ReplayDeclined(DeclineReason),
}

impl SynthesisCause {
    fn from_resolution(input: EncodeInput<'_>, resolved: &ResolvedWrite<'_>) -> Self {
        if resolved.source_preservation_eligible() && !source_record_available(input.fidelity) {
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable)
        } else {
            Self::Fresh
        }
    }

    fn into_fidelity(self) -> (Consumption, Option<cadmpeg_ir::report::loss::LossNote>) {
        match self {
            Self::Fresh => (Consumption::NotConsumed, None),
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable) => (
                Consumption::Degraded {
                    reason: "preserved IGES source image is unavailable".into(),
                },
                Some(IgesLossCode::PreservedSourceUnavailable.note(
                    "preserved IGES source image is unavailable; semantic regeneration is required",
                )),
            ),
            Self::ReplayDeclined(DeclineReason::Rejected(reason)) => {
                (Consumption::Degraded { reason }, None)
            }
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

fn replay_bytes(ir: &CadIr, fidelity: Option<&SourceFidelity>) -> Result<Replay, CodecError> {
    let (Some(_), Some(fidelity)) = (
        ir.source
            .as_ref()
            .filter(|source| source.format() == crate::dialect::FORMAT),
        fidelity,
    ) else {
        return Ok(Replay::Declined(DeclineReason::SourceImageUnavailable));
    };
    let record = fidelity.retained_record(crate::SOURCE_IMAGE_ID);
    if fidelity.edited(ir)? {
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
