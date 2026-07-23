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
        .sum();
    let design_field_count = native
        .design_objects
        .iter()
        .map(|object| object.fields.len())
        .sum();
    let classified_design_object_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_class.is_some() || !object.field_classes.is_empty())
        .count();
    let design_object_reference_count = native
        .design_objects
        .iter()
        .map(|object| object.object_references.len())
        .sum();
    let design_object_owner_link_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_design_object.is_some())
        .count();
    let unresolved_design_owner_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_none())
        .count();
    let value_field_count = native
        .value_blocks
        .iter()
        .map(|block| block.fields.len())
        .sum();
    let value_selection_count = native
        .value_blocks
        .iter()
        .map(|block| block.schema_selections.len())
        .sum();
    report.coverage.extend([
        (
            "decoded_object_graph_count".to_string(),
            native.object_graphs.len(),
        ),
        (
            "decoded_object_record_count".to_string(),
            object_record_count,
        ),
        (
            "decoded_design_object_count".to_string(),
            native.design_objects.len(),
        ),
        ("decoded_design_field_count".to_string(), design_field_count),
        (
            "classified_design_object_count".to_string(),
            classified_design_object_count,
        ),
        (
            "decoded_design_object_reference_count".to_string(),
            design_object_reference_count,
        ),
        (
            "decoded_design_object_owner_link_count".to_string(),
            design_object_owner_link_count,
        ),
        (
            "unresolved_design_owner_count".to_string(),
            unresolved_design_owner_count,
        ),
        (
            "decoded_value_block_count".to_string(),
            native.value_blocks.len(),
        ),
        ("decoded_value_field_count".to_string(), value_field_count),
        (
            "decoded_value_schema_selection_count".to_string(),
            value_selection_count,
        ),
        (
            "transferred_feature_count".to_string(),
            ir.model.features.len(),
        ),
        (
            "transferred_parameter_count".to_string(),
            ir.model.parameters.len(),
        ),
        (
            "transferred_sketch_count".to_string(),
            ir.model.sketches.len(),
        ),
        (
            "transferred_sketch_constraint_count".to_string(),
            ir.model.sketch_constraints.len(),
        ),
        (
            "transferred_configuration_count".to_string(),
            ir.model.configurations.len(),
        ),
    ]);
    if object_record_count != 0 {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Blocking,
            message: format!(
                "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), {design_object_owner_link_count} structural owner link(s), and {design_object_reference_count} inter-object reference(s); {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; neutral features, parameters, sketch geometry, constraints, configurations, and re-derivable history remain unresolved.",
                native.design_objects.len(),
            ),
            provenance: None,
        });
    }
    if !native.value_blocks.is_empty() {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::AttributesNotTransferred,
            category: LossCategory::Attribute,
            severity: Severity::Warning,
            message: format!(
                "CATIA native data retains {} visualization value block(s), {value_field_count} encoded field(s), and {value_selection_count} schema-selected presentation value(s); neutral visualization and display-property bindings remain unresolved.",
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
