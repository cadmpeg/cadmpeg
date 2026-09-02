// SPDX-License-Identifier: Apache-2.0
//! IGES target planning and retained-image replay.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    Consumption, EncodeInput, ExportBody, ResolvedTarget, ResolvedWrite, SourceIdentity, WritePath,
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
            Replay::Declined { reason } => Err(resolved.unavailable(match reason {
                Some(reason) => format!(
                    "{reason}; the semantic writer cannot synthesize the inherited dialect"
                ),
                None => "its retained source image is unavailable for byte replay and the semantic writer cannot synthesize it".to_owned(),
            })),
        },
        ResolvedTarget::Inherited { index, .. } => {
            replay_or_synthesize(input, crate::IgesVersion::ALL[*index])
        }
        ResolvedTarget::Explicit {
            index,
            entry,
            source: SourceIdentity::Recorded(source),
            ..
        } => {
            let version = crate::IgesVersion::ALL[*index];
            if source == &entry.id {
                replay_or_synthesize(input, version)
            } else {
                synthesized_body(input, version, resolved.displacement_message(), None, false)
            }
        }
        ResolvedTarget::Explicit {
            index,
            source: SourceIdentity::Unrecorded,
            ..
        } => synthesized_body(input, crate::IgesVersion::ALL[*index], None, None, true),
        ResolvedTarget::Explicit { index, .. } | ResolvedTarget::Default { index, .. } => {
            synthesized_body(input, crate::IgesVersion::ALL[*index], None, None, false)
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
        Replay::Declined { reason } => reason,
    };
    synthesized_body(input, version, None, replay_failure, true)
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
    displacement: Option<String>,
    replay_failure: Option<String>,
    preservation_eligible: bool,
) -> Result<ExportBody, CodecError> {
    let source_available = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .is_some();
    let mut losses = Vec::new();
    if preservation_eligible && !source_available {
        losses.push(
            IgesLossCode::PreservedSourceUnavailable.note(
                "preserved IGES source image is unavailable; semantic regeneration is required",
            ),
        );
    }
    let displaced = displacement.is_some();
    if let Some(message) = displacement {
        losses.push(IgesLossCode::SourceDialectDisplaced.note(message));
    }
    let synthesis = super::synthesize(input.ir, version)?;
    losses.extend(synthesis.losses.clone());
    let consumption = if preservation_eligible && !source_available {
        Consumption::Degraded {
            reason: "preserved IGES source image is unavailable".into(),
        }
    } else if displaced {
        Consumption::NotConsumed
    } else if let Some(reason) = replay_failure {
        Consumption::Degraded { reason }
    } else {
        Consumption::NotConsumed
    };
    Ok(super::body(
        synthesis.bytes,
        WritePath::Synthesized { consumption },
        losses,
        "IGES Fixed ASCII container regenerated from supported neutral geometry",
        synthesis.counts,
    ))
}

enum Replay {
    Replayed { bytes: Vec<u8> },
    Declined { reason: Option<String> },
}

impl Replay {
    fn declined() -> Self {
        Self::Declined { reason: None }
    }

    fn declined_because(reason: impl Into<String>) -> Self {
        Self::Declined {
            reason: Some(reason.into()),
        }
    }
}

fn replay_bytes(ir: &CadIr, fidelity: Option<&SourceFidelity>) -> Result<Replay, CodecError> {
    let Some(source) = ir
        .source
        .as_ref()
        .filter(|source| source.format() == crate::dialect::FORMAT)
    else {
        return Ok(Replay::declined());
    };
    let Some(expected) = source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE) else {
        return Ok(Replay::declined_because(format!(
            "preserved IGES source carries no `{DOCUMENT_LOCAL_DIGEST_ATTRIBUTE}` baseline; byte replay skipped"
        )));
    };
    if crate::document_digest(ir) != *expected {
        return Ok(Replay::declined_because(
            "decoded model no longer matches the preserved IGES source digest; byte replay skipped",
        ));
    }
    let Some(record) = fidelity.and_then(|value| value.retained_record(crate::SOURCE_IMAGE_ID))
    else {
        return Ok(Replay::declined());
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
