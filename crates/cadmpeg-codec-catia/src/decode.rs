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

use std::collections::{HashMap, HashSet};

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
    let design_parallel_reference_table_count = native
        .design_objects
        .iter()
        .filter(|object| object.parallel_reference_table.is_some())
        .count();
    let design_parallel_reference_row_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .map(|table| table.rows.len())
        .sum();
    let design_parallel_reference_cell_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .map(|row| row.cells.len())
        .sum();
    let design_parallel_reference_classified_cell_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .flat_map(|row| &row.cells)
        .filter(|cell| cell.field_class.is_some())
        .count();
    let design_parallel_reference_classified_column_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.column_classes)
        .filter(|class| class.is_some())
        .count();
    let design_parallel_reference_schema_membership_count = native
        .design_objects
        .iter()
        .filter_map(|object| object.parallel_reference_table.as_ref())
        .flat_map(|table| &table.rows)
        .filter(|row| row.schema_member.is_some())
        .count();
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
    let (
        constraint_range_count,
        dimension_constraint_range_count,
        complex_constraint_range_count,
        evaluated_constraint_range_count,
        unset_constraint_range_count,
    ) = native
        .entity_records
        .iter()
        .filter_map(|record| record.constraint_range.as_ref())
        .fold(
            (0_usize, 0_usize, 0_usize, 0_usize, 0_usize),
            |(total, dimensions, complex, evaluated, unset), range| {
                let (dimensions, complex) = match range.framing {
                    crate::native::CatiaConstraintRangeFraming::DimensionB8
                    | crate::native::CatiaConstraintRangeFraming::DimensionC1 => {
                        (dimensions + 1, complex)
                    }
                    crate::native::CatiaConstraintRangeFraming::ComplexC9 => {
                        (dimensions, complex + 1)
                    }
                };
                let (evaluated, unset) = match range.evaluation {
                    crate::native::CatiaEntityEvaluation::Scalar { .. } => (evaluated + 1, unset),
                    crate::native::CatiaEntityEvaluation::Unset => (evaluated, unset + 1),
                };
                (total + 1, dimensions, complex, evaluated, unset)
            },
        );
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
    let (
        definition_chain_value_count,
        definition_chain_evaluation_count,
        evaluated_definition_chain_count,
        unset_definition_chain_count,
        definition_chain_atom_count,
        definition_chain_control_count,
        definition_chain_separator_count,
        definition_chain_schema_selector_count,
    ) = native
        .entity_records
        .iter()
        .filter_map(|record| record.definition_chain_value.as_ref())
        .fold(
            (
                0_usize, 0_usize, 0_usize, 0_usize, 0_usize, 0_usize, 0_usize, 0_usize,
            ),
            |(total, evaluations, evaluated, unset, atoms, controls, separators, schemas),
             value| {
                use crate::native::{
                    CatiaEntityEvaluation, CatiaEntitySuffixSchemaValue as Selected,
                };
                match &value.value {
                    Selected::Evaluation {
                        evaluation: CatiaEntityEvaluation::Scalar { .. },
                    } => (
                        total + 1,
                        evaluations + 1,
                        evaluated + 1,
                        unset,
                        atoms,
                        controls,
                        separators,
                        schemas,
                    ),
                    Selected::Evaluation {
                        evaluation: CatiaEntityEvaluation::Unset,
                    } => (
                        total + 1,
                        evaluations + 1,
                        evaluated,
                        unset + 1,
                        atoms,
                        controls,
                        separators,
                        schemas,
                    ),
                    Selected::Atom { .. } => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms + 1,
                        controls,
                        separators,
                        schemas,
                    ),
                    Selected::ControlE8 => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms,
                        controls + 1,
                        separators,
                        schemas,
                    ),
                    Selected::Separator37 => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms,
                        controls,
                        separators + 1,
                        schemas,
                    ),
                    Selected::SchemaSelector { .. } => (
                        total + 1,
                        evaluations,
                        evaluated,
                        unset,
                        atoms,
                        controls,
                        separators,
                        schemas + 1,
                    ),
                }
            },
        );
    let formula_relation_count = native
        .entity_records
        .iter()
        .filter(|record| record.formula_relation.is_some())
        .count();
    let resolved_formula_output_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .filter(|formula| formula.parameter.is_some())
        .count();
    let unresolved_formula_output_count = formula_relation_count - resolved_formula_output_count;
    let formula_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .map(|formula| formula.parameter_dependencies.len())
        .sum();
    let resolved_formula_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .flat_map(|formula| &formula.parameter_dependencies)
        .filter(|dependency| dependency.candidates.len() == 1)
        .count();
    let ambiguous_formula_parameter_dependency_count = native
        .entity_records
        .iter()
        .filter_map(|record| record.formula_relation.as_ref())
        .flat_map(|formula| &formula.parameter_dependencies)
        .filter(|dependency| dependency.candidates.len() > 1)
        .count();
    let unresolved_formula_parameter_dependency_count =
        formula_parameter_dependency_count - resolved_formula_parameter_dependency_count;
    let (
        escaped_word_entity_suffix_count,
        token_8149_entity_suffix_count,
        fixed_fe_f6_entity_suffix_count,
        paged_atom_state_01_entity_suffix_count,
    ) = native.entity_records.iter().fold(
        (0, 0, 0, 0),
        |(escaped, token, fixed, paged), record| match record.suffix_framing.as_ref() {
            Some(crate::native::CatiaEntitySuffixFraming::EscapedWord(_)) => {
                (escaped + 1, token, fixed, paged)
            }
            Some(crate::native::CatiaEntitySuffixFraming::Token8149) => {
                (escaped, token + 1, fixed, paged)
            }
            Some(crate::native::CatiaEntitySuffixFraming::FixedFeF6 { .. }) => {
                (escaped, token, fixed + 1, paged)
            }
            Some(crate::native::CatiaEntitySuffixFraming::PagedAtomState01 { .. }) => {
                (escaped, token, fixed, paged + 1)
            }
            None => (escaped, token, fixed, paged),
        },
    );
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
    let (control_e8_entity_suffix_value_count, control_e9_entity_suffix_value_count) = native
        .entity_records
        .iter()
        .fold((0, 0), |(e8, e9), record| {
            match record.suffix_value.as_ref().map(|value| &value.payload) {
                Some(crate::native::CatiaEntitySuffixPayload::ControlE8) => (e8 + 1, e9),
                Some(crate::native::CatiaEntitySuffixPayload::ControlE9) => (e8, e9 + 1),
                _ => (e8, e9),
            }
        });
    let control_entity_suffix_value_count =
        control_e8_entity_suffix_value_count + control_e9_entity_suffix_value_count;
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
        schema_selected_control_entity_suffix_value_count,
        schema_selected_separator_entity_suffix_value_count,
        schema_selected_schema_entity_suffix_value_count,
    ) = native.entity_records.iter().fold(
        (0, 0, 0, 0, 0),
        |(atoms, evaluations, controls, separators, schemas), record| {
            let Some(crate::native::CatiaEntitySuffixPayload::SchemaSelected { value, .. }) =
                record.suffix_value.as_ref().map(|suffix| &suffix.payload)
            else {
                return (atoms, evaluations, controls, separators, schemas);
            };
            match value {
                crate::native::CatiaEntitySuffixSelectedValue::Atom { .. } => {
                    (atoms + 1, evaluations, controls, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::Evaluation { .. } => {
                    (atoms, evaluations + 1, controls, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::ControlE8 => {
                    (atoms, evaluations, controls + 1, separators, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::Separator37 => {
                    (atoms, evaluations, controls, separators + 1, schemas)
                }
                crate::native::CatiaEntitySuffixSelectedValue::SchemaSelector { .. } => {
                    (atoms, evaluations, controls, separators, schemas + 1)
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
    let object_records_by_id = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let unassigned_owner_slot_count = object_records_by_id
        .values()
        .filter(|record| record.has_unassigned_owner())
        .count();
    let structurally_owned_records = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_some())
        .flat_map(|object| object.fields.iter().cloned())
        .collect::<HashSet<_>>();
    let structurally_owned_definition_chain_value_count = native
        .design_objects
        .iter()
        .filter(|object| object.owner_record.is_some())
        .map(|object| object.definition_chain_values.len())
        .sum::<usize>();
    let unowned_definition_chain_value_count = definition_chain_value_count
        .checked_sub(structurally_owned_definition_chain_value_count)
        .expect("owned CATIA definition-chain values are a subset of decoded values");
    let unassigned_definition_chain_value_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.definition_chain_value.is_some()
                && object_records_by_id
                    .get(record.object_record.as_str())
                    .is_some_and(|record| record.has_unassigned_owner())
        })
        .count();
    let structurally_owned_definition_chain_evaluation_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.definition_chain_value.as_ref().is_some_and(|value| {
                matches!(
                    &value.value,
                    crate::native::CatiaEntitySuffixSchemaValue::Evaluation { .. }
                )
            }) && structurally_owned_records.contains(&record.object_record)
        })
        .count();
    let unowned_definition_chain_evaluation_count = definition_chain_evaluation_count
        .checked_sub(structurally_owned_definition_chain_evaluation_count)
        .expect("owned CATIA definition-chain evaluations are a subset of decoded values");
    let unassigned_definition_chain_evaluation_count = native
        .entity_records
        .iter()
        .filter(|record| {
            record.definition_chain_value.as_ref().is_some_and(|value| {
                matches!(
                    &value.value,
                    crate::native::CatiaEntitySuffixSchemaValue::Evaluation { .. }
                )
            }) && object_records_by_id
                .get(record.object_record.as_str())
                .is_some_and(|record| record.has_unassigned_owner())
        })
        .count();
    let transferred_formula_design_records = formula_transfer
        .consumed_object_records
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
    let transferred_line_profile_count = ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            curve
                .id
                .0
                .starts_with("catia:consolidated:line-profile-curve#")
        })
        .count();
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
            "decoded_consolidated_cone_face_count".to_string(),
            native.consolidated_cone_faces.len(),
        ),
        (
            "decoded_consolidated_cone_face_parameter_point_count".to_string(),
            native
                .consolidated_cone_faces
                .iter()
                .map(|face| face.parameter_points.len())
                .sum(),
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
            "transferred_consolidated_line_profile_count".to_string(),
            transferred_line_profile_count,
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
            "transferred_consolidated_revolution_count".to_string(),
            ir.model
                .procedural_surfaces
                .iter()
                .filter(|surface| surface.id.0.starts_with("catia:standard:revolution#"))
                .count(),
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
            "decoded_zero_entity_edge_stride_count".to_string(),
            native.zero_entity_edge_strides.len(),
        ),
        (
            "decoded_zero_entity_edge_stride_allocation_count".to_string(),
            native
                .zero_entity_edge_strides
                .iter()
                .map(|stride| stride.allocations.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_face_bound_support_run_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter(|run| run.face.is_some())
                .count(),
        ),
        (
            "decoded_zero_entity_face_terminal_control_03_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .filter(|face| face.terminal_control == 0x03)
                .count(),
        ),
        (
            "decoded_zero_entity_face_terminal_control_05_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .filter(|face| face.terminal_control == 0x05)
                .count(),
        ),
        (
            "decoded_zero_entity_loop_terminal_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .map(|face| face.loop_terminals.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_loop_record_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .map(|face| face.loops.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_loop_class_41_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .filter(|loop_| loop_.loop_class == 0x41)
                .count(),
        ),
        (
            "decoded_zero_entity_loop_class_50_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .filter(|loop_| loop_.loop_class == 0x50)
                .count(),
        ),
        (
            "decoded_zero_entity_loop_class_c1_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .filter(|loop_| loop_.loop_class == 0xc1)
                .count(),
        ),
        (
            "decoded_zero_entity_forward_loop_member_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .flat_map(|loop_| &loop_.forward_senses)
                .filter(|sense| **sense)
                .count(),
        ),
        (
            "decoded_zero_entity_reversed_loop_member_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .flat_map(|loop_| &loop_.forward_senses)
                .filter(|sense| !**sense)
                .count(),
        ),
        (
            "decoded_zero_entity_oriented_loop_member_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.forward_senses.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_oriented_model_endpoint_pair_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.oriented_model_endpoints.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_bound_support_member_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.support_record_ordinals.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_bound_typed_loop_reference_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .filter_map(|run| run.face.as_ref())
                .flat_map(|face| &face.loops)
                .map(|loop_record| loop_record.typed_records.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_oriented_use_pair_count".to_string(),
            native.zero_entity_oriented_use_pairs.len(),
        ),
        (
            "decoded_zero_entity_oriented_use_count".to_string(),
            native
                .zero_entity_oriented_use_pairs
                .iter()
                .map(|pair| pair.uses.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_oriented_use_allocation_count".to_string(),
            native
                .zero_entity_oriented_use_pairs
                .iter()
                .flat_map(|pair| &pair.uses)
                .map(|use_| use_.allocations.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_endpoint_pair_candidate_count".to_string(),
            native.zero_entity_endpoint_pair_candidates.len(),
        ),
        (
            "decoded_zero_entity_endpoint_locus_candidate_count".to_string(),
            native.zero_entity_endpoint_locus_candidates.len(),
        ),
        (
            "decoded_zero_entity_record_count".to_string(),
            native.zero_entity_records.len(),
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
            "decoded_zero_entity_support_pcurve_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.pcurve.is_some())
                .count(),
        ),
        (
            "decoded_zero_entity_support_model_curve_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_curve.is_some())
                .count(),
        ),
        (
            "decoded_zero_entity_support_model_construction_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_curve_construction.is_some())
                .count(),
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
            "decoded_zero_entity_model_endpoint_pair_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_endpoints.is_some())
                .count(),
        ),
        (
            "decoded_zero_entity_model_midpoint_count".to_string(),
            native
                .zero_entity_support_runs
                .iter()
                .flat_map(|run| &run.supports)
                .filter(|support| support.model_midpoint.is_some())
                .count(),
        ),
        (
            "decoded_zero_entity_vertex_incidence_count".to_string(),
            native.zero_entity_vertex_incidences.len(),
        ),
        (
            "decoded_zero_entity_vertex_incidence_allocation_count".to_string(),
            native
                .zero_entity_vertex_incidences
                .iter()
                .map(|incidence| incidence.allocations.len())
                .sum(),
        ),
        (
            "decoded_zero_entity_vertex_owner_binding_count".to_string(),
            native
                .zero_entity_vertex_incidences
                .iter()
                .filter(|incidence| incidence.vertex_record.is_some())
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
            "decoded_design_parallel_reference_table_count".to_string(),
            design_parallel_reference_table_count,
        ),
        (
            "decoded_design_parallel_reference_row_count".to_string(),
            design_parallel_reference_row_count,
        ),
        (
            "decoded_design_parallel_reference_cell_count".to_string(),
            design_parallel_reference_cell_count,
        ),
        (
            "decoded_design_parallel_reference_classified_cell_count".to_string(),
            design_parallel_reference_classified_cell_count,
        ),
        (
            "decoded_design_parallel_reference_classified_column_count".to_string(),
            design_parallel_reference_classified_column_count,
        ),
        (
            "decoded_design_parallel_reference_schema_membership_count".to_string(),
            design_parallel_reference_schema_membership_count,
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
            "decoded_dimension_constraint_range_count".to_string(),
            dimension_constraint_range_count,
        ),
        (
            "decoded_complex_constraint_range_count".to_string(),
            complex_constraint_range_count,
        ),
        (
            "decoded_evaluated_constraint_range_count".to_string(),
            evaluated_constraint_range_count,
        ),
        (
            "decoded_unset_constraint_range_count".to_string(),
            unset_constraint_range_count,
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
            "decoded_definition_chain_value_count".to_string(),
            definition_chain_value_count,
        ),
        (
            "decoded_structurally_owned_definition_chain_value_count".to_string(),
            structurally_owned_definition_chain_value_count,
        ),
        (
            "unresolved_definition_chain_value_owner_count".to_string(),
            unowned_definition_chain_value_count,
        ),
        (
            "decoded_unassigned_definition_chain_value_count".to_string(),
            unassigned_definition_chain_value_count,
        ),
        (
            "decoded_definition_chain_evaluation_count".to_string(),
            definition_chain_evaluation_count,
        ),
        (
            "decoded_evaluated_definition_chain_count".to_string(),
            evaluated_definition_chain_count,
        ),
        (
            "decoded_unset_definition_chain_count".to_string(),
            unset_definition_chain_count,
        ),
        (
            "decoded_definition_chain_atom_count".to_string(),
            definition_chain_atom_count,
        ),
        (
            "decoded_definition_chain_control_count".to_string(),
            definition_chain_control_count,
        ),
        (
            "decoded_definition_chain_separator_count".to_string(),
            definition_chain_separator_count,
        ),
        (
            "decoded_definition_chain_schema_selector_count".to_string(),
            definition_chain_schema_selector_count,
        ),
        (
            "decoded_structurally_owned_definition_chain_evaluation_count".to_string(),
            structurally_owned_definition_chain_evaluation_count,
        ),
        (
            "unresolved_definition_chain_evaluation_owner_count".to_string(),
            unowned_definition_chain_evaluation_count,
        ),
        (
            "decoded_unassigned_definition_chain_evaluation_count".to_string(),
            unassigned_definition_chain_evaluation_count,
        ),
        (
            "decoded_unassigned_object_owner_slot_count".to_string(),
            unassigned_owner_slot_count,
        ),
        (
            "decoded_formula_relation_count".to_string(),
            formula_relation_count,
        ),
        (
            "decoded_resolved_formula_output_count".to_string(),
            resolved_formula_output_count,
        ),
        (
            "unresolved_formula_output_count".to_string(),
            unresolved_formula_output_count,
        ),
        (
            "decoded_formula_parameter_dependency_count".to_string(),
            formula_parameter_dependency_count,
        ),
        (
            "decoded_resolved_formula_parameter_dependency_count".to_string(),
            resolved_formula_parameter_dependency_count,
        ),
        (
            "unresolved_formula_parameter_dependency_count".to_string(),
            unresolved_formula_parameter_dependency_count,
        ),
        (
            "ambiguous_formula_parameter_dependency_count".to_string(),
            ambiguous_formula_parameter_dependency_count,
        ),
        (
            "decoded_escaped_word_entity_suffix_count".to_string(),
            escaped_word_entity_suffix_count,
        ),
        (
            "decoded_token_8149_entity_suffix_count".to_string(),
            token_8149_entity_suffix_count,
        ),
        (
            "decoded_fixed_fe_f6_entity_suffix_count".to_string(),
            fixed_fe_f6_entity_suffix_count,
        ),
        (
            "decoded_paged_atom_state_01_entity_suffix_count".to_string(),
            paged_atom_state_01_entity_suffix_count,
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
            "decoded_control_e8_entity_suffix_value_count".to_string(),
            control_e8_entity_suffix_value_count,
        ),
        (
            "decoded_control_e9_entity_suffix_value_count".to_string(),
            control_e9_entity_suffix_value_count,
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
            "decoded_schema_selected_control_entity_suffix_value_count".to_string(),
            schema_selected_control_entity_suffix_value_count,
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
            "transferred_formula_design_record_count".to_string(),
            transferred_formula_design_records.len(),
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
    let untransferred_line_profile_count = native
        .consolidated_line_profiles
        .len()
        .saturating_sub(transferred_line_profile_count);
    if untransferred_line_profile_count > 0 {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{untransferred_line_profile_count} consolidated line-profile record(s) retain \
                 exact line geometry but were not transferred by the active geometry route."
            ),
            provenance: None,
        });
    }
    if !native.zero_entity_support_runs.is_empty()
        || !native.zero_entity_edge_strides.is_empty()
        || !native.zero_entity_oriented_use_pairs.is_empty()
        || !native.zero_entity_vertex_incidences.is_empty()
    {
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
        let support_pcurve_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.pcurve.is_some())
            .count();
        let support_model_curve_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_curve.is_some())
            .count();
        let support_model_construction_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_curve_construction.is_some())
            .count();
        let model_endpoint_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_endpoints.is_some())
            .count();
        let model_midpoint_count = native
            .zero_entity_support_runs
            .iter()
            .flat_map(|run| &run.supports)
            .filter(|support| support.model_midpoint.is_some())
            .count();
        let face_count = native
            .zero_entity_support_runs
            .iter()
            .filter(|run| run.face.is_some())
            .count();
        let loop_terminal_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .map(|face| face.loop_terminals.len())
            .sum::<usize>();
        let loop_record_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .map(|face| face.loops.len())
            .sum::<usize>();
        let oriented_loop_member_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.forward_senses.len())
            .sum::<usize>();
        let oriented_model_endpoint_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.oriented_model_endpoints.len())
            .sum::<usize>();
        let bound_support_member_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.support_record_ordinals.len())
            .sum::<usize>();
        let bound_typed_loop_reference_count = native
            .zero_entity_support_runs
            .iter()
            .filter_map(|run| run.face.as_ref())
            .flat_map(|face| &face.loops)
            .map(|loop_record| loop_record.typed_records.len())
            .sum::<usize>();
        let vertex_owner_binding_count = native
            .zero_entity_vertex_incidences
            .iter()
            .filter(|incidence| incidence.vertex_record.is_some())
            .count();
        let ownership_face_count = native
            .zero_entity_ownership_roots
            .first()
            .map_or(0, |root| root.face_slots.len());
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Warning,
            message: format!(
                "{} zero-entity surface-support run(s) retain {support_count} face-local \
                 occurrence(s), including {support_pcurve_count} complete parameter-space \
                 curve(s), {support_model_curve_count} with exact model-space carriers, \
                 {support_model_construction_count} with exact procedural model-space carriers, \
                 {endpoint_count} with exact UV endpoint pairs, and \
                 {model_endpoint_count} lifted model-space endpoint pairs with \
                 {model_midpoint_count} bounded-curve midpoint witnesses; \
                 {face_count} run(s) bind the complete face roster with {loop_terminal_count} \
                 ordered loop terminal(s), {loop_record_count} loop record(s), and \
                 {oriented_loop_member_count} stored member sense(s), including \
                 {oriented_model_endpoint_count} sense-oriented model-space endpoint pair(s), \
                 {bound_support_member_count} member(s) bound to face-local support records and \
                 {bound_typed_loop_reference_count} typed reference(s) bound to global records; {} \
                 edge-stride allocation tuple(s) remain separate, and \
                 {vertex_owner_binding_count} of {} vertex-incidence record(s) bind their \
                 adjacent vertex owner; {} ownership root(s) bind {ownership_face_count} face \
                 allocation(s) through a shell and body; {} radial occurrence endpoint-pair \
                 candidate(s) and {} \
                 complete endpoint-locus candidate(s) are established from matching bounded \
                 model-space endpoint and midpoint witnesses; curve coincidence, loop-to-use, \
                 use-to-incidence, physical \
                 endpoint identity remain unresolved; {} oriented-use \
                 pair(s) remain separate.",
                native.zero_entity_support_runs.len(),
                native.zero_entity_edge_strides.len(),
                native.zero_entity_vertex_incidences.len(),
                native.zero_entity_ownership_roots.len(),
                native.zero_entity_endpoint_pair_candidates.len(),
                native.zero_entity_endpoint_locus_candidates.len(),
                native.zero_entity_oriented_use_pairs.len(),
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
                "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), including {unassigned_owner_slot_count} with an explicit literal unassigned owner slot, {entity_value_field_count} entity-value field(s), {entity_value_schema_selection_count} entity-value schema selection(s), {numeric_entity_value_tuple_count} complete numeric entity-value tuple(s), {numeric_entity_value_packet_count} embedded numeric entity-value packet(s), {compact_entity_value_packet_count} compact entity-value packet(s), {layout_entity_value_packet_count} layout entity-value packet(s), {escaped_word_entity_suffix_count} escaped-word entity suffix(es), {token_8149_entity_suffix_count} standalone 8149 suffix token(s), {fixed_fe_f6_entity_suffix_count} fixed FE-F6 suffix frame(s), {paged_atom_state_01_entity_suffix_count} paged-atom state-01 suffix(es), {scalar_entity_suffix_value_count} scalar entity-suffix value(s), {unset_entity_suffix_value_count} unset entity-suffix value(s), {atom_entity_suffix_value_count} atom entity-suffix value(s), {separator_entity_suffix_value_count} separator entity-suffix value(s), {schema_selected_atom_entity_suffix_value_count} schema-selected atom value(s), {schema_selected_evaluation_entity_suffix_value_count} schema-selected evaluation(s), {schema_selected_control_entity_suffix_value_count} schema-selected control value(s), {schema_selected_separator_entity_suffix_value_count} schema-selected separator(s), {schema_selected_schema_entity_suffix_value_count} schema-selected schema value(s), {schema_selected_entity_suffix_value_count} suffix value(s) with resolved schema selectors, {wide_prefix_entity_suffix_value_count} suffix value(s) with multi-byte prefix atoms, {control_entity_suffix_value_count} direct control entity-suffix value(s), comprising {control_e8_entity_suffix_value_count} E8 and {control_e9_entity_suffix_value_count} E9 state(s), {relation_expression_count} complete relation expression(s), {parameter_value_count} complete named parameter value(s), {constraint_range_count} complete constraint-range value(s), comprising {dimension_constraint_range_count} dimension and {complex_constraint_range_count} complex-constraint range(s), with {evaluated_constraint_range_count} finite evaluation(s) and {unset_constraint_range_count} unset evaluation(s), {definition_value_count} definition-bound suffix value(s), including {owned_definition_value_count} assigned to design objects and {unowned_definition_value_count} without a resolved owner, {definition_chain_evaluation_count} two-definition chain evaluation(s), comprising {evaluated_definition_chain_count} finite and {unset_definition_chain_count} unset value(s), with {structurally_owned_definition_chain_evaluation_count} structurally owned and {unowned_definition_chain_evaluation_count} without a resolved structural owner; {unassigned_definition_chain_value_count} chain value(s), including {unassigned_definition_chain_evaluation_count} evaluation(s), occupy explicit literal unassigned owner slots; {formula_relation_count} complete formula relation(s), comprising {resolved_formula_output_count} resolved and {unresolved_formula_output_count} unresolved output identities, {formula_parameter_dependency_count} formula parameter symbol occurrence(s), comprising {resolved_formula_parameter_dependency_count} uniquely resolved and {unresolved_formula_parameter_dependency_count} unresolved, including {ambiguous_formula_parameter_dependency_count} with multiple candidates, {repeated_reference_suffix_count} repeated-reference suffix(es), {repeated_reference_schema_selection_count} repeated-reference schema selection(s), {definition_schema_selection_count} definition-schema selection(s), {design_object_owner_link_count} structural owner link(s), and {design_object_relation_count} exact outbound design-field relation occurrence(s), including {design_same_object_relation_count} within one design object, {design_reflexive_field_relation_count} reflexive field occurrence(s), and {design_unowned_field_relation_count} to fields without owner groups; {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; {} typed formula parameter(s), {} exact formula, expression, or parameter field record(s), and {} exact principal-plane field record(s) transferred, while {unresolved_object_record_count} field record(s) across {unresolved_design_object_count} design object(s), neutral features, other parameters, sketch identity and geometry, constraints, configurations, and re-derivable history remain unresolved.",
                native.design_objects.len(),
                formula_transfer.formula_parameter_count,
                transferred_formula_design_records.len(),
                transferred_principal_plane_records.len(),
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
