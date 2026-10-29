// SPDX-License-Identifier: Apache-2.0
//! SLDPRT target resolution, write dispatch, and honesty checking.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{target::ResolvedWrite, EncodeInput, ExportBody, WritePath};
use cadmpeg_ir::Annotations;

use crate::loss::SldprtLossCode;
use crate::{source_records, ReplaySkipped, SemanticFidelity, SemanticPath, SldprtCodec, Written};

/// Resolve the request against the source, then plan the export it names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog, and is
/// otherwise the replay law's compare: replaying the retained image is
/// eligible exactly when `id` is the source's dialect.
///
/// `Inherit` preserves the typed source dialect. When retained layout cannot
/// express an edit, synthesis authors the selected envelope from the document.
/// A fresh or cross-format document uses the catalog default.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    if !resolved.preserves_source() || input.fidelity.is_none() {
        let cause = if resolved.preserves_source() && input.fidelity.is_none() {
            super::synthesis::SynthesisCause::SourceImageUnavailable
        } else {
            super::synthesis::SynthesisCause::Fresh
        };
        return super::synthesis::plan(input, resolved, cause);
    }
    let classes = input.source_unrepresented();
    if !classes.is_empty() {
        return super::synthesis::plan(
            input,
            resolved,
            super::synthesis::SynthesisCause::SourceUnrepresented { classes },
        );
    }
    if input
        .ir
        .model
        .attributes
        .iter()
        .any(super::accounting::requires_attribute_omission)
    {
        return super::synthesis::plan(
            input,
            resolved,
            super::synthesis::SynthesisCause::PatchDeclined {
                reason: "the current attributes require omission from scalar SLDPRT fields".into(),
            },
        );
    }
    if super::synthesis::parameters::has_conflict(input.ir) {
        return super::synthesis::plan(
            input,
            resolved,
            super::synthesis::SynthesisCause::PatchDeclined {
                reason: "held parameters conflict with the current operation roles".into(),
            },
        );
    }
    let attempt = write(input, true);
    let (written, bytes) = match attempt {
        Ok(written) => written,
        Err(error) => {
            let reason = match error {
                CodecError::UnsupportedContent(reason) => reason,
                error => error.to_string(),
            };
            return super::synthesis::plan(
                input,
                resolved,
                super::synthesis::SynthesisCause::PatchDeclined { reason },
            );
        }
    };
    if matches!(
        &written,
        Written::Semantic {
            path: SemanticPath::Synthesized,
            ..
        }
    ) {
        return super::synthesis::plan(
            input,
            resolved,
            super::synthesis::SynthesisCause::SourceImageUnavailable,
        );
    }
    if !check_honesty(resolved, &written) {
        return super::synthesis::plan(
            input,
            resolved,
            super::synthesis::SynthesisCause::PatchDeclined {
                reason: "the semantic source envelope differs from the selected target".into(),
            },
        );
    }
    Ok(finish(
        input,
        resolved.displacement_message(),
        &written,
        bytes,
    ))
}

/// Run replay when the target equals the source row; otherwise run the
/// semantic writer and let its emitted envelope state the resulting row.
fn write(input: EncodeInput<'_>, replay_eligible: bool) -> Result<(Written, Vec<u8>), CodecError> {
    let mut bytes = Vec::new();
    let written = match input.fidelity {
        Some(value) if replay_eligible => {
            let records = source_records(input.ir, value);
            SldprtCodec::write_preserved_with_annotations(
                input.ir,
                &value.annotations,
                &records,
                value,
                &mut bytes,
            )?
        }
        Some(_) => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            &[],
            None,
            SemanticFidelity::NotConsumed,
            &mut bytes,
        )?,
        None => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            &[],
            None,
            if replay_eligible {
                SemanticFidelity::ReplaySkipped(ReplaySkipped::ImageMissing)
            } else {
                SemanticFidelity::NotConsumed
            },
            &mut bytes,
        )?,
    };
    Ok((written, bytes))
}

/// Whether a semantic write emitted the selected source envelope.
fn check_honesty(resolved: &ResolvedWrite<'_>, written: &Written) -> bool {
    match written {
        Written::Semantic { dialect, .. } => dialect == resolved.target_id(),
        Written::Replayed => true,
    }
}

fn finish(
    input: EncodeInput<'_>,
    displacement: Option<String>,
    written: &Written,
    bytes: Vec<u8>,
) -> ExportBody {
    let write_path = written.path();
    let mut losses: Vec<_> = written
        .replay_skipped()
        .filter(|(_, reason)| *reason == ReplaySkipped::ImageMissing)
        .map(|(path, _)| {
            SldprtLossCode::SourcePreservedImageUnavailable.note(match path {
                SemanticPath::Patched => {
                    "preserved SLDPRT source image is unavailable; wrote from retained source records with semantic patches"
                }
                SemanticPath::Synthesized => {
                    "preserved SLDPRT source image is unavailable; regenerated from IR"
                }
            })
        })
        .into_iter()
        .collect();
    if let Written::Semantic {
        coverage,
        omitted_envelope_bytes,
        ..
    } = written
    {
        losses.extend(super::envelope::auxiliary_loss(*omitted_envelope_bytes));
        // An unknown carrier cannot be generated. A successful semantic write
        // with one therefore retained or patched its native partition.
        let regenerated_brep = coverage.pcurves
            != cadmpeg_ir::codec::write::ArenaDisposition::Written
            && !super::requires_native_partition(input.ir);
        losses.extend(super::accounting::losses(input.ir, regenerated_brep));
        losses.extend(super::accounting::arena_losses(input.ir, coverage));
    }
    if let Some(message) = displacement {
        losses.push(SldprtLossCode::SourceDialectDisplaced.note(message));
    }
    if matches!(write_path, WritePath::Synthesized { .. }) && input.ir.model.faces.is_empty() {
        losses.push(
            SldprtLossCode::WriterNoExportableSolids
                .note("the document has no exportable B-rep solids"),
        );
    }
    let path_note = match &write_path {
        WritePath::VerbatimReplay => "preserved source container replayed verbatim",
        WritePath::Patched { .. } => "preserved source container replayed with semantic patches",
        WritePath::Synthesized { .. } => "source container regenerated from IR",
    };
    ExportBody {
        bytes,
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: input.ir.census(),
        },
        write_path,
        coverage: written.coverage(),
        native_dispositions: cadmpeg_ir::codec::write::NativeDispositions::for_namespace(
            "sldprt",
            crate::native::SLDPRT_ARENA_NAMES,
            cadmpeg_ir::codec::write::NativeDisposition::Written,
        ),
        synthesis_cause: None,
        losses,
        notes: vec![
            path_note.into(),
            "entity counts are derived from the IR".into(),
        ],
    }
}
