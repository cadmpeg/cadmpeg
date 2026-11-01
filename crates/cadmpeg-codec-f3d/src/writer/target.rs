// SPDX-License-Identifier: Apache-2.0
//! F3D target resolution, preservation dispatch, and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, Consumption, EncodeInput, ExportBody, PatchConsumption,
    WritePath,
};
use cadmpeg_ir::document::CadIr;

use crate::loss::F3dLossCode;
use crate::{ids, F3dCodec, PreservedWritePath};

/// Preserve an eligible source dialect, or synthesize the selected catalog target.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    if target.entry().is_none() {
        return match preserve(input)? {
            Preservation::Written {
                bytes,
                write_path,
                notes,
            } => Ok(preserved_body(input.ir, write_path, bytes, notes)),
            Preservation::PatchDeclined { reason } => {
                synthesize_displaced(input, target, SynthesisCause::PatchDeclined { reason })
            }
            Preservation::SourceImageUnavailable => {
                synthesize_displaced(input, target, SynthesisCause::SourceImageUnavailable)
            }
        };
    }
    if target.preserves_source() {
        return match preserve(input)? {
            Preservation::Written {
                bytes,
                write_path,
                notes,
            } => Ok(preserved_body(input.ir, write_path, bytes, notes)),
            Preservation::SourceImageUnavailable => {
                synthesized_body(input, SynthesisCause::SourceImageUnavailable)
            }
            Preservation::PatchDeclined { reason } => {
                synthesized_body(input, SynthesisCause::PatchDeclined { reason })
            }
        };
    }
    let cause = if let Some(message) = target.displacement_message() {
        SynthesisCause::Displaced(message)
    } else if target.source_preservation_eligible()
        && input
            .fidelity
            .and_then(|fidelity| fidelity.retained_record(ids::FILE_SOURCE_IMAGE_ID))
            .is_none()
    {
        SynthesisCause::SourceImageUnavailable
    } else {
        SynthesisCause::Fresh
    };
    synthesized_body(input, cause)
}

fn synthesize_displaced(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let synthesis = target.for_synthesis()?;
    let mut body = synthesized_body(input, cause)?;
    if let Some(message) = synthesis.resolution().displacement_message() {
        body.losses
            .push(F3dLossCode::SourceDialectDisplaced.note(message));
    }
    Ok(body)
}

enum Preservation {
    Written {
        bytes: Vec<u8>,
        write_path: PreservedWritePath,
        notes: Vec<String>,
    },
    SourceImageUnavailable,
    PatchDeclined {
        reason: String,
    },
}

fn preserve(input: EncodeInput<'_>) -> Result<Preservation, CodecError> {
    if !input.can_reuse_source() {
        return Ok(Preservation::SourceImageUnavailable);
    }
    let Some(record) = input
        .fidelity
        .and_then(|sidecar| sidecar.retained_record(ids::FILE_SOURCE_IMAGE_ID))
    else {
        return Ok(Preservation::SourceImageUnavailable);
    };
    let Some(data) = record.data() else {
        return Err(CodecError::Malformed(
            "retained F3D source image has no bytes".into(),
        ));
    };
    let Some(expected) = input
        .fidelity
        .and_then(|fidelity| fidelity.decode_digest.as_ref())
    else {
        return Ok(Preservation::PatchDeclined {
            reason: "retained F3D layout has no bound document digest".into(),
        });
    };
    let authored = super::synthesis::write(input.ir)?;
    let omitted_native = input
        .has_foreign_native_content(cadmpeg_ir::codec::FormatId::new(crate::dialect::FORMAT))
        || input.ir.native.namespace("f3d").is_some_and(|namespace| {
            namespace.arenas().iter().any(|(arena, records)| {
                !records.is_empty()
                    && records.iter().any(|record| {
                        authored
                            .native_dispositions
                            .record_disposition("f3d", arena, record.id())
                            == cadmpeg_ir::codec::write::NativeDisposition::Dropped
                    })
            })
        });
    if !authored.losses.is_empty() || omitted_native || !authored.preservation_compatible {
        return Ok(Preservation::PatchDeclined {
            reason: "retained layout cannot carry content omitted by document synthesis".into(),
        });
    }
    let mut bytes = Vec::new();
    let mut notes = Vec::new();
    let write_path =
        match F3dCodec::write_preserved_bytes(input.ir, data, expected, &mut bytes, &mut notes) {
            Ok(path) => path,
            Err(error) if super::synthesis::declines(&error) => {
                return Ok(Preservation::PatchDeclined {
                    reason: error.to_string(),
                });
            }
            Err(error) => return Err(error),
        };
    Ok(Preservation::Written {
        bytes,
        write_path,
        notes,
    })
}

fn preserved_body(
    ir: &CadIr,
    write_path: PreservedWritePath,
    bytes: Vec<u8>,
    notes: Vec<String>,
) -> ExportBody {
    let write_path = match write_path {
        PreservedWritePath::Patched => WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        },
        PreservedWritePath::VerbatimReplay => WritePath::VerbatimReplay,
    };
    // Replay runs only while the document digest matches its decode baseline,
    // and the patcher refuses unless the patched baseline digests equal to the
    // target document, so every arena is carried.
    let mut body = body(ir, write_path, ArenaCoverage::Complete, Vec::new(), bytes);
    body.native_dispositions = cadmpeg_ir::codec::write::NativeDispositions::for_namespace(
        "f3d",
        crate::native::F3D_ARENA_NAMES,
        cadmpeg_ir::codec::write::NativeDisposition::Written,
    );
    body.notes.extend(notes);
    body
}

enum SynthesisCause {
    Fresh,
    Displaced(String),
    SourceImageUnavailable,
    PatchDeclined { reason: String },
}

impl SynthesisCause {
    fn into_fidelity(self) -> (Consumption, Option<cadmpeg_ir::report::loss::LossNote>) {
        match self {
            Self::Fresh => (Consumption::NotConsumed, None),
            Self::PatchDeclined { reason } => (Consumption::Degraded { reason }, None),
            Self::Displaced(message) => (
                Consumption::NotConsumed,
                Some(F3dLossCode::SourceDialectDisplaced.note(message)),
            ),
            Self::SourceImageUnavailable => (
                Consumption::Degraded {
                    reason: "preserved F3D source image is unavailable".into(),
                },
                Some(
                    F3dLossCode::SourcePreservedImageUnavailable
                        .note("preserved F3D source image is unavailable; regenerated from IR"),
                ),
            ),
        }
    }
}

fn synthesized_body(
    input: EncodeInput<'_>,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let super::synthesis::Synthesized {
        bytes,
        coverage,
        losses: synthesis_losses,
        native_dispositions,
        preservation_compatible: _,
    } = super::synthesis::write(input.ir)?;
    let synthesis_cause = match &cause {
        SynthesisCause::PatchDeclined { reason } => {
            Some(cadmpeg_ir::codec::write::SynthesisCause::PatchDeclined {
                reason: reason.clone(),
            })
        }
        _ => None,
    };
    let (consumption, loss) = cause.into_fidelity();
    let mut losses: Vec<_> = loss.into_iter().collect();
    losses.extend(synthesis_losses);
    let mut exported = body(
        input.ir,
        WritePath::Synthesized { consumption },
        ArenaCoverage::Declared(coverage),
        losses,
        bytes,
    );
    exported.synthesis_cause = synthesis_cause;
    exported.native_dispositions = native_dispositions;
    Ok(exported)
}

fn body(
    ir: &CadIr,
    write_path: WritePath,
    coverage: ArenaCoverage,
    losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    bytes: Vec<u8>,
) -> ExportBody {
    let path_note = match &write_path {
        WritePath::VerbatimReplay => "preserved source container replayed verbatim",
        WritePath::Patched { .. } => "preserved source container replayed with semantic patches",
        WritePath::Synthesized { .. } => "source container regenerated from IR",
    };
    ExportBody {
        bytes,
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        write_path,
        coverage,
        native_dispositions: cadmpeg_ir::codec::write::NativeDispositions::for_namespace(
            "f3d",
            crate::native::F3D_ARENA_NAMES,
            cadmpeg_ir::codec::write::NativeDisposition::Dropped,
        ),
        synthesis_cause: None,
        losses,
        notes: vec![
            path_note.into(),
            "entity counts are derived from the IR".into(),
        ],
    }
}
