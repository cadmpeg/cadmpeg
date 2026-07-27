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

use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_ir::codec::{CodecError, DecodeResult};
use cadmpeg_ir::decode::{DecodeContext, View};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{DesignParameter, Length, ParameterId, ParameterValue};
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
    let transferred_formula_parameter_count =
        transfer_formula_parameters(&mut ir, &native, &mut annotations);
    let object_record_count = native
        .object_graphs
        .iter()
        .map(|graph| graph.records.len())
        .sum();
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
    let design_object_reference_count = native
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
            "decoded_design_object_reference_count".to_string(),
            design_object_reference_count,
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
            "decoded_formula_relation_count".to_string(),
            formula_relation_count,
        ),
        (
            "decoded_formula_parameter_dependency_count".to_string(),
            formula_parameter_dependency_count,
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
                "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), {entity_value_field_count} entity-value field(s), {entity_value_schema_selection_count} entity-value schema selection(s), {numeric_entity_value_packet_count} numeric entity-value packet(s), {compact_entity_value_packet_count} compact entity-value packet(s), {layout_entity_value_packet_count} layout entity-value packet(s), {relation_expression_count} complete relation expression(s), {parameter_value_count} complete named parameter value(s), {formula_relation_count} complete formula relation(s), {formula_parameter_dependency_count} formula parameter dependency link(s), {repeated_reference_suffix_count} repeated-reference suffix(es), {repeated_reference_schema_selection_count} repeated-reference schema selection(s), {definition_schema_selection_count} definition-schema selection(s), {design_object_owner_link_count} structural owner link(s), and {design_object_reference_count} inter-object reference(s); {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; {} closed formula parameter(s) transferred, while neutral features, other parameters, sketch geometry, constraints, configurations, and re-derivable history remain unresolved.",
                native.design_objects.len(),
                transferred_formula_parameter_count,
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
) -> usize {
    let entities = native
        .entity_records
        .iter()
        .map(|entity| (entity.id.as_str(), entity))
        .collect::<HashMap<_, _>>();
    let mut candidates = BTreeMap::<ParameterId, DesignParameter>::new();
    let mut conflicting = BTreeSet::<ParameterId>::new();

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
        if signature.input_type != "LENGTH" || signature.result_type != "LENGTH" {
            continue;
        }
        let Some(output) = formula.parameter.as_deref().and_then(|id| entities.get(id)) else {
            continue;
        };
        let Some(output_value) = &output.parameter_value else {
            continue;
        };

        let mut transferred = Vec::with_capacity(formula.parameter_dependencies.len() + 1);
        let mut dependencies = Vec::with_capacity(formula.parameter_dependencies.len());
        let mut complete = !formula.parameter_dependencies.is_empty();
        for dependency in &formula.parameter_dependencies {
            let Some(entity) = entities.get(dependency.parameter.as_str()) else {
                complete = false;
                break;
            };
            let Some(parameter) = &entity.parameter_value else {
                complete = false;
                break;
            };
            let crate::native::CatiaParameterEvaluation::Scalar { bits } = parameter.evaluation
            else {
                complete = false;
                break;
            };
            let value = f64::from_bits(bits);
            let id = neutral_parameter_id(&entity.id);
            if entity.id == output.id || dependencies.contains(&id) {
                complete = false;
                break;
            }
            let Ok(ordinal) = u32::try_from(entity.ordinal) else {
                complete = false;
                break;
            };
            dependencies.push(id.clone());
            transferred.push(DesignParameter {
                id,
                owner: None,
                ordinal,
                name: parameter.name.value.clone(),
                expression: format!("{value} mm"),
                display: None,
                value: Some(ParameterValue::Length(Length(value))),
                dependencies: Vec::new(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: Some(entity.id.clone()),
            });
        }
        if !complete {
            continue;
        }
        let Ok(ordinal) = u32::try_from(output.ordinal) else {
            continue;
        };
        let value = match output_value.evaluation {
            crate::native::CatiaParameterEvaluation::Unset => None,
            crate::native::CatiaParameterEvaluation::Scalar { bits } => {
                Some(ParameterValue::Length(Length(f64::from_bits(bits))))
            }
        };
        transferred.push(DesignParameter {
            id: neutral_parameter_id(&output.id),
            owner: None,
            ordinal,
            name: output_value.name.value.clone(),
            expression: expression.expression.value.clone(),
            display: None,
            value,
            dependencies,
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: Some(output.id.clone()),
        });

        for parameter in transferred {
            match candidates.get(&parameter.id) {
                Some(existing) if existing != &parameter => {
                    conflicting.insert(parameter.id);
                }
                Some(_) => {}
                None => {
                    candidates.insert(parameter.id.clone(), parameter);
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
    let mut parameters = candidates.into_values().collect::<Vec<_>>();
    parameters.sort_by_key(|parameter| parameter.ordinal);
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
    transferred
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
