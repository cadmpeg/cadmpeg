// SPDX-License-Identifier: Apache-2.0
//! SLDPRT target resolution, write dispatch, and honesty checking.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, ExportPlan, ResolvedWrite};
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{Annotations, FidelityResolution, WritePath};

use crate::loss::SldprtLossCode;
use crate::{source_records, ReplaySkipped, SemanticFidelity, SldprtCodec, Written};

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
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite,
) -> Result<ExportPlan, CodecError> {
    let target = resolved.dialect().clone();
    let (written, bytes) = write(input, resolved.preserves_source())?;
    check_honesty(resolved, &written)?;
    Ok(finish(
        input,
        target,
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
            let records = source_records(input.ir, value)?;
            SldprtCodec::write_preserved_with_annotations(
                input.ir,
                &value.annotations,
                &records,
                &mut bytes,
            )?
        }
        Some(_) => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            &[],
            SemanticFidelity::Resolution(FidelityResolution::NotConsumed),
            &mut bytes,
        )?,
        None => SldprtCodec::write_semantic(
            input.ir,
            &Annotations::default(),
            &[],
            if replay_eligible {
                SemanticFidelity::ReplaySkipped(ReplaySkipped::ImageMissing)
            } else {
                SemanticFidelity::Resolution(FidelityResolution::NotProvided)
            },
            &mut bytes,
        )?,
    };
    Ok((written, bytes))
}

/// Refuse a semantic write whose emitted `swSolidWorks` envelope does not land
/// on the resolved target.
fn check_honesty(resolved: &ResolvedWrite, written: &Written) -> Result<(), CodecError> {
    let Written::Semantic { dialect: got, .. } = written else {
        return Ok(());
    };
    if got == resolved.dialect() {
        return Ok(());
    }
    Err(resolved.unavailable(format!(
        "the retained document blocks decide the swSolidWorks envelope this writer emits, \
             and from this input that envelope is {got}"
    )))
}

fn finish(
    input: EncodeInput<'_>,
    target: DialectId,
    displacement: Option<String>,
    written: &Written,
    bytes: Vec<u8>,
) -> ExportPlan {
    let write_path = written.path();
    let fidelity = written.fidelity();
    let mut losses: Vec<_> = (written.replay_skipped() == Some(ReplaySkipped::ImageMissing))
        .then(|| {
            SldprtLossCode::SourcePreservedImageUnavailable.note(match write_path {
                WritePath::Patched => {
                    "preserved SLDPRT source image is unavailable; wrote from retained source records with semantic patches"
                }
                WritePath::Synthesized => {
                    "preserved SLDPRT source image is unavailable; regenerated from IR"
                }
                WritePath::VerbatimReplay => unreachable!("a replay did not skip its source image"),
            })
        })
        .into_iter()
        .collect();
    if let Some(message) = displacement {
        losses.push(SldprtLossCode::SourceDialectDisplaced.note(message));
    }
    ExportPlan::buffered(
        ExportReport::native(
            target,
            cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::IrArenas,
                counts: input.ir.census(),
            },
            fidelity,
            write_path,
            losses,
            vec![
                match write_path {
                    WritePath::VerbatimReplay => "preserved source container replayed verbatim",
                    WritePath::Patched => {
                        "preserved source container replayed with semantic patches"
                    }
                    WritePath::Synthesized => "source container regenerated from IR",
                }
                .into(),
                "entity counts are derived from the IR".into(),
            ],
        ),
        bytes,
    )
}
