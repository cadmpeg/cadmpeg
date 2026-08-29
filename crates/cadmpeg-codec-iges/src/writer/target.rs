// SPDX-License-Identifier: Apache-2.0
//! IGES target resolution and retained-image replay.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{
    plan_preserve_or_synthesize, resolve_write_request, unsupported_target, EncodeInput,
    ExportPlan, PreserveAttempt, TargetRequest,
};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::{CadIr, SourceFidelity};

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
    plan_preserve_or_synthesize(
        resolved,
        |target| match replay_bytes(input.ir, input.fidelity, target)? {
            Replay::Replayed { bytes, dialect } => Ok(PreserveAttempt::Preserved(
                super::replayed_plan(input.ir, dialect, bytes),
            )),
            Replay::Declined { reason } => Ok(PreserveAttempt::Declined(reason)),
        },
        |entry, displaced, replay_declined| {
            super::synthesized_plan(
                input,
                crate::dialect::target_version(entry),
                displaced,
                replay_declined.flatten(),
            )
        },
        |dialect, _| {
            Err(unsupported_target(
                crate::dialect::FORMAT,
                dialect.as_str(),
                "its retained source image is unavailable for byte replay and the semantic \
                     writer cannot synthesize it",
                crate::dialect::TARGETS,
            ))
        },
    )
}

enum Replay {
    Replayed { bytes: Vec<u8>, dialect: DialectId },
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

fn replay_bytes(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    target: &DialectId,
) -> Result<Replay, CodecError> {
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
    let source_dialect = match source
        .dialect
        .as_ref()
        .and_then(|matched| matched.dialect.as_ref())
    {
        None => {
            return Ok(Replay::declined_because(format!(
                "preserved IGES source records no dialect, target is {target}; byte replay skipped"
            )));
        }
        Some(dialect) if dialect != target => {
            return Ok(Replay::declined_because(format!(
                "source is {dialect}, target is {target}; byte replay skipped"
            )));
        }
        Some(dialect) => dialect.clone(),
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
        dialect: source_dialect,
    })
}
