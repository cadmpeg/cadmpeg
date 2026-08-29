// SPDX-License-Identifier: Apache-2.0
//! IGES target resolution and retained-image replay.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{
    resolve_write_request, unsupported_target, EncodeInput, ExportPlan, TargetRequest, WriteRequest,
};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::{CadIr, FidelityResolution, SourceFidelity, WritePath};

use crate::dialect::IgesDialect;
use crate::loss::IgesLossCode;

pub(crate) fn plan<'a>(
    input: EncodeInput<'a>,
    request: TargetRequest<'_>,
) -> Result<ExportPlan<'a>, CodecError> {
    let resolved = resolve_write_request(
        input.ir,
        request,
        crate::dialect::FORMAT,
        crate::dialect::TARGETS,
    )?;
    match resolved {
        WriteRequest::Catalog {
            entry,
            displaced,
            preserve: should_preserve,
        } => {
            let mut replay_failure = None;
            if should_preserve {
                match replay_bytes(input.ir, input.fidelity)? {
                    Replay::Replayed { bytes } => {
                        return Ok(replayed_plan(input.ir, DialectId::pinned(entry.id), bytes));
                    }
                    Replay::Declined { reason } => replay_failure = reason,
                }
            }
            synthesized_plan(
                input,
                crate::dialect::target_version(entry),
                displaced.as_ref(),
                replay_failure,
            )
        }
        WriteRequest::OffCatalog { dialect } => match replay_bytes(input.ir, input.fidelity)? {
            Replay::Replayed { bytes } => Ok(replayed_plan(input.ir, dialect.clone(), bytes)),
            Replay::Declined { .. } => Err(unsupported_target(
                crate::dialect::FORMAT,
                dialect.as_str(),
                "its retained source image is unavailable for byte replay and the semantic \
                     writer cannot synthesize it",
                crate::dialect::TARGETS,
            )),
        },
    }
}

fn replayed_plan(ir: &CadIr, dialect: DialectId, bytes: Vec<u8>) -> ExportPlan<'_> {
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

fn synthesized_plan<'a>(
    input: EncodeInput<'a>,
    version: crate::IgesVersion,
    displaced: Option<&DialectId>,
    replay_failure: Option<String>,
) -> Result<ExportPlan<'a>, CodecError> {
    let target = IgesDialect::fixed_ascii(version).id();
    let preservation_eligible = displaced.is_none()
        && input
            .ir
            .source
            .as_ref()
            .is_some_and(|source| source.format == crate::dialect::FORMAT);
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
        .filter(|source| source.format == crate::dialect::FORMAT)
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
