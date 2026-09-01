// SPDX-License-Identifier: Apache-2.0
//! IGES target resolution and retained-image replay.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::target::TargetDescriptor;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{resolve_write_request, EncodeInput, ExportPlan, TargetRequest};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::{CadIr, FidelityResolution, SourceFidelity, WritePath};

use crate::dialect::IgesDialect;
use crate::loss::IgesLossCode;

pub(crate) fn plan(
    input: EncodeInput<'_>,
    request: TargetRequest<'_>,
) -> Result<ExportPlan, CodecError> {
    let resolved = resolve_write_request(
        input.ir,
        request,
        crate::dialect::FORMAT,
        crate::IgesVersion::TARGETS,
    )?;
    let Some(entry) = resolved.catalog_entry() else {
        return match replay_bytes(input.ir, input.fidelity)? {
            Replay::Replayed { bytes } => {
                Ok(replayed_plan(input.ir, resolved.dialect().clone(), bytes))
            }
            Replay::Declined { reason } => Err(resolved.unavailable(match reason {
                Some(reason) => format!(
                    "{reason}; the semantic writer cannot synthesize the inherited dialect"
                ),
                None => "its retained source image is unavailable for byte replay and the semantic writer cannot synthesize it".to_owned(),
            })),
        };
    };
    let preservation_eligible = resolved.source_preservation_eligible();
    if resolved.preserves_source() {
        let replay_failure = match replay_bytes(input.ir, input.fidelity)? {
            Replay::Replayed { bytes } => {
                return Ok(replayed_plan(input.ir, entry.id.clone(), bytes));
            }
            Replay::Declined { reason } => reason,
        };
        synthesized_plan(
            input,
            target_version(entry)?,
            None,
            replay_failure,
            preservation_eligible,
        )
    } else {
        synthesized_plan(
            input,
            target_version(entry)?,
            resolved.displaced_source(),
            None,
            preservation_eligible,
        )
    }
}

fn target_version(target: &TargetDescriptor) -> Result<crate::IgesVersion, CodecError> {
    crate::IgesVersion::from_target(target).ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES target catalog entry {} has no typed write version",
            target.id
        ))
    })
}

fn replayed_plan(ir: &CadIr, dialect: DialectId, bytes: Vec<u8>) -> ExportPlan {
    ExportPlan::buffered(
        super::report(
            dialect,
            FidelityResolution::Replayed,
            WritePath::VerbatimReplay,
            Vec::new(),
            "preserved source container replayed verbatim",
            super::counts_for_ir(ir),
        ),
        bytes,
    )
}

fn synthesized_plan(
    input: EncodeInput<'_>,
    version: crate::IgesVersion,
    displaced: Option<&DialectId>,
    replay_failure: Option<String>,
    preservation_eligible: bool,
) -> Result<ExportPlan, CodecError> {
    let target = IgesDialect::fixed_ascii(version).id();
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
    if let Some(source) = displaced.as_ref() {
        losses.push(IgesLossCode::SourceDialectDisplaced.note(
            cadmpeg_ir::codec::source_dialect_displaced_message(source, &target),
        ));
    }
    let synthesis = super::synthesize(input.ir, version)?;
    losses.extend(synthesis.losses.clone());
    let fidelity = if preservation_eligible && !source_available {
        FidelityResolution::Degraded {
            reason: "preserved IGES source image is unavailable".into(),
        }
    } else if displaced.is_some() {
        if input.fidelity.is_some() {
            FidelityResolution::NotConsumed
        } else {
            FidelityResolution::NotProvided
        }
    } else if let Some(reason) = replay_failure {
        FidelityResolution::Degraded { reason }
    } else if input.fidelity.is_some() {
        FidelityResolution::NotConsumed
    } else {
        FidelityResolution::NotProvided
    };
    Ok(ExportPlan::buffered(
        super::report(
            target,
            fidelity,
            WritePath::Synthesized,
            losses,
            "IGES Fixed ASCII container regenerated from supported neutral geometry",
            synthesis.counts,
        ),
        synthesis.bytes,
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
    let Some(data) = record.data.as_deref() else {
        return Err(CodecError::Malformed(
            "retained IGES source image has no bytes".into(),
        ));
    };
    if record.byte_len != data.len() as u64 || record.sha256 != sha256_hex(data) {
        return Err(CodecError::Malformed(
            "retained IGES source image failed integrity validation".into(),
        ));
    }
    Ok(Replay::Replayed {
        bytes: data.to_vec(),
    })
}
