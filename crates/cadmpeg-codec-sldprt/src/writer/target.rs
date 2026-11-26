// SPDX-License-Identifier: Apache-2.0
//! SLDPRT target resolution, write dispatch, and the row each write is.

use std::collections::HashSet;
use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    carried_source_losses, edit_dropped, edit_dropped_note, edit_unverified,
    target::{ResolvedWrite, WrittenRow},
    EncodeInput, ExportBody, WritePath,
};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::diff::ArenaKind;
use cadmpeg_ir::document::{ArenaName, CadIr};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::source_fidelity::CodecDigests;
use cadmpeg_ir::Annotations;

use crate::history::write::brep_agreement::{agreement, ShapingEdit, Source};
use crate::history::write::edits::UnwrittenEdits;
use crate::loss::SldprtLossCode;
use crate::{
    source_records, HistoryDropped, ReplaySkipped, SemanticFidelity, SemanticPath, SldprtCodec,
    Written,
};

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

/// Write the file in the first path of §3.4 that writes it: replay, patch,
/// synthesis, then the source part as stored.
///
/// [`write_first`] replays or patches. Where it refuses for any reason but a
/// resource ceiling or the destination, or where a patch of a preserved
/// source lands on another dialect than the source's, the document is
/// synthesized without its `SolidWorks` history ([`synthesize`]). Where that
/// refuses too, or lands on another dialect, a preserved source is written as
/// its retained image, without the document's edits, and charges
/// `export:edit.dropped` naming each edited record ([`stored_losses`]).
/// Without a retained image the first write's result stands.
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
    let synthesis = match synthesize(input, digests, preserves, &reason) {
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
    let why = format!(
        "the patch of the edited document {reason}, and its synthesis without the SolidWorks \
         history {synthesis}"
    );
    let losses = stored_losses(input.ir, image, &why);
    Ok((Written::Stored { losses }, image.to_vec()))
}

/// The `export:edit.dropped` losses of `ir` written as its stored part
/// `image` for the reason `why`: one per arena, naming each record that `ir`
/// adds, removes or edits against `image` decoded again. Where `image` does
/// not decode, or no record differs, one loss names the file.
fn stored_losses(ir: &CadIr, image: &[u8], why: &str) -> Vec<LossNote> {
    let reason = format!(
        "keeps the edits of these records as stored ({}): {why}",
        crate::SOURCE_IMAGE_ID
    );
    let losses = SldprtCodec
        .decode(&mut Cursor::new(image), &DecodeOptions::default())
        .map(|source| {
            cadmpeg_ir::diff::diff(source.ir(), ir)
                .per_arena
                .into_iter()
                .filter_map(|arena| {
                    let kind = match arena.kind {
                        ArenaKind::Model(name) => ArenaName::ALL
                            .iter()
                            .position(|candidate| *candidate == name)
                            .map_or(EntityKind::Feature, |index| EntityKind::ALL[index]),
                        ArenaKind::DesignNames | ArenaKind::Native { .. } => EntityKind::Feature,
                    };
                    edit_dropped(
                        kind,
                        "SLDPRT",
                        &reason,
                        arena
                            .added
                            .into_iter()
                            .chain(arena.removed)
                            .chain(arena.modified.into_iter().map(|entity| entity.id)),
                    )
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !losses.is_empty() {
        return losses;
    }
    vec![edit_dropped_note(
        EntityKind::Feature,
        format!(
            "The SLDPRT output is the source part as stored ({}), without the document's edits: \
             {why}.",
            crate::SOURCE_IMAGE_ID
        ),
    )]
}

/// The document synthesized without its `SolidWorks` history: the write
/// takes no record of the `sldprt` native namespace and no codec digest, so
/// the history stage drops what it cannot write (§3.4, Content the target
/// cannot hold). Where the write consumes a sidecar, its path states why the
/// patch did not write the file; the write declares each arena of the
/// namespace dropped with that cause, and the export wrapper names each
/// record. The configuration of each edit that shapes the solid against the
/// retained source image takes the stale mark (§3.4, States); `digests` are
/// the codec digests the decode recorded.
fn synthesize(
    input: EncodeInput<'_>,
    digests: &CodecDigests,
    preserves: bool,
    reason: &str,
) -> Result<(Written, Vec<u8>), CodecError> {
    let shaped = shaping_edits(input, digests)?;
    let mut ir = input.ir.clone();
    ir.native.0.remove("sldprt");
    let mut bytes = Vec::new();
    let mut written = SldprtCodec::write_semantic(
        &ir,
        &Annotations::default(),
        &CodecDigests::new(),
        &[],
        &shaped,
        SemanticFidelity::NotConsumed,
        &mut bytes,
    )?;
    if let Written::Semantic {
        history_dropped, ..
    } = &mut written
    {
        *history_dropped = Some(Box::new(HistoryDropped {
            cause: reason.to_owned(),
            sidecar: preserves && input.fidelity.is_some(),
        }));
    }
    Ok((written, bytes))
}

/// The edits of the document that shape the solid of the retained source
/// image. A document with no such image, or whose native records do not
/// load, has none.
fn shaping_edits(
    input: EncodeInput<'_>,
    digests: &CodecDigests,
) -> Result<Vec<ShapingEdit>, CodecError> {
    let image = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .and_then(|record| record.data());
    let native = input
        .ir
        .native
        .namespace("sldprt")
        .map(crate::native::SldprtNative::load);
    let (Some(image), Some(Ok(native))) = (image, native) else {
        return Ok(Vec::new());
    };
    Ok(agreement(
        input.ir,
        digests,
        Some(&native),
        Some(Source::Image(image)),
        &UnwrittenEdits::default(),
        &HashSet::new(),
    )?
    .shaped)
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
        Some(_) => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            digests,
            &[],
            &[],
            SemanticFidelity::NotConsumed,
            &mut bytes,
        )?,
        None => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            digests,
            &[],
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
        reason: "the swSolidWorks envelope this writer emits states this row".to_owned(),
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
    if let Written::Stored { losses: stored } = written {
        losses.extend(stored.iter().cloned());
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
        native: written.native(),
        losses,
        notes: vec![
            path_note.into(),
            "entity counts are derived from the IR".into(),
        ],
        written_row: written_row(written),
    }
}
