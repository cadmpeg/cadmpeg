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
use crate::design_feature;
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
    let design_feature_transfer = design_feature::transfer_design_features(&mut ir, &native);
    let formula_transfer = formula::transfer_parameters(
        &mut ir,
        &native,
        &design_feature_transfer.features_by_design_object,
        &mut annotations,
    );
    design_feature::project_feature_source_content(&mut ir, &native);
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
    let design_unowned_field_relation_count = native
        .design_objects
        .iter()
        .flat_map(|object| &object.relations)
        .filter(|relation| relation.target_design_object.is_none())
        .count();
    let design_same_object_relation_count = native
        .design_objects
        .iter()
        .map(|object| {
            object
                .relations
                .iter()
                .filter(|relation| {
                    relation.target_design_object.as_deref() == Some(object.id.as_str())
                })
                .count()
        })
        .sum();
    let design_reflexive_field_relation_count = native
        .design_objects
        .iter()
        .flat_map(|object| &object.relations)
        .filter(|relation| relation.source_field == relation.target_field)
        .count();
    let design_object_owner_link_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_design_object.is_some())
        .count();
    let legacy_entity_identity_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.identities.len())
        .sum();
    let legacy_text_field_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.text_fields.len())
        .sum();
    let legacy_role_text_field_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.text_fields)
        .filter(|field| field.role.is_some())
        .count();
    let legacy_relation_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.relations.len())
        .sum();
    let legacy_parameter_relation_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.relations)
        .filter(|relation| relation.parameter_entity_id.is_some())
        .count();
    let legacy_type_descriptor_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.type_descriptors.len())
        .sum();
    let legacy_literal_type_descriptor_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.type_descriptors)
        .filter(|descriptor| {
            matches!(
                &descriptor.value,
                crate::native::CatiaLegacyTypeValue::Name { .. }
            )
        })
        .count();
    let legacy_scalar_value_count = native
        .legacy_entity_runs
        .iter()
        .map(|run| run.scalar_values.len())
        .sum();
    let legacy_named_scalar_value_count = native
        .legacy_entity_runs
        .iter()
        .flat_map(|run| &run.scalar_values)
        .filter(|value| value.name.is_some())
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
    let numeric_entity_value_tuple_count = native
        .entity_records
        .iter()
        .filter(|record| record.numeric_tuple.is_some())
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
    let constraint_range_count = native
        .entity_records
        .iter()
        .filter(|record| record.constraint_range.is_some())
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
    let transferred_sketch_declaration_records = design_feature_transfer
        .declaration_records
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let transferred_sketch_placement_records = design_feature_transfer
        .placement_records
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let transferred_principal_plane_records = design_feature_transfer
        .principal_plane_records
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let transferred_design_feature_records = design_feature_transfer
        .consumed_records()
        .intersection(&structurally_owned_records)
        .cloned()
        .collect::<HashSet<_>>();
    let transferred_design_records = transferred_formula_design_records
        .union(&transferred_design_feature_records)
        .cloned()
        .collect::<HashSet<_>>();
    let unresolved_object_record_count =
        object_record_count.saturating_sub(transferred_design_records.len());
    let unresolved_design_object_count = native
        .design_objects
        .iter()
        .filter(|object| {
            object
                .fields
                .iter()
                .any(|field| !transferred_design_records.contains(field))
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
            "decoded_consolidated_line_profile_count".to_string(),
            native.consolidated_line_profiles.len(),
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
            "decoded_zero_entity_support_run_count".to_string(),
            native.zero_entity_support_runs.len(),
        ),
        (
            "decoded_zero_entity_support_occurrence_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .map(|run| run.supports.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_uv_endpoint_pair_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.uv_endpoints.is_some())
                .count(),
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
            "decoded_design_unowned_field_relation_count".to_string(),
            design_unowned_field_relation_count,
        ),
        (
            "decoded_design_same_object_relation_count".to_string(),
            design_same_object_relation_count,
        ),
        (
            "decoded_design_reflexive_field_relation_count".to_string(),
            design_reflexive_field_relation_count,
        ),
        (
            "decoded_design_object_owner_link_count".to_string(),
            design_object_owner_link_count,
        ),
        (
            "decoded_legacy_entity_run_count".to_string(),
            native.legacy_entity_runs.len(),
        ),
        (
            "decoded_legacy_entity_identity_count".to_string(),
            legacy_entity_identity_count,
        ),
        (
            "decoded_legacy_text_field_count".to_string(),
            legacy_text_field_count,
        ),
        (
            "decoded_legacy_role_text_field_count".to_string(),
            legacy_role_text_field_count,
        ),
        (
            "decoded_legacy_relation_count".to_string(),
            legacy_relation_count,
        ),
        (
            "decoded_legacy_parameter_relation_count".to_string(),
            legacy_parameter_relation_count,
        ),
        (
            "decoded_legacy_type_descriptor_count".to_string(),
            legacy_type_descriptor_count,
        ),
        (
            "decoded_legacy_literal_type_descriptor_count".to_string(),
            legacy_literal_type_descriptor_count,
        ),
        (
            "decoded_legacy_scalar_value_count".to_string(),
            legacy_scalar_value_count,
        ),
        (
            "decoded_legacy_named_scalar_value_count".to_string(),
            legacy_named_scalar_value_count,
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
            "decoded_numeric_entity_value_tuple_count".to_string(),
            numeric_entity_value_tuple_count,
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
            "decoded_constraint_range_count".to_string(),
            constraint_range_count,
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
            "transferred_legacy_parameter_count".to_string(),
            formula_transfer.legacy_parameter_count,
        ),
        (
            "transferred_legacy_selector_parameter_count".to_string(),
            formula_transfer.legacy_selector_parameter_count,
        ),
        (
            "transferred_legacy_formula_count".to_string(),
            formula_transfer.legacy_formula_count,
        ),
        (
            "transferred_owned_parameter_count".to_string(),
            formula_transfer.owned_parameter_count,
        ),
        (
            "transferred_formula_design_record_count".to_string(),
            transferred_formula_design_records.len(),
        ),
        (
            "transferred_sketch_declaration_record_count".to_string(),
            transferred_sketch_declaration_records.len(),
        ),
        (
            "transferred_sketch_placement_record_count".to_string(),
            transferred_sketch_placement_records.len(),
        ),
        (
            "transferred_principal_plane_record_count".to_string(),
            transferred_principal_plane_records.len(),
        ),
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
    if !native.consolidated_line_profiles.is_empty() {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{} consolidated line-profile record(s) retain their exact origin, unit \
                 direction, metric scalar, and parameter interval, but their owner bindings and \
                 metric-scalar parameter semantics remain unresolved.",
                native.consolidated_line_profiles.len(),
            ),
            provenance: None,
        });
    }
    if !native.zero_entity_support_runs.is_empty() {
        let support_count = native
            .zero_entity_support_runs
            .iter()
            .map(|run| run.supports.len())
            .sum::<usize>();
        let endpoint_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.uv_endpoints.is_some())
            .count();
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "{} zero-entity surface-support run(s) retain {support_count} face-local \
                 occurrence(s), including {endpoint_count} with exact UV endpoint pairs; the \
                 oriented-use and vertex-incidence registries remain unresolved.",
                native.zero_entity_support_runs.len(),
            ),
            provenance: None,
        });
    }
    if unresolved_object_record_count != 0 {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Blocking,
            message: format!(
                "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), {entity_value_field_count} entity-value field(s), {entity_value_schema_selection_count} entity-value schema selection(s), {numeric_entity_value_tuple_count} complete numeric entity-value tuple(s), {numeric_entity_value_packet_count} embedded numeric entity-value packet(s), {compact_entity_value_packet_count} compact entity-value packet(s), {layout_entity_value_packet_count} layout entity-value packet(s), {scalar_entity_suffix_value_count} scalar entity-suffix value(s), {unset_entity_suffix_value_count} unset entity-suffix value(s), {atom_entity_suffix_value_count} atom entity-suffix value(s), {separator_entity_suffix_value_count} separator entity-suffix value(s), {schema_selected_atom_entity_suffix_value_count} schema-selected atom value(s), {schema_selected_evaluation_entity_suffix_value_count} schema-selected evaluation(s), {schema_selected_separator_entity_suffix_value_count} schema-selected separator(s), {schema_selected_schema_entity_suffix_value_count} schema-selected schema value(s), {schema_selected_entity_suffix_value_count} suffix value(s) with resolved schema selectors, {wide_prefix_entity_suffix_value_count} suffix value(s) with multi-byte prefix atoms, {control_entity_suffix_value_count} control entity-suffix value(s), {relation_expression_count} complete relation expression(s), {parameter_value_count} complete named parameter value(s), {constraint_range_count} complete constraint-range value(s), {definition_value_count} definition-bound suffix value(s), including {owned_definition_value_count} assigned to design objects and {unowned_definition_value_count} without a resolved owner, {formula_relation_count} complete formula relation(s), {formula_parameter_dependency_count} formula parameter dependency link(s), {repeated_reference_suffix_count} repeated-reference suffix(es), {repeated_reference_schema_selection_count} repeated-reference schema selection(s), {definition_schema_selection_count} definition-schema selection(s), {design_object_owner_link_count} structural owner link(s), and {design_object_relation_count} exact outbound design-field relation occurrence(s), including {design_same_object_relation_count} within one design object, {design_reflexive_field_relation_count} reflexive field occurrence(s), and {design_unowned_field_relation_count} to fields without owner groups; {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; {} typed formula parameter(s), {} exact formula, expression, or parameter field record(s), {} exact principal-plane field record(s), {} exact sketch declaration field record(s), and {} exact sketch placement field record(s) transferred, while {unresolved_object_record_count} field record(s) across {unresolved_design_object_count} design object(s), neutral features, other parameters, remaining sketch geometry, constraints, configurations, and re-derivable history remain unresolved.",
                native.design_objects.len(),
                formula_transfer.formula_parameter_count,
                transferred_formula_design_records.len(),
                transferred_principal_plane_records.len(),
                transferred_sketch_declaration_records.len(),
                transferred_sketch_placement_records.len(),
            ),
            provenance: None,
        });
    }
    if !native.legacy_entity_runs.is_empty() {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Blocking,
            message: format!(
                "CATIA native data retains {} legacy design run(s) with {legacy_entity_identity_count} source-ordered entity identity marker(s), {legacy_text_field_count} complete schema text field(s), including {legacy_role_text_field_count} schema-role binding(s), {legacy_relation_count} typed expression/signature pair(s), including {legacy_parameter_relation_count} with exact parameter identities, {legacy_type_descriptor_count} type descriptor(s), including {legacy_literal_type_descriptor_count} literal name(s), and {legacy_scalar_value_count} typed scalar evaluation(s), including {legacy_named_scalar_value_count} named scalar(s); {} uniquely named, literal-typed numeric parameter(s), including {} resolved through descriptor selectors, and {} closed zero-input formula(s) transferred, while remaining inter-marker fields, unbound relation ownership and parameters, unresolved selector types, feature semantics, and feature history remain unresolved.",
                native.legacy_entity_runs.len(),
                formula_transfer.legacy_parameter_count,
                formula_transfer.legacy_selector_parameter_count,
                formula_transfer.legacy_formula_count,
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
