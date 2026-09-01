// SPDX-License-Identifier: Apache-2.0
//! F3D target resolution, preservation dispatch, and export reporting.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, ExportPlan, ResolvedWrite};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::ExportReport;
use cadmpeg_ir::{FidelityResolution, WritePath};

use crate::loss::F3dLossCode;
use crate::{ids, F3dCodec};

/// Resolve the request against the source, then plan the export it names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog, and is
/// otherwise the replay law's compare: preserving the retained archive is
/// eligible exactly when `id` is the source's dialect.
///
/// `Inherit` asks for preservation instead: a valid retained baseline
/// replays or patches whatever dialect the source is, the F3Z
/// multi-document row and the recovery row included, which the generator
/// could never synthesize. Where the baseline is not usable, `Inherit`
/// synthesizes the source's own dialect, and refuses when that dialect is
/// not a target. There is no fall-through to the catalog default: a
/// same-format conversion never silently changes what the file is.
///
/// The catalog default supplies the target only when there is nothing to
/// inherit: the document has no source, or a source of another format.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    resolved: &ResolvedWrite,
) -> Result<ExportPlan, CodecError> {
    let Some(entry) = resolved.catalog_entry() else {
        return match preserve(input)? {
            Preservation::Written { bytes, write_path } => Ok(preserved_plan(
                input.ir,
                resolved.dialect().clone(),
                write_path,
                bytes,
            )),
            Preservation::Declined => Err(resolved.unavailable(
                "its retained source image is unavailable for preservation and the generator \
                 cannot synthesize it",
            )),
        };
    };
    let target = entry.id.clone();
    let preservation_eligible = resolved.source_preservation_eligible();
    if resolved.preserves_source() {
        if let Preservation::Written { bytes, write_path } = preserve(input)? {
            return Ok(preserved_plan(input.ir, target, write_path, bytes));
        }
        return synthesized_plan(input, &target, None, preservation_eligible);
    }
    synthesized_plan(
        input,
        &target,
        resolved.displacement_message(),
        preservation_eligible,
    )
}

enum Preservation {
    Written {
        bytes: Vec<u8>,
        write_path: WritePath,
    },
    Declined,
}

fn preserve(input: EncodeInput<'_>) -> Result<Preservation, CodecError> {
    let Some(record) = input
        .fidelity
        .and_then(|sidecar| sidecar.retained_record(ids::FILE_SOURCE_IMAGE_ID))
    else {
        return Ok(Preservation::Declined);
    };
    let Some(data) = record.data.as_deref() else {
        return Err(CodecError::Malformed(
            "retained F3D source image has no bytes".into(),
        ));
    };
    let mut bytes = Vec::new();
    let write_path = F3dCodec::write_preserved_bytes(
        input.ir,
        data,
        record.byte_len,
        &record.sha256,
        &mut bytes,
    )?;
    Ok(Preservation::Written { bytes, write_path })
}

fn preserved_plan(
    ir: &CadIr,
    target: DialectId,
    write_path: WritePath,
    bytes: Vec<u8>,
) -> ExportPlan {
    ExportPlan::buffered(
        report(
            ir,
            target,
            FidelityResolution::Replayed,
            write_path,
            Vec::new(),
        ),
        bytes,
    )
}

fn synthesized_plan(
    input: EncodeInput<'_>,
    target: &DialectId,
    displacement: Option<String>,
    preservation_eligible: bool,
) -> Result<ExportPlan, CodecError> {
    let mut bytes = Vec::new();
    super::generate::write_new(input.ir, &mut bytes)?;
    let source_available = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(ids::FILE_SOURCE_IMAGE_ID))
        .is_some();
    let fidelity = if preservation_eligible && !source_available {
        FidelityResolution::Degraded {
            reason: "preserved F3D source image is unavailable".into(),
        }
    } else if input.fidelity.is_some() {
        FidelityResolution::NotConsumed
    } else {
        FidelityResolution::NotProvided
    };
    let mut losses: Vec<_> = (preservation_eligible && !source_available)
        .then(|| {
            F3dLossCode::SourcePreservedImageUnavailable
                .note("preserved F3D source image is unavailable; regenerated from IR")
        })
        .into_iter()
        .collect();
    if let Some(message) = displacement {
        losses.push(F3dLossCode::SourceDialectDisplaced.note(message));
    }
    Ok(ExportPlan::buffered(
        report(
            input.ir,
            target.clone(),
            fidelity,
            WritePath::Synthesized,
            losses,
        ),
        bytes,
    ))
}

fn report(
    ir: &CadIr,
    target: DialectId,
    fidelity: FidelityResolution,
    write_path: WritePath,
    losses: Vec<cadmpeg_ir::LossNote>,
) -> ExportReport {
    ExportReport::native(
        target,
        cadmpeg_ir::EntityCensus {
            basis: cadmpeg_ir::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        fidelity,
        write_path,
        losses,
        vec![
            match write_path {
                WritePath::VerbatimReplay => "preserved source container replayed verbatim",
                WritePath::Patched => "preserved source container replayed with semantic patches",
                WritePath::Synthesized => "source container regenerated from IR",
            }
            .into(),
            "entity counts are derived from the IR".into(),
        ],
    )
}
