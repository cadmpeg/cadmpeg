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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Angle, DesignParameter, Length, ParameterId, ParameterValue};
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{Annotations, Exactness, SourceFidelity};

use crate::assemble::{build_container_report, build_metadata_ir};
use crate::container::{self, ContainerScan};
use crate::entity_table;
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
    mut annotations: Annotations,
    unknowns: &[UnknownRecord],
) -> Result<DecodeResult, CodecError> {
    let native = CatiaNative::decode(&scan.data);
    let formula_transfer = transfer_formula_parameters(&mut ir, &native, &mut annotations);
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

fn transfer_formula_parameters(
    ir: &mut CadIr,
    native: &CatiaNative,
    annotations: &mut Annotations,
) -> FormulaTransfer {
    let entities = native
        .entity_records
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    let object_records = native
        .object_graphs
        .iter()
        .flat_map(|graph| &graph.records)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let mut candidates = BTreeMap::<ParameterId, FormulaParameterCandidate>::new();
    let mut conflicting = BTreeSet::<ParameterId>::new();
    let mut programs = Vec::<FormulaProgramCandidate>::new();

    for formula_entity in &native.entity_records {
        let Some(formula) = &formula_entity.formula_relation else {
            continue;
        };
        let Some(expression_entity) = entities.get(formula.expression.as_str()) else {
            continue;
        };
        let Some(expression) = &expression_entity.relation_expression else {
            continue;
        };
        let Some(signature) = &expression.signature else {
            continue;
        };
        let mut transferred = Vec::with_capacity(formula.parameter_dependencies.len() + 1);
        let mut dependencies = Vec::with_capacity(formula.parameter_dependencies.len());
        let mut used_inputs = BTreeSet::new();
        let mut all_inputs_complete = !formula.parameter_dependencies.is_empty();
        for dependency in &formula.parameter_dependencies {
            let Some(input) = signature.inputs.iter().find(|input| {
                dependency
                    .symbol
                    .strip_prefix(&input.parameter)
                    .is_some_and(|suffix| suffix.starts_with(char::is_whitespace))
            }) else {
                all_inputs_complete = false;
                continue;
            };
            let Some(entity) = entities.get(dependency.parameter.as_str()) else {
                all_inputs_complete = false;
                continue;
            };
            let Some(parameter) = &entity.parameter_value else {
                all_inputs_complete = false;
                continue;
            };
            let Some(TypedParameterEvaluation::Value(value)) =
                typed_parameter_evaluation(&input.input_type, &parameter.evaluation)
            else {
                all_inputs_complete = false;
                continue;
            };
            used_inputs.insert(input.parameter.as_str());
            let id = neutral_parameter_id(&entity.id);
            if dependencies.contains(&id) {
                continue;
            }
            dependencies.push(id.clone());
            transferred.push(FormulaParameterCandidate {
                parameter: DesignParameter {
                    id,
                    owner: None,
                    ordinal: 0,
                    name: parameter.name.value.clone(),
                    expression: match &value {
                        ParameterValue::Length(Length(value)) => format!("{value} mm"),
                        ParameterValue::Angle(Angle(value)) => format!("{value} rad"),
                        ParameterValue::Real(value) => value.to_string(),
                        ParameterValue::Integer(value) => value.to_string(),
                        ParameterValue::Boolean(_) | ParameterValue::String(_) => unreachable!(),
                    },
                    display: None,
                    value: Some(value),
                    dependencies: Vec::new(),
                    properties: BTreeMap::new(),
                    pmi: None,
                    native_ref: Some(entity.id.clone()),
                },
                formula_output: false,
                source_order: entity.byte_offset,
            });
        }
        let formula_complete = all_inputs_complete
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        if let Some(output) = formula
            .parameter
            .as_deref()
            .filter(|_| formula_complete)
            .and_then(|id| entities.get(id))
        {
            if let Some(output_value) = &output.parameter_value {
                let output_id = neutral_parameter_id(&output.id);
                if !dependencies.contains(&output_id) {
                    if let Some(value) =
                        typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)
                    {
                        programs.push(FormulaProgramCandidate {
                            formula_entity: formula_entity.id.clone(),
                            expression_entity: expression_entity.id.clone(),
                            output: output_id.clone(),
                            inputs: dependencies.clone(),
                        });
                        transferred.push(FormulaParameterCandidate {
                            parameter: DesignParameter {
                                id: output_id,
                                owner: None,
                                ordinal: 0,
                                name: output_value.name.value.clone(),
                                expression: expression.expression.value.clone(),
                                display: None,
                                value: match value {
                                    TypedParameterEvaluation::Unset => None,
                                    TypedParameterEvaluation::Value(value) => Some(value),
                                },
                                dependencies,
                                properties: BTreeMap::new(),
                                pmi: None,
                                native_ref: Some(output.id.clone()),
                            },
                            formula_output: true,
                            source_order: output.byte_offset,
                        });
                    }
                }
            }
        }

        for candidate in transferred {
            match candidates.get(&candidate.parameter.id) {
                Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
                    conflicting.insert(candidate.parameter.id);
                }
                Some(existing) if !existing.formula_output && candidate.formula_output => {
                    candidates.insert(candidate.parameter.id.clone(), candidate);
                }
                Some(_) => {}
                None => {
                    candidates.insert(candidate.parameter.id.clone(), candidate);
                }
            }
        }
    }

    for id in &conflicting {
        candidates.remove(id);
    }
    loop {
        let invalid = candidates
            .iter()
            .filter(|(_, parameter)| {
                parameter
                    .parameter
                    .dependencies
                    .iter()
                    .any(|dependency| !candidates.contains_key(dependency))
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if invalid.is_empty() {
            break;
        }
        for id in invalid {
            candidates.remove(&id);
        }
    }
    let mut derivable = BTreeSet::new();
    loop {
        let previous_len = derivable.len();
        for (id, candidate) in &candidates {
            if candidate
                .parameter
                .dependencies
                .iter()
                .all(|dependency| derivable.contains(dependency))
            {
                derivable.insert(id.clone());
            }
        }
        if derivable.len() == previous_len {
            break;
        }
    }
    candidates.retain(|id, _| derivable.contains(id));
    let mut consumed_entity_records = candidates
        .values()
        .filter_map(|candidate| candidate.parameter.native_ref.clone())
        .collect::<HashSet<_>>();
    let mut programs_by_output = BTreeMap::<ParameterId, Vec<FormulaProgramCandidate>>::new();
    for program in programs {
        programs_by_output
            .entry(program.output.clone())
            .or_default()
            .push(program);
    }
    for programs in programs_by_output.into_values() {
        let [program] = programs.as_slice() else {
            continue;
        };
        if candidates
            .get(&program.output)
            .is_some_and(|candidate| candidate.formula_output)
            && program
                .inputs
                .iter()
                .all(|input| candidates.contains_key(input))
        {
            consumed_entity_records.insert(program.formula_entity.clone());
            consumed_entity_records.insert(program.expression_entity.clone());
        }
    }
    let consumed_object_records = consumed_entity_records
        .iter()
        .filter_map(|entity| {
            let entity = entities.get(entity.as_str())?;
            let object = object_records.get(entity.object_record.as_str())?;
            (entity.formula_relation.is_some()
                || object.subtype == crate::object_graph::PayloadSubtype::Empty
                    && object.references.is_empty())
            .then(|| object.id.clone())
        })
        .collect();
    let mut parameters = candidates.into_values().collect::<Vec<_>>();
    parameters.sort_by_key(|candidate| candidate.source_order);
    let Some(parameters) = parameters
        .into_iter()
        .enumerate()
        .map(|(ordinal, mut candidate)| {
            candidate.parameter.ordinal = u32::try_from(ordinal).ok()?;
            Some(candidate.parameter)
        })
        .collect::<Option<Vec<_>>>()
    else {
        return FormulaTransfer::default();
    };
    for parameter in &parameters {
        if parameter.dependencies.is_empty() {
            annotations
                .exactness
                .entry(parameter.id.0.clone())
                .or_default()
                .fields
                .insert("expression".to_string(), Exactness::Derived);
        }
    }
    let transferred = parameters.len();
    ir.model.parameters.extend(parameters);
    FormulaTransfer {
        parameter_count: transferred,
        consumed_object_records,
    }
}

#[derive(Default)]
struct FormulaTransfer {
    parameter_count: usize,
    consumed_object_records: HashSet<String>,
}

struct FormulaParameterCandidate {
    parameter: DesignParameter,
    formula_output: bool,
    source_order: u64,
}

struct FormulaProgramCandidate {
    formula_entity: String,
    expression_entity: String,
    output: ParameterId,
    inputs: Vec<ParameterId>,
}

fn formula_parameter_candidates_agree(
    existing: &FormulaParameterCandidate,
    candidate: &FormulaParameterCandidate,
) -> bool {
    if existing.source_order != candidate.source_order {
        return false;
    }
    match (existing.formula_output, candidate.formula_output) {
        (true, true) | (false, false) => existing.parameter == candidate.parameter,
        (true, false) => formula_parameter_matches_input(&existing.parameter, &candidate.parameter),
        (false, true) => formula_parameter_matches_input(&candidate.parameter, &existing.parameter),
    }
}

fn formula_parameter_matches_input(formula: &DesignParameter, input: &DesignParameter) -> bool {
    formula.id == input.id
        && formula.owner == input.owner
        && formula.ordinal == input.ordinal
        && formula.name == input.name
        && formula.display == input.display
        && formula.value == input.value
        && formula.properties == input.properties
        && formula.pmi == input.pmi
        && formula.native_ref == input.native_ref
}

enum TypedParameterEvaluation {
    Unset,
    Value(ParameterValue),
}

fn typed_parameter_evaluation(
    source_type: &str,
    evaluation: &crate::native::CatiaEntityEvaluation,
) -> Option<TypedParameterEvaluation> {
    if !matches!(
        source_type,
        "LENGTH" | "ANGLE" | "Real" | "R" | "Integer" | "I"
    ) {
        return None;
    }
    let bits = match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            return Some(TypedParameterEvaluation::Unset);
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => bits,
    };
    let value = f64::from_bits(*bits);
    let value = match source_type {
        "LENGTH" => ParameterValue::Length(Length(value)),
        "ANGLE" => ParameterValue::Angle(Angle(value)),
        "Real" | "R" => ParameterValue::Real(value),
        "Integer" | "I"
            if value.fract() == 0.0 && value >= i64::MIN as f64 && value < -(i64::MIN as f64) =>
        {
            ParameterValue::Integer(value as i64)
        }
        _ => return None,
    };
    Some(TypedParameterEvaluation::Value(value))
}

fn neutral_parameter_id(native_id: &str) -> ParameterId {
    ParameterId(format!("{native_id}:parameter"))
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
