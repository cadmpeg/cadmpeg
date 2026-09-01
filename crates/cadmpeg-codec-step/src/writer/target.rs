// SPDX-License-Identifier: Apache-2.0
//! STEP target resolution and export reporting.

use cadmpeg_core::dialect::DialectId;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{resolve_write_request, EncodeInput, ExportPlan, TargetRequest};
use cadmpeg_ir::{ExportReport, FidelityResolution, WritePath};

use crate::export::write_step_outcome;
use crate::loss::StepLossCode;
use crate::options::StepSchema;
use crate::StepCodec;

/// Why this writer cannot reproduce a source schema outside
/// [`StepSchema::TARGETS`].
///
/// STEP has no retained-image path, and every schema this writer emits stamps
/// object-identifier arcs, so an edition-unspecified or unrecognized
/// declaration cannot be written back.
const OFF_CATALOG_SOURCE_REASON: &str =
    "the semantic writer cannot synthesize it, and writing another schema would change what the \
     file declares; name a target to choose one";

/// Resolve the request against the source and synthesize the selected schema.
pub(crate) fn plan(
    codec: &StepCodec,
    input: EncodeInput<'_>,
    request: TargetRequest<'_>,
) -> Result<ExportPlan, CodecError> {
    let resolved = resolve_write_request(
        input.ir,
        request,
        crate::dialect::FORMAT,
        StepSchema::TARGETS,
    )?;
    let Some(entry) = resolved.catalog_entry() else {
        return Err(resolved.unavailable(OFF_CATALOG_SOURCE_REASON));
    };
    let schema = StepSchema::from_catalog_entry(entry);
    let mut bytes = Vec::new();
    let outcome = write_step_outcome(input.ir, &mut bytes, schema, &codec.options)
        .map_err(CodecError::from)?;
    let target = DialectId::pinned(schema.target());
    let mut losses = outcome.losses;
    if let Some(source) = resolved.displaced_source() {
        losses.push(StepLossCode::SourceDialectDisplaced.note(
            cadmpeg_ir::codec::source_dialect_displaced_message(source, &target),
        ));
    }
    let report = ExportReport::native(
        target,
        outcome.census,
        if input.fidelity.is_some() {
            FidelityResolution::NotConsumed
        } else {
            FidelityResolution::NotProvided
        },
        WritePath::Synthesized,
        losses,
        outcome.notes,
    );
    Ok(ExportPlan::buffered(report, bytes))
}
