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
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchGeometry, SketchId, SketchPlacement,
};
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
    transfer_sketch_points(&mut ir, &native);
    let object_record_count: usize = native
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
            "decoded_consolidated_class61_record_count".to_string(),
            native.consolidated_class61_records.len(),
        ),
        (
            "decoded_consolidated_group_count".to_string(),
            native.consolidated_groups.len(),
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
            "transferred_formula_design_record_count".to_string(),
            transferred_formula_design_records.len(),
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
    if unresolved_object_record_count != 0 {
        report.losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::FeatureHistoryRetained,
            category: LossCategory::DesignIntent,
            severity: Severity::Blocking,
            message: format!(
                "CATIA native data retains {} design object(s), {design_field_count} grouped field(s), {object_record_count} object-graph field record(s), {entity_value_field_count} entity-value field(s), {entity_value_schema_selection_count} entity-value schema selection(s), {numeric_entity_value_packet_count} numeric entity-value packet(s), {compact_entity_value_packet_count} compact entity-value packet(s), {layout_entity_value_packet_count} layout entity-value packet(s), {relation_expression_count} complete relation expression(s), {parameter_value_count} complete named parameter value(s), {formula_relation_count} complete formula relation(s), {formula_parameter_dependency_count} formula parameter dependency link(s), {repeated_reference_suffix_count} repeated-reference suffix(es), {repeated_reference_schema_selection_count} repeated-reference schema selection(s), {definition_schema_selection_count} definition-schema selection(s), {design_object_owner_link_count} structural owner link(s), and {design_object_reference_count} inter-object reference(s); {classified_design_object_count} design object(s) have class evidence and {unresolved_design_owner_count} owner identity or identities remain unresolved; {} typed formula parameter(s) and {} exact formula, expression, or parameter field record(s) transferred, while {unresolved_object_record_count} field record(s) across {unresolved_design_object_count} design object(s), neutral features, other parameters, sketch geometry, constraints, configurations, and re-derivable history remain unresolved.",
                native.design_objects.len(),
                formula_transfer.parameter_count,
                transferred_formula_design_records.len(),
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

fn transfer_sketch_points(ir: &mut CadIr, native: &CatiaNative) {
    let entities = native
        .entity_records
        .iter()
        .map(|entity| (entity.object_record.as_str(), entity))
        .collect::<HashMap<_, _>>();
    let design_objects = native
        .design_objects
        .iter()
        .map(|object| (object.id.as_str(), object))
        .collect::<HashMap<_, _>>();
    let existing_sketch_ids = ir
        .model
        .sketches
        .iter()
        .map(|sketch| sketch.id.clone())
        .collect::<HashSet<_>>();
    let existing_entity_ids = ir
        .model
        .sketch_entities
        .iter()
        .map(|entity| entity.id.clone())
        .collect::<HashSet<_>>();
    let mut points_by_sketch = HashMap::<String, (u64, Vec<SketchPointCandidate>)>::new();

    for object in &native.design_objects {
        let Some(owner_record) = object.owner_record.as_deref() else {
            continue;
        };
        let Some(entity) = entities.get(owner_record) else {
            continue;
        };
        let Some(position) = exact_point2(entity.numeric_tuple.as_ref()) else {
            continue;
        };
        let sketch_targets = object
            .relations
            .iter()
            .filter(|relation| {
                relation
                    .source_class
                    .as_ref()
                    .is_some_and(|class| class.name == "2DPoint")
            })
            .filter_map(|relation| {
                let target = design_objects.get(relation.target_design_object.as_str())?;
                target
                    .field_classes
                    .iter()
                    .any(|class| class.name == "PRTSketch")
                    .then_some(target.id.as_str())
            })
            .collect::<BTreeSet<_>>();
        let mut sketch_targets = sketch_targets.into_iter();
        let Some(sketch_target) = sketch_targets.next() else {
            continue;
        };
        if sketch_targets.next().is_some() {
            continue;
        }
        let sketch_object = design_objects
            .get(sketch_target)
            .expect("sketch target was selected from the design-object map");
        points_by_sketch
            .entry(sketch_object.id.clone())
            .or_insert_with(|| (sketch_object.first_field_byte_offset, Vec::new()))
            .1
            .push(SketchPointCandidate {
                design_object: object.id.clone(),
                native_entity: entity.id.clone(),
                source_order: object.first_field_byte_offset,
                position,
            });
    }

    let mut sketch_groups = points_by_sketch.into_iter().collect::<Vec<_>>();
    sketch_groups.sort_by_key(|(_, (source_order, _))| *source_order);
    for (native_sketch, (_, mut points)) in sketch_groups {
        let sketch_id = SketchId(format!("{native_sketch}:sketch"));
        if existing_sketch_ids.contains(&sketch_id) {
            continue;
        }
        points.sort_by_key(|point| point.source_order);
        let entities = points
            .into_iter()
            .filter_map(|point| {
                let id = SketchEntityId(format!("{}:sketch-point", point.design_object));
                (!existing_entity_ids.contains(&id)).then_some(SketchEntity {
                    id,
                    sketch: sketch_id.clone(),
                    construction: false,
                    native_ref: Some(point.native_entity),
                    geometry_ref: None,
                    endpoint_refs: Vec::new(),
                    geometry: SketchGeometry::Point {
                        position: point.position,
                    },
                })
            })
            .collect::<Vec<_>>();
        if entities.is_empty() {
            continue;
        }
        ir.model.sketches.push(Sketch {
            id: sketch_id,
            name: None,
            configuration: None,
            placement: SketchPlacement::Unresolved,
            profiles: Vec::new(),
            native_ref: Some(native_sketch),
        });
        ir.model.sketch_entities.extend(entities);
    }
}

fn exact_point2(tuple: Option<&entity_table::NumericTuple>) -> Option<Point2> {
    let tuple = tuple?;
    let [entity_table::NumericTupleItem::Binary64 { bits: x, .. }, entity_table::NumericTupleItem::Binary64 { bits: y, .. }] =
        tuple.items.as_slice()
    else {
        return None;
    };
    let (x, y) = (f64::from_bits(*x), f64::from_bits(*y));
    (x.is_finite() && y.is_finite()).then(|| Point2::new(x, y))
}

struct SketchPointCandidate {
    design_object: String,
    native_entity: String,
    source_order: u64,
    position: Point2,
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
            entities
                .get(entity.as_str())
                .map(|entity| entity.object_record.clone())
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
    evaluation: &crate::native::CatiaParameterEvaluation,
) -> Option<TypedParameterEvaluation> {
    if !matches!(
        source_type,
        "LENGTH" | "ANGLE" | "Real" | "R" | "Integer" | "I"
    ) {
        return None;
    }
    let crate::native::CatiaParameterEvaluation::Scalar { bits } = evaluation else {
        return Some(TypedParameterEvaluation::Unset);
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
