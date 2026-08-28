// SPDX-License-Identifier: Apache-2.0
//! SLDPRT target resolution, write dispatch, and honesty checking.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{
    resolve_write_request, unsupported_target, EncodeInput, ExportPlan, TargetRequest, WriteRequest,
};
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{Annotations, FidelityResolution, WritePath};

use crate::dialect;
use crate::loss::SldprtLossCode;
use crate::{source_records, ReplaySkipped, SemanticFidelity, SldprtCodec, Written};

/// Resolve the request against the source, then plan the export it names
/// (design §8.2).
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
pub(crate) fn plan<'a>(
    input: EncodeInput<'a>,
    request: TargetRequest<'_>,
) -> Result<ExportPlan<'a>, CodecError> {
    let (target, displaced) = resolve(input, request)?;
    let (written, bytes) = write(input, &target)?;
    check_honesty(&target, &written)?;
    Ok(finish(input, target, displaced.as_ref(), &written, bytes))
}

/// Resolve an explicit catalog request or inherit the same-format source row.
fn resolve(
    input: EncodeInput<'_>,
    request: TargetRequest<'_>,
) -> Result<(DialectId, Option<DialectId>), CodecError> {
    match resolve_write_request(input.ir, request, dialect::FORMAT, dialect::TARGETS)? {
        WriteRequest::Catalog { entry, displaced } => Ok((DialectId::pinned(entry.id), displaced)),
        WriteRequest::OffCatalog { dialect } => Ok((dialect.clone(), None)),
    }
}

/// Run replay when the target equals the source row; otherwise run the
/// semantic writer and let its emitted envelope state the resulting row.
fn write(input: EncodeInput<'_>, target: &DialectId) -> Result<(Written, Vec<u8>), CodecError> {
    let source_dialect = input
        .ir
        .source
        .as_ref()
        .filter(|source| source.format == dialect::FORMAT)
        .and_then(|source| source.dialect.as_ref());
    let replay_eligible = source_dialect == Some(target);
    let mut bytes = Vec::new();
    let written = match input.fidelity {
        Some(value) => {
            let records = source_records(input.ir, value)?;
            if replay_eligible {
                SldprtCodec::write_preserved_with_annotations(
                    input.ir,
                    &value.annotations,
                    &records,
                    &mut bytes,
                )?
            } else {
                SldprtCodec::write_semantic(
                    input.ir,
                    &value.annotations,
                    &records,
                    SemanticFidelity::Resolution(FidelityResolution::NotConsumed),
                    &mut bytes,
                )?
            }
        }
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
fn check_honesty(target: &DialectId, written: &Written) -> Result<(), CodecError> {
    let Written::Semantic { dialect: got, .. } = written else {
        return Ok(());
    };
    if got == target {
        return Ok(());
    }
    Err(unsupported_target(
        dialect::FORMAT,
        target.as_str(),
        &format!(
            "the retained document blocks decide the swSolidWorks envelope this writer emits, \
             and from this input that envelope is {got}"
        ),
        dialect::TARGETS,
    ))
}

fn finish<'a>(
    input: EncodeInput<'a>,
    target: DialectId,
    displaced: Option<&DialectId>,
    written: &Written,
    bytes: Vec<u8>,
) -> ExportPlan<'a> {
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
    if let Some(source) = displaced.as_ref() {
        losses.push(SldprtLossCode::SourceDialectDisplaced.note(
            cadmpeg_ir::codec::source_dialect_displaced_message(source, &target),
        ));
    }
    ExportPlan::buffered(
        ExportReport {
            target: Some(target),
            format: dialect::FORMAT.into(),
            census: cadmpeg_ir::EntityCensus {
                basis: cadmpeg_ir::CensusBasis::IrArenas,
                counts: input.ir.census(),
            },
            fidelity,
            write_path,
            losses,
            notes: vec![
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
        },
        bytes,
    )
}
