// SPDX-License-Identifier: Apache-2.0
//! IGES target planning and retained-image replay.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    Consumption, EncodeInput, ExportBody, ResolvedTarget, ResolvedWrite, WritePath,
};
use cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE;
use cadmpeg_ir::{CadIr, SourceFidelity};

use crate::loss::IgesLossCode;

pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    match resolved.target() {
        // Off-catalog same-format source: only a verbatim replay can honor it.
        ResolvedTarget::Preserved { .. } => match replay_bytes(input.ir, input.fidelity)? {
            Replay::Replayed { bytes } => Ok(replayed_body(input.ir, bytes)),
            Replay::Declined(reason) => Err(resolved.unavailable(match reason {
                DeclineReason::Rejected(reason) => format!(
                    "{reason}; the semantic writer cannot synthesize the inherited dialect"
                ),
                DeclineReason::SourceImageUnavailable => "its retained source image is unavailable for byte replay and the semantic writer cannot synthesize it".to_owned(),
            })),
        },
        ResolvedTarget::Explicit { index, .. }
        | ResolvedTarget::Inherited { index, .. }
        | ResolvedTarget::Default { index, .. } => {
            let version = crate::IgesVersion::ALL[*index];
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
    }
}

/// A same-format source at the resolved target: replay its retained image when
/// it is intact, otherwise regenerate and report why replay was declined.
fn replay_or_synthesize(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
) -> Result<ExportBody, CodecError> {
    let replay_failure = match replay_bytes(input.ir, input.fidelity)? {
        Replay::Replayed { bytes } => return Ok(replayed_body(input.ir, bytes)),
        Replay::Declined(reason) => reason,
    };
    synthesized_body(
        input,
        version,
        SynthesisCause::ReplayDeclined(replay_failure),
    )
}

fn replayed_body(ir: &CadIr, bytes: Vec<u8>) -> ExportBody {
    super::body(
        bytes,
        WritePath::VerbatimReplay,
        Vec::new(),
        "preserved source container replayed verbatim",
        super::counts_for_ir(ir),
    )
}

fn synthesized_body(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let (consumption, loss) = cause.into_fidelity();
    let mut losses = loss.into_iter().collect::<Vec<_>>();
    let synthesis = super::synthesize(input.ir, version)?;
    losses.extend(synthesis.losses.clone());
    Ok(super::body(
        synthesis.bytes,
        WritePath::Synthesized { consumption },
        losses,
        "IGES Fixed ASCII container regenerated from supported neutral geometry",
        synthesis.counts,
    ))
}

enum SynthesisCause {
    Fresh,
    Displaced(String),
    ReplayDeclined(DeclineReason),
}

impl SynthesisCause {
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

    fn into_fidelity(self) -> (Consumption, Option<cadmpeg_ir::LossNote>) {
        match self {
            Self::Fresh => (Consumption::NotConsumed, None),
            Self::Displaced(message) => (
                Consumption::NotConsumed,
                Some(IgesLossCode::SourceDialectDisplaced.note(message)),
            ),
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
    let Some(source) = ir
        .source
        .as_ref()
        .filter(|source| source.format() == crate::dialect::FORMAT)
    else {
        return Ok(Replay::Declined(DeclineReason::SourceImageUnavailable));
    };
    let record = fidelity.and_then(|value| value.retained_record(crate::SOURCE_IMAGE_ID));
    let Some(expected) = source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE) else {
        return Ok(Replay::declined_for_record(
            record,
            format!(
                "preserved IGES source carries no `{DOCUMENT_LOCAL_DIGEST_ATTRIBUTE}` baseline; byte replay skipped"
            ),
        ));
    };
    if crate::document_digest(ir) != *expected {
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
