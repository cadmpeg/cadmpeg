// SPDX-License-Identifier: Apache-2.0
//! IGES target planning and retained-image replay.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    ArenaCoverage, Consumption, EncodeInput, ExportBody, WritePath,
    target::{ResolvedTarget, ResolvedWrite},
};
use cadmpeg_ir::hash::DOCUMENT_FINGERPRINT_METADATA_KEY;
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
                DeclineReason::Rejected(reason) => {
                    format!("{reason}; the semantic writer cannot synthesize the inherited dialect")
                }
                DeclineReason::SourceImageUnavailable => {
                    "its retained source image is unavailable for byte replay and the \
                     semantic writer cannot synthesize it"
                        .to_owned()
                }
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
    let mut body = super::body(
        bytes,
        WritePath::VerbatimReplay,
        // Replay runs only while the document digest matches its decode baseline.
        ArenaCoverage::Complete,
        Vec::new(),
        "preserved source container replayed verbatim",
        super::counts_for_ir(ir),
    );
    body.native_dispositions = cadmpeg_ir::codec::write::NativeDispositions::for_namespace(
        "iges",
        crate::native::ARENAS,
        cadmpeg_ir::codec::write::NativeDisposition::Written,
    );
    body
}

fn synthesized_body(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let synthesis_cause = cause.report_cause();
    let (consumption, loss) = cause.into_fidelity();
    let mut losses = loss.into_iter().collect::<Vec<_>>();
    let synthesis = super::synthesize(input.ir, version)?;
    losses.extend(synthesis.losses.clone());
    let mut body = super::body(
        synthesis.bytes,
        WritePath::Synthesized { consumption },
        ArenaCoverage::Declared(synthesis.coverage),
        losses,
        "IGES Fixed ASCII container regenerated from supported neutral geometry",
        synthesis.counts,
    );
    body.native_dispositions = super::synthesized_native_dispositions(
        input.ir,
        &body.losses,
        &synthesis.regenerated_native,
    );
    body.synthesis_cause = synthesis_cause;
    Ok(body)
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

    fn report_cause(&self) -> Option<cadmpeg_ir::codec::write::SynthesisCause> {
        let reason = match self {
            Self::ReplayDeclined(DeclineReason::Rejected(reason)) => reason.clone(),
            Self::ReplayDeclined(DeclineReason::SourceImageUnavailable) => {
                "preserved IGES source image is unavailable".to_owned()
            }
            Self::Fresh | Self::Displaced(_) => return None,
        };
        Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined { reason })
    }

    fn into_fidelity(self) -> (Consumption, Option<cadmpeg_ir::report::loss::LossNote>) {
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
                let loss = IgesLossCode::PreservedSourceUnavailable
                    .note(format!("{reason}; IGES is synthesized from the document"));
                (Consumption::Degraded { reason }, Some(loss))
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
    let Some(_source) = ir
        .source
        .as_ref()
        .filter(|source| source.format() == crate::dialect::FORMAT)
    else {
        return Ok(Replay::Declined(DeclineReason::SourceImageUnavailable));
    };
    if fidelity.is_some_and(|fidelity| !fidelity.unrepresented().is_empty()) {
        return Ok(Replay::Declined(DeclineReason::Rejected(
            "retained source contains untyped content; synthesis is required".to_owned(),
        )));
    }
    let record = fidelity.and_then(|value| value.retained_record(crate::SOURCE_IMAGE_ID));
    let Some(expected) =
        fidelity.and_then(|fidelity| fidelity.metadata.get(DOCUMENT_FINGERPRINT_METADATA_KEY))
    else {
        return Ok(Replay::declined_for_record(
            record,
            format!(
                "preserved IGES source carries no `{DOCUMENT_FINGERPRINT_METADATA_KEY}` baseline; byte replay skipped"
            ),
        ));
    };
    // Replay may preserve layout and identities, but cannot preserve a native
    // arena whose actual payload synthesis drops. Name properties with neutral
    // counterparts are regenerated from those counterparts.
    if let Some(namespace) = ir.native.namespace("iges") {
        let represented_names = super::represented_name_properties(ir, namespace).is_some();
        if let Some((arena, _)) = namespace.arenas().iter().find(|(arena, records)| {
            !(records.is_empty()
                || super::ALLOWED_NATIVE_ARENAS.contains(&arena.as_str())
                || (represented_names
                    && matches!(arena.as_str(), "properties" | "product_properties")))
        }) {
            return Ok(Replay::declined_for_record(
                record,
                format!(
                    "IGES native arena {arena} has no synthesized encoding; byte replay cannot supply that content"
                ),
            ));
        }
    }
    if crate::document_digest(ir)? != *expected {
        return Ok(Replay::declined_for_record(
            record,
            "decoded model no longer matches the preserved IGES source digest; byte replay skipped",
        ));
    }
    let Some(record) = record else {
        return Ok(Replay::Declined(DeclineReason::SourceImageUnavailable));
    };
    let Some(data) = record.data() else {
        return Ok(Replay::Declined(DeclineReason::SourceImageUnavailable));
    };
    Ok(Replay::Replayed {
        bytes: data.to_vec(),
    })
}

#[cfg(test)]
mod tests;
