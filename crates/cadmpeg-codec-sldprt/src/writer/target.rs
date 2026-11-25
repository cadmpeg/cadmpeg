// SPDX-License-Identifier: Apache-2.0
//! SLDPRT target resolution, write dispatch, and honesty checking.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    carried_source_losses, edit_dropped_note, edit_unverified, native_records_omitted,
    target::ResolvedWrite, EncodeInput, ExportBody, WritePath,
};
use cadmpeg_ir::schema::EntityKind;
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
/// through unchanged. Where neither holds, the write lands on the totality row
/// and the request is refused by name. There is no fall-through to the catalog
/// default: a same-format conversion never silently changes what the file is.
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
    let (written, bytes) = match write(input, resolved, digests) {
        Err(error @ CodecError::NotImplemented(_)) if resolved.entry().is_none() => {
            let synthesis_targets = crate::dialect::TARGETS
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(resolved.unavailable(format!(
                "cannot preserve {}; synthesis catalog targets: {synthesis_targets}; {error}",
                resolved.target_id()
            )));
        }
        result => result?,
    };
    check_honesty(resolved, &written)?;
    let mut body = finish(
        input,
        resolved.displacement_message(),
        &written,
        bytes,
        digests,
    );
    if unverified {
        body.losses.push(edit_unverified(
            "the SLDPRT part comes with no decode sidecar, so its edits since decode are not \
             known; the writer reads it as unedited and writes the records its native lanes \
             hold, so an edit of a neutral record those lanes state is not written",
        ));
    }
    Ok(body)
}

/// Write the file in the first path of §3.4 that writes it: replay, patch,
/// synthesis, then the source part as stored.
///
/// [`write_first`] replays or patches. Where it refuses for any reason but a
/// resource ceiling or the destination, or where a patch of a preserved
/// source lands on another dialect than the source's, the document is
/// synthesized without its `SolidWorks` history ([`synthesize`]). Where that
/// refuses too, or lands on another dialect, a preserved source is written as
/// its retained image, without the document's edits, and charges
/// `export:edit.dropped` naming the file, since the codec digests do not
/// isolate the edited records. Without a retained image the first write's
/// result stands.
fn write(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
    digests: &CodecDigests,
) -> Result<(Written, Vec<u8>), CodecError> {
    let target = resolved.target_id();
    let preserves = resolved.preserves_source();
    let first = write_first(input, resolved, digests);
    let reason = match &first {
        Ok((Written::Semantic { dialect, .. }, _)) if preserves && dialect != target => Some(
            format!("lands on dialect {dialect}, not the source's {target}"),
        ),
        Ok(_) | Err(CodecError::ResourceLimit(_) | CodecError::Io(_)) => None,
        Err(error) => Some(format!("refuses it ({error})")),
    };
    let Some(reason) = reason else {
        return first;
    };
    let stored = input
        .fidelity
        .filter(|_| preserves)
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .and_then(|record| record.data());
    let synthesis = match synthesize(input, preserves, &reason) {
        Ok((Written::Semantic { dialect, .. }, _)) if stored.is_some() && dialect != *target => {
            format!("lands on dialect {dialect}")
        }
        Ok(written) => return Ok(written),
        Err(error @ (CodecError::ResourceLimit(_) | CodecError::Io(_))) => return Err(error),
        Err(error) => format!("refuses it ({error})"),
    };
    let Some(image) = stored else {
        return first;
    };
    let loss = edit_dropped_note(
        EntityKind::Feature,
        format!(
            "The SLDPRT output is the source part as stored ({}), without the document's \
             edits: the patch of the edited document {reason}, and its synthesis without the \
             SolidWorks history {synthesis}; the codec digests do not isolate the edited records.",
            crate::SOURCE_IMAGE_ID
        ),
    );
    Ok((Written::StoredImage { loss }, image.to_vec()))
}

/// The document synthesized without its `SolidWorks` history: the write
/// takes no record of the `sldprt` native namespace and no codec digest, so
/// the history stage drops what it cannot write (§3.4, Content the target
/// cannot hold). `export:native.records-omitted` names each native record
/// left out, and why the patch did not write the file.
fn synthesize(
    input: EncodeInput<'_>,
    preserves: bool,
    reason: &str,
) -> Result<(Written, Vec<u8>), CodecError> {
    let mut ir = input.ir.clone();
    let omitted = ir
        .native
        .0
        .remove("sldprt")
        .map(|namespace| {
            namespace
                .arenas()
                .values()
                .flatten()
                .map(|record| record.id().to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let fidelity = if preserves && input.fidelity.is_some() {
        SemanticFidelity::ReplaySkipped(ReplaySkipped::PatchDeclined)
    } else {
        SemanticFidelity::NotConsumed
    };
    let mut bytes = Vec::new();
    let mut written = SldprtCodec::write_semantic(
        &ir,
        &Annotations::default(),
        &CodecDigests::new(),
        &[],
        None,
        fidelity,
        &mut bytes,
    )?;
    if let Written::Semantic { losses, .. } = &mut written {
        losses.extend(native_records_omitted(
            "SLDPRT",
            &format!(
                "because the patch of the document {reason}, and its synthesis writes no \
                 SolidWorks history record"
            ),
            omitted,
        ));
    }
    Ok((written, bytes))
}

/// Run replay when the target equals the source row; otherwise run the
/// semantic writer and let its emitted envelope state the resulting row.
fn write_first(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite<'_>,
    digests: &CodecDigests,
) -> Result<(Written, Vec<u8>), CodecError> {
    let replay_eligible = resolved.preserves_source();
    let mut bytes = Vec::new();
    let written = match input.fidelity {
        Some(value) if replay_eligible => {
            let records = source_records(input.ir, value)?;
            SldprtCodec::write_preserved_with_annotations(input.ir, value, &records, &mut bytes)?
        }
        Some(value) => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            digests,
            &[],
            value
                .retained_record(crate::SOURCE_IMAGE_ID)
                .and_then(|record| record.data()),
            SemanticFidelity::NotConsumed,
            &mut bytes,
        )?,
        None => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            digests,
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

/// Refuse a semantic write whose emitted `swSolidWorks` envelope does not land
/// on the resolved target.
fn check_honesty(resolved: &ResolvedWrite<'_>, written: &Written) -> Result<(), CodecError> {
    let Written::Semantic { dialect: got, .. } = written else {
        return Ok(());
    };
    if got == resolved.target_id() {
        return Ok(());
    }
    Err(resolved.unavailable(format!(
        "the retained document blocks decide the swSolidWorks envelope this writer emits, \
             and from this input that envelope is {got}"
    )))
}

fn finish(
    input: EncodeInput<'_>,
    displacement: Option<String>,
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
    if let Written::StoredImage { loss } = written {
        losses.push(loss.clone());
    }
    if let Some(message) = displacement {
        losses.push(SldprtLossCode::SourceDialectDisplaced.note(message));
    }
    losses.extend(carried_source_losses(input.fidelity, &write_path));
    let path_note = match &write_path {
        WritePath::VerbatimReplay if matches!(written, Written::StoredImage { .. }) => {
            "preserved source replayed verbatim without the document's edits"
        }
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
        losses,
        notes: vec![
            path_note.into(),
            "entity counts are derived from the IR".into(),
        ],
    }
}
