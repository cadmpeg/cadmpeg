// SPDX-License-Identifier: Apache-2.0
//! F3D target resolution, preservation dispatch, and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{Consumption, EncodeInput, ExportBody, ResolvedWrite};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::WritePath;

use crate::loss::F3dLossCode;
use crate::{ids, F3dCodec};

/// Plan the export the resolved request names.
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
///
/// The sealed encoder stamps the target identity and maps
/// [`Consumption`] onto the report; this body states only what was done.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    if target.entry().is_none() {
        return match preserve(input)? {
            Preservation::Written { bytes, write_path } => {
                Ok(preserved_body(input.ir, write_path, bytes))
            }
            Preservation::Declined => Err(target.unavailable(
                "its retained source image is unavailable for preservation and the generator \
                 cannot synthesize it",
            )),
        };
    }
    let preservation_eligible = target.source_preservation_eligible();
    if target.preserves_source() {
        if let Preservation::Written { bytes, write_path } = preserve(input)? {
            return Ok(preserved_body(input.ir, write_path, bytes));
        }
        return synthesized_body(input, None, preservation_eligible);
    }
    synthesized_body(input, target.displacement_message(), preservation_eligible)
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

fn preserved_body(ir: &CadIr, write_path: WritePath, bytes: Vec<u8>) -> ExportBody {
    body(ir, Consumption::Replayed, write_path, Vec::new(), bytes)
}

fn synthesized_body(
    input: EncodeInput<'_>,
    displacement: Option<String>,
    preservation_eligible: bool,
) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    super::generate::write_new(input.ir, &mut bytes)?;
    let source_available = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(ids::FILE_SOURCE_IMAGE_ID))
        .is_some();
    let consumption = if preservation_eligible && !source_available {
        Consumption::Degraded {
            reason: "preserved F3D source image is unavailable".into(),
        }
    } else {
        Consumption::NotConsumed
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
    Ok(body(
        input.ir,
        consumption,
        WritePath::Synthesized,
        losses,
        bytes,
    ))
}

fn body(
    ir: &CadIr,
    consumption: Consumption,
    write_path: WritePath,
    losses: Vec<cadmpeg_ir::LossNote>,
    bytes: Vec<u8>,
) -> ExportBody {
    ExportBody {
        bytes,
        census: cadmpeg_ir::EntityCensus {
            basis: cadmpeg_ir::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        write_path,
        losses,
        notes: vec![
            match write_path {
                WritePath::VerbatimReplay => "preserved source container replayed verbatim",
                WritePath::Patched => "preserved source container replayed with semantic patches",
                WritePath::Synthesized => "source container regenerated from IR",
            }
            .into(),
            "entity counts are derived from the IR".into(),
        ],
        consumption,
    }
}
