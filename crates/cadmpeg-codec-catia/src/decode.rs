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

use std::collections::HashSet;

use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, SourceFidelity};

use crate::assemble::{build_container_report, build_metadata_ir};
use crate::container::{self, ContainerScan};
use crate::entity_table;
use crate::families;
use crate::formula;
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
    mut annotations: Annotations,
    unknowns: &[UnknownRecord],
) -> Result<DecodeResult, CodecError> {
    let native = CatiaNative::decode(&scan.data);
    let formula_transfer = formula::transfer_parameters(&mut ir, &native, &mut annotations);
    let object_record_count: usize = native
        .object_graphs
        .iter()
        .map(|graph| graph.records.len())
        .sum();
    let resolved_storage_record_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| record.storage_record.is_some())
        .count();
    let unresolved_storage_record_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| record.storage_ref.is_some() && record.storage_record.is_none())
        .count();
    let repeated_reference_suffix_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| record.repeated_reference_suffix.is_some())
        .count();
    let repeated_reference_schema_selection_count = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .filter(|record| record.repeated_reference_schema_selection.is_some())
        .count();
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
    let design_object_relation_count = native
        .design_objects
        .iter()
        .map(|object| object.relations.len())
        .sum();
    let design_object_owner_link_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_design_object.is_some())
        .count();
    let definition_schema_selection_count = native
        .entity_records
        .iter()
        .map(|record| record.definition_schema_selections.len())
        .sum();
    let entity_value_field_count = native
        .entity_records
        .iter()
        .map(|record| record.value_fields.len())
        .sum();
    let entity_value_schema_selection_count = native
        .entity_records
        .iter()
        .map(|record| record.value_schema_selections.len())
        .sum();
    let compact_entity_value_packet_count = native
        .entity_records
        .iter()
        .flat_map(|record| &record.value_packets)
        .filter(|packet| matches!(packet, entity_table::EntityValuePacket::Compact { .. }))
        .count();
    let numeric_entity_value_packet_count = native
        .entity_records
        .iter()
        .flat_map(|record| &record.value_packets)
        .filter(|packet| matches!(packet, entity_table::EntityValuePacket::Numeric { .. }))
        .count();
    let layout_entity_value_packet_count = native
        .entity_records
        .iter()
        .flat_map(|record| &record.value_packets)
        .filter(|packet| matches!(packet, entity_table::EntityValuePacket::Layout { .. }))
        .count();
    let relation_expression_count = native
        .entity_records
        .iter()
        .filter(|record| record.relation_expression.is_some())
        .count();
    let parameter_value_count = native
        .entity_records
        .iter()
        .filter(|record| record.parameter_value.is_some())
        .count();
    let definition_value_count = native
        .entity_records
        .iter()
        .filter(|record| record.definition_value.is_some())
        .count();
    let owned_definition_value_count = native
        .design_objects
        .iter()
        .map(|object| object.definition_values.len())
        .sum::<usize>();
    let unowned_definition_value_count = definition_value_count
        .checked_sub(owned_definition_value_count)
        .expect("owned CATIA definition values are a subset of decoded values");
    let formula_relation_count = native
        .entity_records
        .iter()
        .filter(|record| record.formula_relation.is_some())
        .count();
    let formula_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .map(|formula| formula.parameter_dependencies.len())
        .sum();
    let scalar_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Evaluation {
                        evaluation: crate::native::CatiaEntityEvaluation::Scalar { .. },
                        ..
                    }
                )
            })
        })
        .count();
    let unset_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Evaluation {
                        evaluation: crate::native::CatiaEntityEvaluation::Unset,
                        ..
                    }
                )
            })
        })
        .count();
    let control_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::ControlE8
                )
            })
        })
        .count();
    let separator_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Separator37
                )
            })
        })
        .count();
    let atom_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.suffix_value.as_ref().is_some_and(|value| {
                matches!(
                    value.payload,
                    crate::native::CatiaEntitySuffixPayload::Atom { .. }
                )
            })
        })
        .count();
    let (
        schema_selected_atom_entity_suffix_value_count,
        schema_selected_evaluation_entity_suffix_value_count,
        schema_selected_separator_entity_suffix_value_count,
        schema_selected_schema_entity_suffix_value_count,
    ) = native.entity_records.iter().fold(
        (0, 0, 0, 0),
        |(atoms, evaluations, separators, schemas), record| {
            let Some(crate::native::CatiaEntitySuffixPayload::SchemaSelected { value, .. }) =
                record.suffix_value.as_ref().map(|suffix| &suffix.payload)
            else {
                return (atoms, evaluations, separators, schemas);
            };
            match value {
                crate::native::CatiaEntitySuffixSelectedValue::Atom { .. } => {
                    (atoms + 1, evaluations, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::Evaluation { .. } => {
                    (atoms, evaluations + 1, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
                    (atoms, evaluations, separators + 1, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector { .. } => {
                    (atoms, evaluations, separators, schemas + 1)
                }
            }
        },
    );
    let schema_selected_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| record.suffix_schema_selection.is_some())
        .count();
    let wide_prefix_entity_suffix_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record
                .suffix_value
                .as_ref()
                .is_some_and(|value| value.prefix_atom_widths.iter().any(|width| *width > 1))
        })
        .count();
    let unresolved_design_owner_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_none())
        .count();
    let structurally_owned_records = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_some())
        .flat_map(|object| object.fields.iter().cloned())
        .collect::<HashSet<_>>();
    let transferred_formula_design_records = formula_transfer
        .consumed_object_records
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let unresolved_object_record_count =
        object_record_count.saturating_sub(transferred_formula_design_records.len());
    let unresolved_design_object_count = native
        .design_objects
        .iter()
        .filter(|object| {
            object
                .fields
                .iter()
                .any(|field| !transferred_formula_design_records.contains(field))
        })
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
            "decoded_consolidated_circle_count".to_string(),
            native.consolidated_circles.len(),
        ),
        (
            "decoded_consolidated_class61_record_count".to_string(),
            native.consolidated_class61_records.len(),
        ),
        (
            "decoded_consolidated_cone_count".to_string(),
            native.consolidated_cones.len(),
        ),
        (
            "decoded_consolidated_cylinder_count".to_string(),
            native.consolidated_cylinders.len(),
        ),
        (
            "decoded_consolidated_group_count".to_string(),
            native.consolidated_groups.len(),
        ),
        (
            "decoded_consolidated_parameter_point_count".to_string(),
            native.consolidated_parameter_points.len(),
        ),
        (
            "decoded_consolidated_pcurve_count".to_string(),
            native.consolidated_pcurves.len(),
        ),
        (
            "decoded_consolidated_reference_list_count".to_string(),
            native.consolidated_reference_lists.len(),
        ),
        (
            "decoded_consolidated_revolution_count".to_string(),
            native.consolidated_revolutions.len(),
        ),
        (
            "decoded_consolidated_sphere_count".to_string(),
            native.consolidated_spheres.len(),
        ),
        (
            "decoded_consolidated_torus_count".to_string(),
            native.consolidated_tori.len(),
        ),
        (
            "decoded_object_graph_count".to_string(),
            native.object_graphs.len(),
        ),
        (
            "decoded_object_record_count".to_string(),
            object_record_count,
        ),
        (
            "decoded_storage_record_link_count".to_string(),
            resolved_storage_record_count,
        ),
        (
            "unresolved_storage_record_count".to_string(),
            unresolved_storage_record_count,
        ),
        (
            "decoded_repeated_reference_suffix_count".to_string(),
            repeated_reference_suffix_count,
        ),
        (
            "decoded_repeated_reference_schema_selection_count".to_string(),
            repeated_reference_schema_selection_count,
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
            "decoded_design_object_relation_count".to_string(),
            design_object_relation_count,
        ),
        (
            "decoded_design_object_owner_link_count".to_string(),
            design_object_owner_link_count,
        ),
        (
            "decoded_definition_schema_selection_count".to_string(),
            definition_schema_selection_count,
        ),
        (
            "decoded_entity_value_field_count".to_string(),
            entity_value_field_count,
        ),
        (
            "decoded_entity_value_schema_selection_count".to_string(),
            entity_value_schema_selection_count,
        ),
        (
            "decoded_numeric_entity_value_packet_count".to_string(),
            numeric_entity_value_packet_count,
        ),
        (
            "decoded_compact_entity_value_packet_count".to_string(),
            compact_entity_value_packet_count,
        ),
        (
            "decoded_layout_entity_value_packet_count".to_string(),
            layout_entity_value_packet_count,
        ),
        (
            "decoded_relation_expression_count".to_string(),
            relation_expression_count,
        ),
        (
            "decoded_parameter_value_count".to_string(),
            parameter_value_count,
        ),
        (
            "decoded_definition_value_count".to_string(),
            definition_value_count,
        ),
        (
            "decoded_owned_definition_value_count".to_string(),
            owned_definition_value_count,
        ),
        (
            "unresolved_definition_value_owner_count".to_string(),
            unowned_definition_value_count,
        ),
        (
            "decoded_formula_relation_count".to_string(),
            formula_relation_count,
        ),
        (
            "decoded_formula_parameter_dependency_count".to_string(),
            formula_parameter_dependency_count,
        ),
        (
            "decoded_scalar_entity_suffix_value_count".to_string(),
            scalar_entity_suffix_value_count,
        ),
        (
            "decoded_unset_entity_suffix_value_count".to_string(),
            unset_entity_suffix_value_count,
        ),
        (
            "decoded_control_entity_suffix_value_count".to_string(),
            control_entity_suffix_value_count,
        ),
        (
            "decoded_separator_entity_suffix_value_count".to_string(),
            separator_entity_suffix_value_count,
        ),
        (
            "decoded_atom_entity_suffix_value_count".to_string(),
            atom_entity_suffix_value_count,
        ),
        (
            "decoded_schema_selected_atom_entity_suffix_value_count".to_string(),
            schema_selected_atom_entity_suffix_value_count,
        ),
        (
            "decoded_schema_selected_evaluation_entity_suffix_value_count".to_string(),
            schema_selected_evaluation_entity_suffix_value_count,
        ),
        (
            "decoded_schema_selected_separator_entity_suffix_value_count".to_string(),
            schema_selected_separator_entity_suffix_value_count,
        ),
        (
            "decoded_schema_selected_schema_entity_suffix_value_count".to_string(),
            schema_selected_schema_entity_suffix_value_count,
        ),
        (
            "decoded_schema_selected_entity_suffix_value_count".to_string(),
            schema_selected_entity_suffix_value_count,
        ),
        (
            "decoded_wide_prefix_entity_suffix_value_count".to_string(),
            wide_prefix_entity_suffix_value_count,
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
            "transferred_formula_design_record_count".to_string(),
            transferred_formula_design_records.len(),
        ),
        ("transferred_sketch_design_record_count".to_string(), 0),
        (
            "unresolved_design_record_count".to_string(),
            unresolved_object_record_count,
        ),
        (
            "transferred_sketch_count".to_string(),
            ir.model.sketches.len(),
        ),
        (
            "transferred_sketch_entity_count".to_string(),
            ir.model.sketch_entities.len(),
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
    if unresolved_object_record_count != 0 {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Blocking,
            message: format!(
                "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), {entity_value_field_count} entity-value field(s), {entity_value_schema_selection_count} entity-value schema selection(s), {numeric_entity_value_packet_count} numeric entity-value packet(s), {compact_entity_value_packet_count} compact entity-value packet(s), {layout_entity_value_packet_count} layout entity-value packet(s), {scalar_entity_suffix_value_count} scalar entity-suffix value(s), {unset_entity_suffix_value_count} unset entity-suffix value(s), {atom_entity_suffix_value_count} atom entity-suffix value(s), {separator_entity_suffix_value_count} separator entity-suffix value(s), {schema_selected_atom_entity_suffix_value_count} schema-selected atom value(s), {schema_selected_evaluation_entity_suffix_value_count} schema-selected evaluation(s), {schema_selected_separator_entity_suffix_value_count} schema-selected separator(s), {schema_selected_schema_entity_suffix_value_count} schema-selected schema value(s), {schema_selected_entity_suffix_value_count} suffix value(s) with resolved schema selectors, {wide_prefix_entity_suffix_value_count} suffix value(s) with multi-byte prefix atoms, {control_entity_suffix_value_count} control entity-suffix value(s), {relation_expression_count} complete relation expression(s), {parameter_value_count} complete named parameter value(s), {definition_value_count} definition-bound suffix value(s), including {owned_definition_value_count} assigned to design objects and {unowned_definition_value_count} without a resolved owner, {formula_relation_count} complete formula relation(s), {formula_parameter_dependency_count} formula parameter dependency link(s), {repeated_reference_suffix_count} repeated-reference suffix(es), {repeated_reference_schema_selection_count} repeated-reference schema selection(s), {definition_schema_selection_count} definition-schema selection(s), {design_object_owner_link_count} structural owner link(s), and {design_object_relation_count} exact inter-object relation occurrence(s); {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; {} typed formula parameter(s), {} exact formula, expression, or parameter field record(s), and {} exact sketch field record(s) transferred, while {unresolved_object_record_count} field record(s) across {unresolved_design_object_count} design object(s), neutral features, other parameters, remaining sketch geometry, constraints, configurations, and re-derivable history remain unresolved.",
                native.design_objects.len(),
                formula_transfer.parameter_count,
                transferred_formula_design_records.len(),
                0,
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
    annotations: Annotations,
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
