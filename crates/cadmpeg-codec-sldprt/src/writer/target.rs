// SPDX-License-Identifier: Apache-2.0
//! SLDPRT target resolution, write dispatch, and the row each write is.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    carried_source_losses, edit_unverified,
    target::{ResolvedWrite, WrittenRow},
    EncodeInput, ExportBody, WritePath,
};
use cadmpeg_ir::source_fidelity::CodecDigests;
use cadmpeg_ir::Annotations;

use crate::loss::SldprtLossCode;
use crate::{source_records, ReplaySkipped, SemanticFidelity, SemanticPath, SldprtCodec, Written};

/// Resolve the request against the source, then plan the export it names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog, and is
/// otherwise the replay law's compare: replaying the retained image is
/// eligible exactly when `id` is the source's dialect.
///
/// `Inherit` asks for preservation instead: a valid retained image replays
/// whatever dialect the source is, every versioned row included, which this
/// writer could never synthesize. Where the image is not usable the semantic
/// writer still preserves the dialect whenever the retained blocks carry the
/// source's own `swSolidWorks` envelope, because that envelope is passed
/// through unchanged. Where neither holds, the write lands on the row the
/// emitted envelope states, and the sealed encoder names that row and charges
/// the displacement (§3.4, Refusals).
///
/// The catalog default supplies the target only when there is nothing to
/// inherit: the document has no source, or a source of another format.
///
/// The write compares the codec digests of the sidecar. A part given no
/// sidecar compares those of the document itself, reads it as unedited, and
/// charges `export:edit.unverified`.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    let of_document;
    let (digests, unverified) = match input.fidelity {
        Some(fidelity) => (&fidelity.codec_digests, false),
        None if input.ir.source.is_some() && crate::codec_digests::holds_history(input.ir) => {
            of_document = crate::codec_digests::of_document(input.ir)?;
            (&of_document, true)
        }
        None => {
            of_document = CodecDigests::new();
            (&of_document, false)
        }
    };
    let (written, bytes) = write(input, resolved, digests)?;
    let mut body = finish(input, &written, bytes, digests);
    if unverified {
        body.losses.push(edit_unverified(
            "the SLDPRT part comes with no decode sidecar, so its edits since decode are not \
             known; the writer reads it as unedited and writes the records its native lanes \
             hold, so an edit of a neutral record those lanes state is not written",
        ));
    }
    Ok(body)
}

/// Run replay when the target equals the source row; otherwise run the
/// semantic writer and let its emitted envelope state the resulting row.
fn write(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
    digests: &CodecDigests,
) -> Result<(Written, Vec<u8>), CodecError> {
    let replay_eligible = resolved.preserves_source();
    let mut bytes = Vec::new();
    let written = match input.fidelity {
        Some(value) if replay_eligible => {
            let records = source_records(input.ir, value)?;
            SldprtCodec::write_preserved_with_annotations(
                input.ir,
                value,
                &records,
                resolved.target_id(),
                &mut bytes,
            )?
        }
        Some(_) => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            digests,
            &[],
            SemanticFidelity::NotConsumed,
            &mut bytes,
        )?,
        None => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            digests,
            &[],
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

/// The row a semantic write is: the dialect its emitted `swSolidWorks`
/// envelope states.
fn written_row(written: &Written) -> Option<WrittenRow> {
    let Written::Semantic { dialect, .. } = written else {
        return None;
    };
    Some(WrittenRow {
        target: dialect.clone(),
        reason: "the retained document blocks decide the swSolidWorks envelope this writer \
                 emits"
            .to_owned(),
    })
}

fn finish(
    input: EncodeInput<'_>,
    written: &Written,
    bytes: Vec<u8>,
    digests: &CodecDigests,
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
        stale_solid,
        unwritten_edits,
        unwritten,
        losses: written_losses,
        ..
    } = written
    {
        losses.extend(stale_solid.iter().cloned());
        losses.extend(unwritten_edits.iter().cloned());
        losses.extend(written_losses.iter().cloned());
        // An unknown carrier cannot be generated. A successful semantic write
        // with one therefore retained or patched its native partition.
        let regenerated_brep = coverage.pcurves
            != cadmpeg_ir::codec::write::ArenaDisposition::Written
            && !super::requires_native_partition(input.ir);
        losses.extend(super::accounting::losses(
            input.ir,
            digests,
            regenerated_brep,
        ));
        losses.extend(unwritten.notes());
    }
    if let Written::Stored { loss } = written {
        losses.push(loss.clone());
    }
    losses.extend(carried_source_losses(input.fidelity, &write_path));
    let path_note = match &write_path {
        WritePath::VerbatimReplay if matches!(written, Written::Stored { .. }) => {
            "preserved source part replayed verbatim without the document's edits"
        }
        WritePath::VerbatimReplay => "preserved source container replayed verbatim",
        WritePath::Patched { .. } => "preserved source container replayed with semantic patches",
        WritePath::Synthesized { .. } => "source container regenerated from IR",
    };
    // The SLDPRT form holds no cage, so the census counts no cage body (§5.6).
    let cageless = cadmpeg_ir::codec::write::without_cage_bodies(input.ir);
    ExportBody {
        bytes,
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: cageless.as_ref().unwrap_or(input.ir).census(),
        },
        write_path,
        coverage: written.coverage(),
        losses,
        notes: vec![
            path_note.into(),
            "entity counts are derived from the IR".into(),
        ],
        written_row: written_row(written),
    }
}
