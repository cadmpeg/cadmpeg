// SPDX-License-Identifier: Apache-2.0
//! High-level CATPart-to-IR decoding.
//!
//! [`decode`] scans the container, selects a decoder from the identified storage
//! variant, and returns the transferred model with a [`DecodeReport`]. The
//! per-family pipelines live in `families/*/decode.rs`; this module is the
//! orchestrator: container scan, the ordered route table in [`crate::families`],
//! the metadata fallback, and the `Codec`-facing glue (native side-channel and
//! result assembly).
//!
//! Partial paths preserve the reconstructed B-rep stream or complete file as an
//! [`UnknownRecord`]. Their report identifies unresolved model layers.

use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::SourceFidelity;

use crate::assemble::{build_container_report, build_metadata_ir};
use crate::container::{self, ContainerScan};
use crate::families;
use crate::native::CatiaNative;

/// Decodes a `.CATPart` reader into an IR document and decode report.
///
/// When [`DecodeOptions::container_only`] is set, the result contains source
/// metadata and container diagnostics without entity decoding.
///
/// Otherwise each route in [`crate::families::ROUTES`] whose applicability
/// predicate accepts the scanned variant is tried in table order; the first to
/// return a model wins, a `None` falls through to the next applicable route, and
/// exhausting the table yields the metadata-only fallback.
pub fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<DecodeResult, CodecError> {
    let scan = container::scan_bytes(root.window().to_vec());

    if ctx.container_only() {
        let (ir, annotations, unknowns) = build_metadata_ir(&scan);
        let report = build_container_report(&scan, true);
        return decode_result(ir, report, annotations, &unknowns);
    }

    for route in families::ROUTES {
        if (route.applicable)(scan.variant) {
            if let Some(out) = (route.decode)(&scan) {
                return finish_decode(&scan, out.ir, out.report, out.annotations, &out.unknowns);
            }
        }
    }

    let (ir, annotations, unknowns) = build_metadata_ir(&scan);
    let report = build_container_report(&scan, false);
    finish_decode(&scan, ir, report, annotations, &unknowns)
}

fn finish_decode(
    scan: &ContainerScan,
    mut ir: CadIr,
    mut report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: &[UnknownRecord],
) -> Result<DecodeResult, CodecError> {
    let native = CatiaNative::decode(&scan.data);
    let object_record_count = native
        .object_graphs
        .iter()
        .map(|graph| graph.records.len())
        .sum::<usize>();
    let value_selection_count = native
        .value_blocks
        .iter()
        .map(|block| block.schema_selections.len())
        .sum::<usize>();
    if object_record_count != 0 || !native.value_blocks.is_empty() {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Blocking,
            message: format!(
                "CATIA native data retains {} design object(s), {object_record_count} object-graph field record(s), {} value block(s), and {value_selection_count} schema-selected value(s); neutral features, parameters, sketch geometry, and history dependencies remain unresolved.",
                native.design_objects.len(),
                native.value_blocks.len(),
            ),
            provenance: None,
        });
    }
    native.store_owned(ir.native.namespace_mut("catia"))?;
    decode_result(ir, report, annotations, unknowns)
}

fn decode_result(
    mut ir: CadIr,
    report: DecodeReport,
    annotations: cadmpeg_ir::Annotations,
    unknowns: &[UnknownRecord],
) -> Result<DecodeResult, CodecError> {
    let mut source_fidelity = SourceFidelity {
        annotations,
        ..SourceFidelity::default()
    };
    source_fidelity.attach_native_unknown_records(&mut ir, "catia", unknowns)?;
    Ok(DecodeResult::with_source_fidelity(
        ir,
        report,
        source_fidelity,
    ))
}
