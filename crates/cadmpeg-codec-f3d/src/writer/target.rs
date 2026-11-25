// SPDX-License-Identifier: Apache-2.0
//! F3D target resolution, preservation dispatch, and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    carried_source_losses, target::ResolvedWrite, ArenaCoverage, Consumption, EncodeInput,
    ExportBody, MissingStateForm, PatchConsumption, WritePath,
};
use cadmpeg_ir::document::CadIr;

use crate::loss::F3dLossCode;
use crate::{ids, F3dCodec, PreservedWritePath};

/// Plan the export the resolved request names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog, and is
/// otherwise the replay law's compare: preserving the retained archive is
/// eligible exactly when `id` is the source's dialect.
///
/// `Inherit` asks for preservation instead: a valid retained baseline
/// replays or patches whatever dialect the source is, the F3Z
/// multi-document row and the recovery row included, which the generator
/// could never synthesize. Where the baseline is not usable, or the patch
/// cannot carry an edit, the document is synthesized; an off-catalog source
/// is then written as the catalog default, and the sealed encoder charges
/// the displacement.
///
/// The catalog default supplies the target only when there is nothing to
/// inherit: the document has no source, or a source of another format.
///
/// The sealed encoder stamps the target identity and maps
/// [`Consumption`] onto the report; this body states only what was done.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let off_catalog = target.entry().is_none();
    if off_catalog || target.preserves_source() {
        let cause = match preserve(input) {
            Ok(Preservation::Written {
                bytes,
                write_path,
                notes,
            }) => return Ok(preserved_body(input, write_path, bytes, notes)),
            Ok(Preservation::SourceImageUnavailable) => SynthesisCause::SourceImageUnavailable,
            Err(error @ (CodecError::ResourceLimit(_) | CodecError::Io(_))) => return Err(error),
            Err(error) => SynthesisCause::PatchDeclined(error.to_string()),
        };
        return synthesized_body(input, cause);
    }
    let cause = if target.source_preservation_eligible()
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

enum Preservation {
    Written {
        bytes: Vec<u8>,
        write_path: PreservedWritePath,
        notes: Vec<String>,
    },
    /// No retained source image, or one without bytes.
    SourceImageUnavailable,
}

fn preserve(input: EncodeInput<'_>) -> Result<Preservation, CodecError> {
    let Some((fidelity, record)) = input.fidelity.and_then(|sidecar| {
        sidecar
            .retained_record(ids::FILE_SOURCE_IMAGE_ID)
            .map(|record| (sidecar, record))
    }) else {
        return Ok(Preservation::SourceImageUnavailable);
    };
    let Some(data) = record.data() else {
        return Ok(Preservation::SourceImageUnavailable);
    };
    let mut bytes = Vec::new();
    let mut notes = Vec::new();
    let write_path =
        F3dCodec::write_preserved_bytes(input.ir, fidelity, data, &mut bytes, &mut notes)?;
    Ok(Preservation::Written {
        bytes,
        write_path,
        notes,
    })
}

fn preserved_body(
    input: EncodeInput<'_>,
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
    // target document, so every arena is carried. Bytes the decode kept
    // without decoding are carried too, and reported.
    let losses = carried_source_losses(input.fidelity, &write_path);
    let mut body = body(input.ir, write_path, ArenaCoverage::Complete, losses, bytes);
    body.notes.extend(notes);
    body
}

enum SynthesisCause {
    Fresh,
    SourceImageUnavailable,
    /// The retained source could not carry the document; the reason names
    /// the edit or the missing baseline.
    PatchDeclined(String),
}

impl SynthesisCause {
    fn into_fidelity(self) -> (Consumption, Option<cadmpeg_ir::report::loss::LossNote>) {
        match self {
            Self::Fresh => (Consumption::NotConsumed, None),
            Self::SourceImageUnavailable => (
                Consumption::Degraded {
                    reason: "preserved F3D source image is unavailable".into(),
                },
                Some(
                    F3dLossCode::SourcePreservedImageUnavailable
                        .note("preserved F3D source image is unavailable; regenerated from IR"),
                ),
            ),
            Self::PatchDeclined(reason) => (
                Consumption::Degraded {
                    reason: format!("patch declined: {reason}"),
                },
                Some(F3dLossCode::WriterPatchDeclined.note(format!(
                    "the retained F3D source could not carry the document ({reason}); the file \
                     was regenerated from the document"
                ))),
            ),
        }
    }
}

fn synthesized_body(
    input: EncodeInput<'_>,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let generated = super::generate::write_new(input.ir, &mut bytes)?;
    let (consumption, loss) = cause.into_fidelity();
    let mut losses: Vec<_> = loss.into_iter().collect();
    losses.extend(generated.losses);
    for appearance in &input.ir.model.appearances {
        let written: &[&str] = match appearance.schema.as_deref().unwrap_or("GenericSchema") {
            "GenericSchema" => &["reflectivity_at_0deg", "refraction_index"],
            "PrismOpaqueSchema" | "PrismMetalSchema" => &["surface_roughness"],
            "PrismTransparentSchema" => &["refraction_index"],
            _ => &[],
        };
        let omitted = appearance
            .properties
            .keys()
            .filter(|key| !written.contains(&key.as_str()))
            .collect::<Vec<_>>();
        if !omitted.is_empty() {
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                    "{} appearance property record(s) of {} were not written: {}",
                    omitted.len(),
                    appearance.id,
                    omitted
                        .iter()
                        .map(|key| key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
        }
    }
    let mut body = body(
        input.ir,
        WritePath::Synthesized { consumption },
        ArenaCoverage::Declared(generated.coverage),
        losses,
        bytes,
    );
    if input.missing_states == MissingStateForm::HistoryTrue {
        body.notes.push(
            "the F3D writer writes the geometry-true form only, so the file holds the current \
             geometry and not the history-true form the request names"
                .to_owned(),
        );
    }
    Ok(body)
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
        losses,
        notes: vec![
            path_note.into(),
            "entity counts are derived from the IR".into(),
        ],
        written_row: None,
    }
}
