// SPDX-License-Identifier: Apache-2.0
//! Transfer of complete, typed CATIA formula programs to neutral parameters.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Angle, DesignParameter, Length, ParameterId, ParameterValue};
use cadmpeg_ir::{AnnotationBuilder, Annotations};

use crate::native::CatiaNative;

pub(crate) fn transfer_parameters(
    ir: &mut CadIr,
    native: &CatiaNative,
    annotations: &mut Annotations,
    graph_scope: Option<&HashSet<String>>,
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
    let mut conflicting_inputs = BTreeSet::<ParameterId>::new();
    collect_definition_chain_parameters(
        native,
        graph_scope,
        &mut candidates,
        &mut conflicting_inputs,
    );
    let mut programs = Vec::<FormulaProgramCandidate>::new();
    let mut formula_definition_counts = HashMap::<ParameterId, usize>::new();
    for entity in native.entity_records.iter().filter(|entity| {
        graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
        let outputs = entity
            .formula_relation
            .as_ref()
            .and_then(|relation| relation.output_entity.reference.entity())
            .into_iter()
            .chain(
                entity
                    .relation_program_instance
                    .as_ref()
                    .and_then(|instance| instance.output_entity())
                    .and_then(|output| output.entity()),
            );
        for output in outputs {
            *formula_definition_counts
                .entry(neutral_parameter_id(output))
                .or_default() += 1;
        }
    }
    let legacy_scope = match graph_scope {
        None => LegacyModelingScope::Unbounded,
        Some(scope) => native
            .object_graphs
            .iter()
            .find(|graph| scope.contains(graph.id.as_str()))
            .and_then(|graph| graph.outer_container.as_ref())
            .map_or(
                LegacyModelingScope::Unresolved,
                LegacyModelingScope::Container,
            ),
    };
    let legacy_transfer = collect_legacy_parameters(native, &mut candidates, legacy_scope);
    let mut relation_program_parameters =
        BTreeMap::<ParameterId, Option<(DesignParameter, FormulaParameterType)>>::new();
    for program_entity in native.entity_records.iter().filter(|entity| {
        graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
        let Some(inputs) = program_entity
            .relation_program_instance
            .as_ref()
            .and_then(|instance| instance.inputs.as_ref())
        else {
            continue;
        };
        for input in inputs {
            let Some(entity) = input
                .entity
                .entity()
                .and_then(|entity| entities.get(entity))
            else {
                continue;
            };
            let Some(candidate) =
                typed_entity_parameter_candidate_for_source(entity, input.value_type.as_str())
            else {
                continue;
            };
            relation_program_parameters
                .entry(candidate.parameter.id.clone())
                .and_modify(|existing| {
                    if existing.as_ref().is_some_and(|input| {
                        input.1 != candidate.parameter_type || input.0 != candidate.parameter
                    }) {
                        *existing = None;
                    }
                })
                .or_insert_with(|| Some((candidate.parameter.clone(), candidate.parameter_type)));
            match candidates.get(&candidate.parameter.id) {
                Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
                    conflicting_inputs.insert(candidate.parameter.id);
                }
                Some(_) => {}
                None => {
                    candidates.insert(candidate.parameter.id.clone(), candidate);
                }
            }
        }
    }

    for formula_entity in native.entity_records.iter().filter(|entity| {
        graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
        let Some(formula) = &formula_entity.formula_relation else {
            continue;
        };
        let Some(expression_entity) = formula
            .expression_entity
            .reference
            .entity()
            .and_then(|expression| entities.get(expression))
        else {
            continue;
        };
        let Some(expression) = &expression_entity.relation_expression else {
            continue;
        };
        let Some(signature) = expression.signature() else {
            continue;
        };
        let mut transferred = Vec::with_capacity(formula.parameter_dependencies.len() + 1);
        let mut dependencies = Vec::with_capacity(formula.parameter_dependencies.len());
        let mut used_inputs = BTreeSet::new();
        let mut expression_bindings = BTreeMap::new();
        // E7 inputs have a declared type but no value. Keep a separate typed
        // binding set so their defining expression can still be retained as
        // an unset result after syntax and type validation.
        let mut type_bindings = BTreeMap::new();
        let mut all_inputs_complete = true;
        let mut all_inputs_typed = true;
        for dependency in &formula.parameter_dependencies {
            let Some(input) = signature
                .inputs
                .iter()
                .find(|input| crate::native::dependency_matches_input(dependency, input))
            else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            let [parameter] = dependency.candidates.as_slice() else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            let Some(entity) = parameter
                .entity()
                .and_then(|parameter| entities.get(parameter))
            else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            let Some(candidate) =
                typed_entity_parameter_candidate_for_source(entity, &input.input_type)
            else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            used_inputs.insert(input.parameter.as_str());
            let id = candidate.parameter.id.clone();
            if dependencies.contains(&id) {
                continue;
            }
            dependencies.push(id.clone());
            let Some(type_value) = static_formula_value(candidate.parameter_type) else {
                all_inputs_complete = false;
                all_inputs_typed = false;
                continue;
            };
            type_bindings.insert(input.parameter.as_str(), type_value);
            match candidate.parameter.value.as_ref() {
                None => {
                    all_inputs_complete = false;
                }
                Some(value) => {
                    expression_bindings.insert(
                        input.parameter.as_str(),
                        EvaluatedFormulaValue::from_parameter_value(value),
                    );
                }
            }
            transferred.push(candidate);
        }
        let formula_complete = all_inputs_complete
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        let formula_type_complete = all_inputs_typed
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        let type_checked_expression = formula_type_complete
            .then(|| {
                evaluate_formula_expression_with_mode(
                    &expression.expression.value,
                    &type_bindings,
                    false,
                )
            })
            .flatten()
            .filter(|value| value.satisfies_source_type(&signature.result_type));
        let evaluated_expression = formula_complete
            .then(|| {
                evaluate_formula_expression(&expression.expression.value, &expression_bindings)
            })
            .flatten()
            .filter(|value| value.satisfies_source_type(&signature.result_type));
        let transferable_expression = if formula_complete {
            evaluated_expression.clone()
        } else {
            type_checked_expression.clone()
        };
        let input_parameters = transferred
            .iter()
            .map(|candidate| (candidate.parameter.clone(), candidate.parameter_type))
            .collect::<Vec<_>>();
        if let Some(output) = formula
            .output_entity
            .reference
            .entity()
            .filter(|_| transferable_expression.is_some())
            .and_then(|id| entities.get(id))
        {
            if let Some(output_value) = &output.parameter_value {
                let output_id = neutral_parameter_id(&output.id);
                if !dependencies.contains(&output_id) {
                    if let Some(value) =
                        typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)
                    {
                        let accepted = match &value {
                            TypedParameterEvaluation::Unset => transferable_expression.is_some(),
                            TypedParameterEvaluation::Value(value) => {
                                evaluated_expression.as_ref().is_some_and(|evaluated| {
                                    evaluated.agrees_with(&TypedParameterEvaluation::Value(
                                        value.clone(),
                                    ))
                                })
                            }
                        };
                        if accepted {
                            let parameter_type = canonical_parameter_type(&signature.result_type)
                                .expect("typed evaluation requires a supported type");
                            programs.push(FormulaProgramCandidate {
                                relation_entity: formula_entity.id.clone(),
                                expression_entity: expression_entity.id.clone(),
                                output: output_id.clone(),
                                inputs: dependencies.clone(),
                                input_parameters,
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
                                    properties: parameter_properties(
                                        parameter_type.as_str(),
                                        Some(output_value.binding.value.as_str()),
                                    ),
                                    pmi: None,
                                    native_ref: Some(output.id.clone()),
                                },
                                parameter_type,
                                role: FormulaParameterRole::FormulaOutput { fallback: None },
                                source_order: output.byte_offset,
                            });
                        }
                    }
                }
            }
        }

        for candidate in transferred {
            merge_formula_parameter_candidate(&mut candidates, &mut conflicting_inputs, candidate);
        }
    }

    for relation_entity in native.entity_records.iter().filter(|entity| {
        graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
        let Some(instance) = relation_entity.relation_program_instance.as_ref() else {
            continue;
        };
        let Some(output_entity) = instance
            .output_entity()
            .and_then(|output| output.entity())
            .and_then(|output| entities.get(output))
        else {
            continue;
        };
        let Some(expression_entity) = instance
            .relation_expression
            .as_deref()
            .and_then(|expression| entities.get(expression))
        else {
            continue;
        };
        let Some(expression) = &expression_entity.relation_expression else {
            continue;
        };
        let Some(signature) = expression.signature() else {
            continue;
        };
        let Some(inputs) = instance.inputs.as_ref() else {
            continue;
        };
        let Some((program, candidate)) = relation_program_output_candidate(
            relation_entity,
            expression_entity,
            output_entity,
            expression,
            &signature,
            inputs,
            &entities,
        ) else {
            continue;
        };
        programs.push(program);
        merge_formula_parameter_candidate(&mut candidates, &mut conflicting_inputs, candidate);
    }

    for id in &conflicting_inputs {
        match candidates.get_mut(id) {
            Some(candidate) if candidate.role.is_formula_output() => {
                if let FormulaParameterRole::FormulaOutput { fallback } = &mut candidate.role {
                    *fallback = None;
                }
            }
            Some(_) => {
                candidates.remove(id);
            }
            None => {}
        }
    }
    candidates.retain(|id, candidate| {
        match (
            candidate.role.is_formula_output(),
            formula_definition_counts.get(id),
        ) {
            (true, Some(count)) if *count != 1 => demote_formula_output(candidate),
            (true, Some(_)) => true,
            (false, _) | (true, None) => true,
        }
    });
    let invalid_outputs = programs
        .iter()
        .filter(|program| {
            program.input_parameters.iter().any(|input| {
                !candidates.get(&input.0.id).is_some_and(|candidate| {
                    formula_parameter_candidate_accepts_input(candidate, input)
                })
            })
        })
        .map(|program| program.output.clone())
        .collect::<HashSet<_>>();
    for output in invalid_outputs {
        let Some(candidate) = candidates.get_mut(&output) else {
            continue;
        };
        if !candidate.role.is_formula_output() {
            continue;
        }
        if !demote_formula_output(candidate) {
            candidates.remove(&output);
        }
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
    let relation_program_parameter_count = relation_program_parameters
        .iter()
        .filter(|(id, input)| {
            input.as_ref().is_some_and(|input| {
                candidates.get(*id).is_some_and(|candidate| {
                    formula_parameter_candidate_accepts_input(candidate, input)
                })
            })
        })
        .count();
    let mut consumed_entity_records = candidates
        .values()
        .filter_map(|candidate| candidate.parameter.native_ref.clone())
        .collect::<HashSet<_>>();
    for program in programs {
        if candidates
            .get(&program.output)
            .is_some_and(|candidate| candidate.role.is_formula_output())
            && program
                .inputs
                .iter()
                .all(|input| candidates.contains_key(input))
        {
            consumed_entity_records.insert(program.relation_entity);
            consumed_entity_records.insert(program.expression_entity);
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
            Some(candidate)
        })
        .collect::<Option<Vec<_>>>()
    else {
        return FormulaTransfer::default();
    };
    let definition_chain_parameter_count = parameters
        .iter()
        .filter(|candidate| {
            candidate
                .parameter
                .native_ref
                .as_ref()
                .and_then(|native_ref| entities.get(native_ref.as_str()))
                .is_some_and(|entity| entity.definition_chain_value.is_some())
        })
        .count();
    let mut annotation_builder = AnnotationBuilder::resume(std::mem::take(annotations));
    for candidate in &parameters {
        annotation_builder.derived(&candidate.parameter.id.as_str(), "properties");
        if !candidate.role.is_formula_output() && candidate.parameter.dependencies.is_empty() {
            annotation_builder.derived(&candidate.parameter.id.as_str(), "expression");
        }
    }
    *annotations = annotation_builder.build();
    let transferred = parameters.len();
    ir.model
        .parameters
        .extend(parameters.into_iter().map(|candidate| candidate.parameter));
    FormulaTransfer {
        typed_parameter_count: transferred.saturating_sub(legacy_transfer.parameters),
        definition_chain_parameter_count,
        relation_program_parameter_count,
        legacy_parameter_count: legacy_transfer.parameters,
        legacy_selector_parameter_count: legacy_transfer.selector_parameters,
        legacy_formula_count: legacy_transfer.formulas,
        consumed_object_records,
    }
}

/// Add only typed scalar values from the exact two-definition chain grammar.
///
/// The first definition names the parameter field and the second definition
/// names its source type. The suffix selector must already agree with the
/// first definition; the native decoder enforces that invariant. Other suffix
/// states remain native because they do not contain a neutral parameter value.
fn collect_definition_chain_parameters(
    native: &CatiaNative,
    graph_scope: Option<&HashSet<String>>,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    conflicting_inputs: &mut BTreeSet<ParameterId>,
) {
    for entity in native.entity_records.iter().filter(|entity| {
        graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
        let Some(chain) = entity.definition_chain_value.as_ref() else {
            continue;
        };
        let Some(candidate) = definition_chain_parameter_candidate(entity, chain) else {
            continue;
        };
        let id = candidate.parameter.id.clone();
        match candidates.get(&id) {
            None => {
                candidates.insert(id, candidate);
            }
            Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
                conflicting_inputs.insert(id);
            }
            Some(_) => {}
        }
    }
}

fn definition_chain_parameter_candidate(
    entity: &crate::native::CatiaEntityRecord,
    chain: &crate::native::CatiaDefinitionChainValue,
) -> Option<FormulaParameterCandidate> {
    let parameter_type = canonical_parameter_type(&chain.role.value)?;
    let (evaluation, evaluation_opcode_offset, atom_value) = match &chain.value {
        crate::native::CatiaEntitySuffixSchemaValue::Evaluation {
            opcode_offset,
            evaluation,
        } => (
            typed_parameter_evaluation(&chain.role.value, evaluation)?,
            Some(*opcode_offset),
            None,
        ),
        crate::native::CatiaEntitySuffixSchemaValue::Atom { value }
            if parameter_type == FormulaParameterType::Boolean =>
        {
            (
                TypedParameterEvaluation::Value(ParameterValue::Boolean(match value {
                    0 => false,
                    1 => true,
                    _ => return None,
                })),
                None,
                Some(*value),
            )
        }
        _ => return None,
    };
    let name = (!chain.selector.value.is_empty()).then(|| chain.selector.value.clone())?;
    let (expression, value) = match evaluation {
        TypedParameterEvaluation::Unset => (String::new(), None),
        TypedParameterEvaluation::Value(value) => {
            let expression = parameter_expression(&value);
            (expression, Some(value))
        }
    };
    // A definition chain names the parameter in its first definition. It does
    // not carry the named-parameter value record's scope/expression binding,
    // so do not publish the definition name as `catia_binding`.
    let mut properties = parameter_properties(parameter_type.as_str(), None);
    properties.insert(
        "catia_definition_selector_entry".to_string(),
        chain.selector.entry.clone(),
    );
    properties.insert(
        "catia_definition_selector_ordinal".to_string(),
        chain.selector.ordinal.to_string(),
    );
    properties.insert(
        "catia_definition_selector_offset".to_string(),
        chain.selector.offset.to_string(),
    );
    properties.insert(
        "catia_definition_role_entry".to_string(),
        chain.role.entry.clone(),
    );
    properties.insert(
        "catia_definition_role_ordinal".to_string(),
        chain.role.ordinal.to_string(),
    );
    properties.insert(
        "catia_definition_role_offset".to_string(),
        chain.role.offset.to_string(),
    );
    if let Some(opcode_offset) = evaluation_opcode_offset {
        properties.insert(
            "catia_definition_evaluation_opcode_offset".to_string(),
            opcode_offset.to_string(),
        );
    }
    if let Some(atom_value) = atom_value {
        properties.insert(
            "catia_definition_value_kind".to_string(),
            "atom".to_string(),
        );
        properties.insert(
            "catia_definition_atom_value".to_string(),
            atom_value.to_string(),
        );
    }
    Some(FormulaParameterCandidate {
        parameter: DesignParameter {
            id: neutral_parameter_id(&entity.id),
            owner: None,
            ordinal: 0,
            name,
            expression,
            display: None,
            value,
            dependencies: Vec::new(),
            properties,
            pmi: None,
            native_ref: Some(entity.id.clone()),
        },
        parameter_type,
        role: FormulaParameterRole::Input,
        source_order: entity.byte_offset,
    })
}

#[derive(Default)]
pub(crate) struct FormulaTransfer {
    pub(crate) typed_parameter_count: usize,
    pub(crate) definition_chain_parameter_count: usize,
    pub(crate) relation_program_parameter_count: usize,
    pub(crate) legacy_parameter_count: usize,
    pub(crate) legacy_selector_parameter_count: usize,
    pub(crate) legacy_formula_count: usize,
    pub(crate) consumed_object_records: HashSet<String>,
}

#[derive(Default)]
struct LegacyParameterTransfer {
    parameters: usize,
    selector_parameters: usize,
    formulas: usize,
}

#[derive(Clone, Copy)]
enum LegacyModelingScope<'a> {
    Unbounded,
    Unresolved,
    Container(&'a crate::native::CatiaOuterContainerBinding),
}

fn collect_legacy_parameters(
    native: &CatiaNative,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    modeling_scope: LegacyModelingScope<'_>,
) -> LegacyParameterTransfer {
    let mut transfer = LegacyParameterTransfer::default();
    for run in native
        .legacy_entity_runs
        .iter()
        .filter(|run| outer_container_in_scope(run.outer_container.as_ref(), modeling_scope))
    {
        let mut parameters_by_entity = HashMap::<u32, Vec<ParameterId>>::new();
        let mut parameters_by_name = HashMap::<String, Vec<ParameterId>>::new();
        for scalar in &run.scalar_values {
            if scalar.encoding != crate::native::CatiaLegacyScalarEncoding::Named84 {
                continue;
            }
            let Some(name) = &scalar.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_legacy_type(run, scalar.entity_id) else {
                continue;
            };
            let evaluation = match scalar.evaluation {
                crate::native::CatiaLegacyScalarEvaluation::Value { bits } => {
                    crate::native::CatiaEntityEvaluation::Scalar { bits }
                }
                crate::native::CatiaLegacyScalarEvaluation::Unset => {
                    crate::native::CatiaEntityEvaluation::Unset
                }
            };
            let Some(evaluation) = typed_parameter_evaluation(value_type, &evaluation) else {
                continue;
            };
            let parameter_type = canonical_parameter_type(value_type)
                .expect("typed evaluation requires a supported type");
            let (expression, value) = match evaluation {
                TypedParameterEvaluation::Unset => (String::new(), None),
                TypedParameterEvaluation::Value(value) => {
                    let expression = parameter_expression(&value);
                    (expression, Some(value))
                }
            };
            let Some(key) = scalar.id.strip_prefix("catia:legacy:scalar#") else {
                continue;
            };
            let id = ParameterId(format!("catia:legacy:parameter#{key}"));
            if candidates.contains_key(&id) {
                continue;
            }
            candidates.insert(
                id.clone(),
                FormulaParameterCandidate {
                    parameter: DesignParameter {
                        id: id.clone(),
                        owner: None,
                        ordinal: 0,
                        name: name.clone(),
                        expression,
                        display: None,
                        value,
                        dependencies: Vec::new(),
                        properties: parameter_properties(parameter_type.as_str(), None),
                        pmi: None,
                        native_ref: Some(run.id.clone()),
                    },
                    parameter_type,
                    role: FormulaParameterRole::Input,
                    source_order: scalar.byte_offset,
                },
            );
            parameters_by_entity
                .entry(scalar.entity_id)
                .or_default()
                .push(id.clone());
            parameters_by_name.entry(name.clone()).or_default().push(id);
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        for string in &run.string_values {
            let Some(name) = &string.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_or_intrinsic_legacy_type(
                run,
                string.entity_id,
                string.byte_offset,
                string.name_field,
                name,
                "String",
            ) else {
                continue;
            };
            if value_type != "String" {
                continue;
            }
            let Some(key) = string.id.strip_prefix("catia:legacy:string#") else {
                continue;
            };
            let id = ParameterId(format!("catia:legacy:parameter#{key}"));
            if candidates.contains_key(&id) {
                continue;
            }
            let value = ParameterValue::String(string.value.clone());
            candidates.insert(
                id.clone(),
                FormulaParameterCandidate {
                    parameter: DesignParameter {
                        id: id.clone(),
                        owner: None,
                        ordinal: 0,
                        name: name.clone(),
                        expression: parameter_expression(&value),
                        display: None,
                        value: Some(value),
                        dependencies: Vec::new(),
                        properties: parameter_properties("String", None),
                        pmi: None,
                        native_ref: Some(run.id.clone()),
                    },
                    parameter_type: FormulaParameterType::String,
                    role: FormulaParameterRole::Input,
                    source_order: string.byte_offset,
                },
            );
            parameters_by_entity
                .entry(string.entity_id)
                .or_default()
                .push(id.clone());
            parameters_by_name.entry(name.clone()).or_default().push(id);
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        for integer in &run.integer_values {
            let Some(name) = &integer.name else {
                continue;
            };
            let Some((value_type, selected)) = resolved_or_intrinsic_legacy_type(
                run,
                integer.entity_id,
                integer.byte_offset,
                integer.name_field,
                name,
                "Integer",
            ) else {
                continue;
            };
            if !matches!(value_type, "Integer" | "I") {
                continue;
            }
            let Some(key) = integer.id.strip_prefix("catia:legacy:integer#") else {
                continue;
            };
            let id = ParameterId(format!("catia:legacy:parameter#{key}"));
            if candidates.contains_key(&id) {
                continue;
            }
            let value = ParameterValue::Integer(i64::from(integer.value));
            candidates.insert(
                id.clone(),
                FormulaParameterCandidate {
                    parameter: DesignParameter {
                        id: id.clone(),
                        owner: None,
                        ordinal: 0,
                        name: name.clone(),
                        expression: parameter_expression(&value),
                        display: None,
                        value: Some(value),
                        dependencies: Vec::new(),
                        properties: parameter_properties("Integer", None),
                        pmi: None,
                        native_ref: Some(run.id.clone()),
                    },
                    parameter_type: FormulaParameterType::Integer,
                    role: FormulaParameterRole::Input,
                    source_order: integer.byte_offset,
                },
            );
            parameters_by_entity
                .entry(integer.entity_id)
                .or_default()
                .push(id.clone());
            parameters_by_name.entry(name.clone()).or_default().push(id);
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        let mut relations_by_parameter =
            HashMap::<u32, Vec<&crate::native::CatiaLegacyRelation>>::new();
        for relation in &run.relations {
            if let Some(parameter) = relation.parameter_entity_id {
                relations_by_parameter
                    .entry(parameter)
                    .or_default()
                    .push(relation);
            }
        }
        for (entity_id, relations) in relations_by_parameter {
            let ([parameter], [relation]) = (
                parameters_by_entity
                    .get(&entity_id)
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                relations.as_slice(),
            ) else {
                continue;
            };
            let Some(evaluation) =
                legacy_relation_evaluation(relation, &parameters_by_name, candidates)
            else {
                continue;
            };
            let Some(candidate) = candidates.get_mut(parameter) else {
                continue;
            };
            if canonical_parameter_type(evaluation.source_type) != Some(candidate.parameter_type) {
                continue;
            }
            if let Some(stored) = candidate.parameter.value.clone() {
                if !evaluation
                    .evaluated
                    .agrees_with(&TypedParameterEvaluation::Value(stored))
                {
                    continue;
                }
            }
            candidate.parameter.expression = evaluation.expression.to_string();
            candidate.parameter.dependencies = evaluation.dependencies;
            candidate.role = FormulaParameterRole::FormulaOutput { fallback: None };
            transfer.formulas += 1;
        }
    }
    transfer
}

#[cfg(test)]
fn evaluate_legacy_output_assignment(
    source: &str,
    output_parameter: &str,
) -> Option<EvaluatedFormulaValue> {
    let expression = legacy_output_assignment_expression(source, output_parameter)?;
    evaluate_formula_expression(expression, &BTreeMap::new())
}

struct LegacyRelationEvaluation<'a> {
    source_type: &'a str,
    expression: &'a str,
    evaluated: EvaluatedFormulaValue,
    dependencies: Vec<ParameterId>,
}

// A legacy relation is evaluable only when its signature names a unique,
// typed, same-run packet for every input. This is the complete local binding
// rule; unresolved selector namespaces never participate in the join.
fn legacy_relation_evaluation<'a>(
    relation: &'a crate::native::CatiaLegacyRelation,
    parameters_by_name: &HashMap<String, Vec<ParameterId>>,
    candidates: &BTreeMap<ParameterId, FormulaParameterCandidate>,
) -> Option<LegacyRelationEvaluation<'a>> {
    let (source_type, expression) = match relation.output.as_ref() {
        Some(output) if relation.result_type == "VoidType" => (
            output.value_type.as_str(),
            legacy_output_assignment_expression(&relation.expression, &output.parameter)?,
        ),
        None if relation.result_type != "VoidType" => {
            (relation.result_type.as_str(), relation.expression.as_str())
        }
        _ => return None,
    };
    let symbols = (!relation.inputs.is_empty())
        .then(|| crate::native::relation_symbols(&relation.expression));
    let mut bindings = BTreeMap::new();
    let mut dependencies = Vec::with_capacity(relation.inputs.len());
    for input in &relation.inputs {
        let symbols = symbols.as_ref()?;
        if !symbols
            .iter()
            .any(|(_, symbol)| legacy_symbol_matches_input(symbol, &input.parameter))
        {
            return None;
        }
        let [parameter_id] = parameters_by_name
            .get(&input.parameter)
            .map(Vec::as_slice)
            .unwrap_or_default()
        else {
            return None;
        };
        if dependencies.contains(parameter_id) {
            return None;
        }
        let candidate = candidates.get(parameter_id)?;
        if canonical_parameter_type(&input.value_type) != Some(candidate.parameter_type) {
            return None;
        }
        let value = candidate.parameter.value.as_ref()?;
        bindings.insert(
            input.parameter.as_str(),
            EvaluatedFormulaValue::from_parameter_value(value),
        );
        dependencies.push(parameter_id.clone());
    }
    let evaluated = evaluate_formula_expression(expression, &bindings)?;
    evaluated
        .satisfies_source_type(source_type)
        .then_some(LegacyRelationEvaluation {
            source_type,
            expression,
            evaluated,
            dependencies,
        })
}

fn legacy_symbol_matches_input(symbol: &str, input: &str) -> bool {
    let Some(suffix) = symbol.strip_prefix(input) else {
        return false;
    };
    let suffix = suffix.trim_start_matches(|character: char| character.is_ascii_whitespace());
    suffix.is_empty()
        || suffix.strip_prefix('/').is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn legacy_output_assignment_expression<'a>(
    source: &'a str,
    output_parameter: &str,
) -> Option<&'a str> {
    let source = source.trim_matches(|character: char| character.is_ascii_whitespace());
    let remainder = source.strip_prefix(output_parameter)?;
    let remainder = remainder.trim_start_matches(|character: char| character.is_ascii_whitespace());
    let remainder = remainder.strip_prefix('=')?;
    if remainder.starts_with('=') {
        return None;
    }
    let expression = remainder.trim_matches(|character: char| character.is_ascii_whitespace());
    (!expression.is_empty()).then_some(expression)
}

fn outer_container_in_scope(
    binding: Option<&crate::native::CatiaOuterContainerBinding>,
    modeling_scope: LegacyModelingScope<'_>,
) -> bool {
    match modeling_scope {
        LegacyModelingScope::Unbounded => true,
        LegacyModelingScope::Unresolved => false,
        LegacyModelingScope::Container(modeling_container) => binding == Some(modeling_container),
    }
}

fn resolved_legacy_type(
    run: &crate::native::CatiaLegacyEntityRun,
    mut entity_id: u32,
) -> Option<(&str, bool)> {
    let mut visited = HashSet::new();
    let mut selected = false;
    loop {
        if !visited.insert(entity_id) {
            return None;
        }
        let mut descriptors = run
            .type_descriptors
            .iter()
            .filter(|descriptor| descriptor.entity_id == entity_id);
        let descriptor = descriptors.next()?;
        if descriptors.next().is_some() {
            return None;
        }
        match &descriptor.value {
            crate::native::CatiaLegacyTypeValue::Name { value } => {
                return Some((value, selected));
            }
            crate::native::CatiaLegacyTypeValue::Selector { value } => {
                entity_id = *value;
                selected = true;
            }
        }
    }
}

fn resolved_or_intrinsic_legacy_type<'a>(
    run: &'a crate::native::CatiaLegacyEntityRun,
    entity_id: u32,
    value_offset: u64,
    name_field: Option<u64>,
    name: &str,
    intrinsic_type: &'static str,
) -> Option<(&'a str, bool)> {
    if let Some(resolved) = resolved_legacy_type(run, entity_id) {
        return Some(resolved);
    }
    if run
        .type_descriptors
        .iter()
        .any(|descriptor| descriptor.entity_id == entity_id)
    {
        return None;
    }
    let name_field = name_field?;
    crate::native::legacy_evaluated_value_name(
        &run.role_selectors,
        &run.text_fields,
        entity_id,
        value_offset,
    )
    .is_some_and(|field| field.byte_offset == name_field && field.value == name)
    .then_some((intrinsic_type, false))
}

struct FormulaParameterCandidate {
    parameter: DesignParameter,
    parameter_type: FormulaParameterType,
    role: FormulaParameterRole,
    source_order: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormulaParameterType {
    Length,
    Angle,
    Real,
    Integer,
    Boolean,
    String,
}

impl FormulaParameterType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Length => "LENGTH",
            Self::Angle => "ANGLE",
            Self::Real => "Real",
            Self::Integer => "Integer",
            Self::Boolean => "Boolean",
            Self::String => "String",
        }
    }
}

#[derive(Clone)]
enum FormulaParameterRole {
    Input,
    FormulaOutput {
        fallback: Option<(DesignParameter, FormulaParameterType)>,
    },
}

impl FormulaParameterRole {
    fn is_formula_output(&self) -> bool {
        matches!(self, Self::FormulaOutput { .. })
    }
}

fn typed_entity_parameter_candidate(
    entity: &crate::native::CatiaEntityRecord,
    parameter: &crate::native::CatiaParameterValue,
    source_type: &str,
) -> Option<FormulaParameterCandidate> {
    let evaluation = typed_parameter_evaluation(source_type, &parameter.evaluation)?;
    let parameter_type =
        canonical_parameter_type(source_type).expect("typed evaluation requires a supported type");
    let (expression, value) = match evaluation {
        TypedParameterEvaluation::Unset => (String::new(), None),
        TypedParameterEvaluation::Value(value) => {
            let expression = parameter_expression(&value);
            (expression, Some(value))
        }
    };
    Some(FormulaParameterCandidate {
        parameter: DesignParameter {
            id: neutral_parameter_id(&entity.id),
            owner: None,
            ordinal: 0,
            name: parameter.name.value.clone(),
            expression,
            display: None,
            value,
            dependencies: Vec::new(),
            properties: parameter_properties(
                parameter_type.as_str(),
                Some(parameter.binding.value.as_str()),
            ),
            pmi: None,
            native_ref: Some(entity.id.clone()),
        },
        parameter_type,
        role: FormulaParameterRole::Input,
        source_order: entity.byte_offset,
    })
}

fn typed_entity_parameter_candidate_for_source(
    entity: &crate::native::CatiaEntityRecord,
    source_type: &str,
) -> Option<FormulaParameterCandidate> {
    if let Some(parameter) = &entity.parameter_value {
        return typed_entity_parameter_candidate(entity, parameter, source_type);
    }
    let chain = entity.definition_chain_value.as_ref()?;
    let candidate = definition_chain_parameter_candidate(entity, chain)?;
    (canonical_parameter_type(source_type) == Some(candidate.parameter_type)).then_some(candidate)
}

struct FormulaProgramCandidate {
    relation_entity: String,
    expression_entity: String,
    output: ParameterId,
    inputs: Vec<ParameterId>,
    input_parameters: Vec<(DesignParameter, FormulaParameterType)>,
}

fn merge_formula_parameter_candidate(
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
    conflicting_inputs: &mut BTreeSet<ParameterId>,
    mut candidate: FormulaParameterCandidate,
) {
    match candidates.get(&candidate.parameter.id) {
        Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
            match (
                existing.role.is_formula_output(),
                candidate.role.is_formula_output(),
            ) {
                (true, true) => {}
                (true, false) => {
                    conflicting_inputs.insert(candidate.parameter.id);
                }
                (false, true) => {
                    conflicting_inputs.insert(candidate.parameter.id.clone());
                    candidates.insert(candidate.parameter.id.clone(), candidate);
                }
                (false, false) => {
                    conflicting_inputs.insert(candidate.parameter.id);
                }
            }
        }
        Some(existing)
            if !existing.role.is_formula_output() && candidate.role.is_formula_output() =>
        {
            candidate.role = FormulaParameterRole::FormulaOutput {
                fallback: Some((existing.parameter.clone(), existing.parameter_type)),
            };
            candidates.insert(candidate.parameter.id.clone(), candidate);
        }
        Some(existing)
            if existing.role.is_formula_output() && !candidate.role.is_formula_output() =>
        {
            if let FormulaParameterRole::FormulaOutput { fallback } = &mut candidates
                .get_mut(&candidate.parameter.id)
                .expect("candidate exists")
                .role
            {
                fallback.get_or_insert((candidate.parameter, candidate.parameter_type));
            }
        }
        Some(_) => {}
        None => {
            candidates.insert(candidate.parameter.id.clone(), candidate);
        }
    }
}

fn relation_program_output_candidate(
    relation_entity: &crate::native::CatiaEntityRecord,
    expression_entity: &crate::native::CatiaEntityRecord,
    output_entity: &crate::native::CatiaEntityRecord,
    expression: &crate::native::CatiaRelationExpression,
    signature: &crate::native::CatiaRelationTypeSignature,
    inputs: &[crate::native::CatiaRelationProgramInput],
    entities: &HashMap<&str, &crate::native::CatiaEntityRecord>,
) -> Option<(FormulaProgramCandidate, FormulaParameterCandidate)> {
    if inputs.len() != signature.inputs.len()
        || inputs
            .iter()
            .zip(&signature.inputs)
            .any(|(input, declared)| {
                input.parameter != declared.parameter || input.value_type != declared.input_type
            })
    {
        return None;
    }

    let mut dependencies = Vec::with_capacity(inputs.len());
    let mut input_parameters = Vec::with_capacity(inputs.len());
    let mut expression_bindings = BTreeMap::new();
    let mut type_bindings = BTreeMap::new();
    let mut all_inputs_complete = true;
    for input in inputs {
        let input_entity = input.entity.entity().and_then(|id| entities.get(id))?;
        let candidate =
            typed_entity_parameter_candidate_for_source(input_entity, &input.value_type)?;
        if dependencies.contains(&candidate.parameter.id) {
            return None;
        }
        dependencies.push(candidate.parameter.id.clone());
        type_bindings.insert(
            input.parameter.as_str(),
            static_formula_value(candidate.parameter_type)?,
        );
        match candidate.parameter.value.as_ref() {
            Some(value) => {
                expression_bindings.insert(
                    input.parameter.as_str(),
                    EvaluatedFormulaValue::from_parameter_value(value),
                );
            }
            None => all_inputs_complete = false,
        }
        input_parameters.push((candidate.parameter, candidate.parameter_type));
    }

    let type_checked_expression =
        evaluate_formula_expression_with_mode(&expression.expression.value, &type_bindings, false)
            .filter(|value| value.satisfies_source_type(&signature.result_type));
    let evaluated_expression = all_inputs_complete
        .then(|| evaluate_formula_expression(&expression.expression.value, &expression_bindings))
        .flatten()
        .filter(|value| value.satisfies_source_type(&signature.result_type));
    (if all_inputs_complete {
        evaluated_expression.as_ref()
    } else {
        type_checked_expression.as_ref()
    })?;
    let output_value = output_entity.parameter_value.as_ref()?;
    let output_id = neutral_parameter_id(&output_entity.id);
    if dependencies.contains(&output_id) {
        return None;
    }
    let value = typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)?;
    let accepted = match &value {
        TypedParameterEvaluation::Unset => true,
        TypedParameterEvaluation::Value(value) => {
            evaluated_expression.as_ref().is_some_and(|evaluated| {
                evaluated.agrees_with(&TypedParameterEvaluation::Value(value.clone()))
            })
        }
    };
    if !accepted {
        return None;
    }
    let parameter_type = canonical_parameter_type(&signature.result_type)
        .expect("typed evaluation requires a supported type");
    let candidate = FormulaParameterCandidate {
        parameter: DesignParameter {
            id: output_id.clone(),
            owner: None,
            ordinal: 0,
            name: output_value.name.value.clone(),
            expression: expression.expression.value.clone(),
            display: None,
            value: match value {
                TypedParameterEvaluation::Unset => None,
                TypedParameterEvaluation::Value(value) => Some(value),
            },
            dependencies: dependencies.clone(),
            properties: parameter_properties(
                parameter_type.as_str(),
                Some(output_value.binding.value.as_str()),
            ),
            pmi: None,
            native_ref: Some(output_entity.id.clone()),
        },
        parameter_type,
        role: FormulaParameterRole::FormulaOutput { fallback: None },
        source_order: output_entity.byte_offset,
    };
    Some((
        FormulaProgramCandidate {
            relation_entity: relation_entity.id.clone(),
            expression_entity: expression_entity.id.clone(),
            output: output_id,
            inputs: dependencies,
            input_parameters,
        },
        candidate,
    ))
}

fn formula_parameter_candidates_agree(
    existing: &FormulaParameterCandidate,
    candidate: &FormulaParameterCandidate,
) -> bool {
    if existing.source_order != candidate.source_order
        || existing.parameter_type != candidate.parameter_type
    {
        return false;
    }
    match (
        existing.role.is_formula_output(),
        candidate.role.is_formula_output(),
    ) {
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

fn formula_parameter_candidate_accepts_input(
    candidate: &FormulaParameterCandidate,
    input: &(DesignParameter, FormulaParameterType),
) -> bool {
    if candidate.parameter_type != input.1 {
        return false;
    }
    if candidate.role.is_formula_output() {
        formula_parameter_matches_input(&candidate.parameter, &input.0)
    } else {
        candidate.parameter == input.0
    }
}

fn demote_formula_output(candidate: &mut FormulaParameterCandidate) -> bool {
    let FormulaParameterRole::FormulaOutput {
        fallback: Some((input, parameter_type)),
    } = std::mem::replace(&mut candidate.role, FormulaParameterRole::Input)
    else {
        candidate.role = FormulaParameterRole::Input;
        return false;
    };
    candidate.parameter = input;
    candidate.parameter_type = parameter_type;
    true
}

enum TypedParameterEvaluation {
    Unset,
    Value(ParameterValue),
}

fn parameter_expression(value: &ParameterValue) -> String {
    match value {
        ParameterValue::Length(Length(value)) => format!("{value} mm"),
        ParameterValue::Angle(Angle(value)) => format!("{value} rad"),
        ParameterValue::Real(value) => value.to_string(),
        ParameterValue::Integer(value) => value.to_string(),
        ParameterValue::Boolean(value) => value.to_string(),
        ParameterValue::String(value) => string_literal_expression(value).unwrap_or_default(),
    }
}

fn string_literal_expression(value: &str) -> Option<String> {
    value
        .chars()
        .all(|character| character != '"' && character != '\\' && !character.is_control())
        .then(|| format!("\"{value}\""))
}

fn parameter_properties(
    parameter_type: &'static str,
    binding: Option<&str>,
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::from([("value_type".to_string(), parameter_type.to_string())]);
    if let Some(binding) = binding {
        properties.insert("catia_binding".to_string(), binding.to_string());
    }
    properties
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FormulaDimension {
    length: i32,
    angle: i32,
}

impl FormulaDimension {
    const SCALAR: Self = Self {
        length: 0,
        angle: 0,
    };
    const LENGTH: Self = Self {
        length: 1,
        angle: 0,
    };
    const ANGLE: Self = Self {
        length: 0,
        angle: 1,
    };

    fn product(self, right: Self) -> Option<Self> {
        Some(Self {
            length: self.length.checked_add(right.length)?,
            angle: self.angle.checked_add(right.angle)?,
        })
    }

    fn quotient(self, right: Self) -> Option<Self> {
        Some(Self {
            length: self.length.checked_sub(right.length)?,
            angle: self.angle.checked_sub(right.angle)?,
        })
    }

    fn square_root(self) -> Option<Self> {
        (self.length % 2 == 0 && self.angle % 2 == 0).then_some(Self {
            length: self.length / 2,
            angle: self.angle / 2,
        })
    }

    fn power(self, exponent: i32) -> Option<Self> {
        Some(Self {
            length: self.length.checked_mul(exponent)?,
            angle: self.angle.checked_mul(exponent)?,
        })
    }
}

fn formula_unit(unit: &str) -> Option<(FormulaDimension, f64)> {
    match unit {
        "micron" => Some((FormulaDimension::LENGTH, 0.001)),
        "mm" => Some((FormulaDimension::LENGTH, 1.0)),
        "cm" => Some((FormulaDimension::LENGTH, 10.0)),
        "m" => Some((FormulaDimension::LENGTH, 1_000.0)),
        "km" => Some((FormulaDimension::LENGTH, 1_000_000.0)),
        "in" => Some((FormulaDimension::LENGTH, 25.4)),
        "ft" => Some((FormulaDimension::LENGTH, 304.8)),
        "yard" => Some((FormulaDimension::LENGTH, 914.4)),
        "mile" => Some((FormulaDimension::LENGTH, 1_609_344.0)),
        "rad" => Some((FormulaDimension::ANGLE, 1.0)),
        "grad" => Some((FormulaDimension::ANGLE, std::f64::consts::PI / 200.0)),
        "deg" => Some((FormulaDimension::ANGLE, std::f64::consts::PI / 180.0)),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct EvaluatedFormulaScalar {
    value: f64,
    dimension: FormulaDimension,
    integral: Option<bool>,
    known_value: Option<f64>,
}

impl EvaluatedFormulaScalar {
    fn satisfies_source_type(self, source_type: &str) -> bool {
        match source_type {
            "LENGTH" => self.dimension == FormulaDimension::LENGTH,
            "ANGLE" => self.dimension == FormulaDimension::ANGLE,
            "Real" | "R" => self.dimension == FormulaDimension::SCALAR,
            "Integer" | "I" => {
                self.dimension == FormulaDimension::SCALAR
                    && self.integral == Some(true)
                    && self
                        .known_value
                        .is_none_or(|value| value >= i64::MIN as f64)
                    && self
                        .known_value
                        .is_none_or(|value| value < -(i64::MIN as f64))
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
enum EvaluatedFormulaValue {
    Scalar(EvaluatedFormulaScalar),
    Boolean(EvaluatedFormulaBoolean),
    String(EvaluatedFormulaString),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EvaluatedFormulaBoolean {
    Known(bool),
    Unknown,
}

impl EvaluatedFormulaBoolean {
    fn known(value: bool) -> Self {
        Self::Known(value)
    }

    fn unknown() -> Self {
        Self::Unknown
    }

    fn value(self) -> bool {
        match self {
            Self::Known(value) => value,
            Self::Unknown => false,
        }
    }

    fn known_value(self) -> Option<bool> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown => None,
        }
    }

    fn is_known(self) -> bool {
        matches!(self, Self::Known(_))
    }

    fn not(self) -> Self {
        self.known_value()
            .map_or(Self::Unknown, |value| Self::Known(!value))
    }

    fn and(self, right: Self) -> Self {
        match (self.known_value(), right.known_value()) {
            (Some(false), _) | (_, Some(false)) => Self::Known(false),
            (Some(true), Some(true)) => Self::Known(true),
            _ => Self::Unknown,
        }
    }

    fn or(self, right: Self) -> Self {
        match (self.known_value(), right.known_value()) {
            (Some(true), _) | (_, Some(true)) => Self::Known(true),
            (Some(false), Some(false)) => Self::Known(false),
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone)]
struct EvaluatedFormulaString {
    value: String,
    known: bool,
}

impl EvaluatedFormulaString {
    fn known(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            known: true,
        }
    }

    fn unknown() -> Self {
        Self {
            value: String::new(),
            known: false,
        }
    }
}

impl EvaluatedFormulaValue {
    fn from_parameter_value(value: &ParameterValue) -> Self {
        match value {
            ParameterValue::Length(Length(value)) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value,
                dimension: FormulaDimension::LENGTH,
                integral: finite_integrality(*value),
                known_value: Some(*value),
            }),
            ParameterValue::Angle(Angle(value)) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value,
                dimension: FormulaDimension::ANGLE,
                integral: finite_integrality(*value),
                known_value: Some(*value),
            }),
            ParameterValue::Real(value) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value,
                dimension: FormulaDimension::SCALAR,
                integral: finite_integrality(*value),
                known_value: Some(*value),
            }),
            ParameterValue::Integer(value) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value as f64,
                dimension: FormulaDimension::SCALAR,
                integral: Some(true),
                known_value: Some(*value as f64),
            }),
            ParameterValue::Boolean(value) => Self::Boolean(EvaluatedFormulaBoolean::known(*value)),
            ParameterValue::String(value) => Self::String(EvaluatedFormulaString::known(value)),
        }
    }

    fn scalar(self) -> Option<EvaluatedFormulaScalar> {
        match self {
            Self::Scalar(value) => Some(value),
            Self::Boolean(_) | Self::String(_) => None,
        }
    }

    fn boolean(self) -> Option<EvaluatedFormulaBoolean> {
        match self {
            Self::Boolean(value) => Some(value),
            Self::Scalar(_) | Self::String(_) => None,
        }
    }

    #[cfg(test)]
    fn string(self) -> Option<String> {
        match self {
            Self::String(value) => Some(value.value),
            Self::Scalar(_) | Self::Boolean(_) => None,
        }
    }

    fn satisfies_source_type(&self, source_type: &str) -> bool {
        match self {
            Self::Scalar(value) => value.satisfies_source_type(source_type),
            Self::Boolean(_) => source_type == "Boolean",
            Self::String(_) => source_type == "String",
        }
    }

    fn agrees_with(&self, evaluation: &TypedParameterEvaluation) -> bool {
        match evaluation {
            TypedParameterEvaluation::Unset => true,
            TypedParameterEvaluation::Value(value) => match (self, value) {
                (Self::Boolean(left), ParameterValue::Boolean(right)) => {
                    left.known_value() == Some(*right)
                }
                (Self::String(left), ParameterValue::String(right)) => left.value == *right,
                (
                    Self::Scalar(left),
                    value @ (ParameterValue::Length(_)
                    | ParameterValue::Angle(_)
                    | ParameterValue::Real(_)
                    | ParameterValue::Integer(_)),
                ) => {
                    let right = Self::from_parameter_value(value)
                        .scalar()
                        .expect("numeric parameter produces a scalar");
                    left.dimension == right.dimension && left.value == right.value
                }
                (Self::Boolean(_) | Self::String(_), _)
                | (Self::Scalar(_), ParameterValue::Boolean(_) | ParameterValue::String(_)) => {
                    false
                }
            },
        }
    }
}

struct FormulaExpressionParser<'a, 'b> {
    source: &'a str,
    at: usize,
    bindings: &'b BTreeMap<&'a str, EvaluatedFormulaValue>,
    evaluate: bool,
    static_check: bool,
}

const MAX_FORMULA_EXPRESSION_DEPTH: usize = 128;
const MAX_FORMULA_FUNCTION_ARGUMENTS: usize = 128;

fn finite_scalar(value: f64) -> Option<EvaluatedFormulaScalar> {
    value.is_finite().then_some(EvaluatedFormulaScalar {
        value,
        dimension: FormulaDimension::SCALAR,
        integral: finite_integrality(value),
        known_value: Some(value),
    })
}

fn finite_angle(value: f64) -> Option<EvaluatedFormulaScalar> {
    value.is_finite().then_some(EvaluatedFormulaScalar {
        value,
        dimension: FormulaDimension::ANGLE,
        integral: finite_integrality(value),
        known_value: Some(value),
    })
}

fn finite_integrality(value: f64) -> Option<bool> {
    value.is_finite().then_some(value.fract() == 0.0)
}

fn static_integral_result(value: f64, dimension: FormulaDimension) -> EvaluatedFormulaScalar {
    EvaluatedFormulaScalar {
        value,
        dimension,
        integral: Some(true),
        known_value: None,
    }
}

fn static_unknown_result(value: f64, dimension: FormulaDimension) -> EvaluatedFormulaScalar {
    EvaluatedFormulaScalar {
        value,
        dimension,
        integral: None,
        known_value: None,
    }
}

fn static_all_integral(left: Option<bool>, right: Option<bool>) -> Option<bool> {
    (left == Some(true) && right == Some(true)).then_some(true)
}

impl FormulaExpressionParser<'_, '_> {
    fn parse(mut self) -> Option<EvaluatedFormulaValue> {
        let value = self.conditional(0)?;
        self.skip_whitespace();
        (self.at == self.source.len()).then_some(value)
    }

    fn conditional(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let predicate = self.disjunction(depth)?;
        self.skip_whitespace();
        if self.peek() != Some(b'?') {
            return Some(predicate);
        }
        self.at += 1;
        let predicate = predicate.boolean()?;
        let evaluate = self.evaluate;
        let static_check = self.static_check;
        self.evaluate = evaluate && predicate.value();
        self.static_check = static_check && (!predicate.is_known() || predicate.value());
        let when_true = self.conditional(Self::nested_depth(depth)?)?;
        self.skip_whitespace();
        (self.peek()? == b';').then_some(())?;
        self.at += 1;
        self.evaluate = evaluate && !predicate.value();
        self.static_check = static_check && (!predicate.is_known() || !predicate.value());
        let when_false = self.conditional(Self::nested_depth(depth)?)?;
        self.evaluate = evaluate;
        self.static_check = static_check;
        Self::same_value_type(&when_true, &when_false)?;
        if evaluate {
            return Some(if predicate.value() {
                when_true
            } else {
                when_false
            });
        }
        Some(if let Some(predicate) = predicate.known_value() {
            if predicate {
                when_true
            } else {
                when_false
            }
        } else {
            Self::merge_static_values(&when_true, &when_false)?
        })
    }

    fn merge_static_values(
        left: &EvaluatedFormulaValue,
        right: &EvaluatedFormulaValue,
    ) -> Option<EvaluatedFormulaValue> {
        match (left, right) {
            (EvaluatedFormulaValue::Scalar(left), EvaluatedFormulaValue::Scalar(right))
                if left.dimension == right.dimension =>
            {
                Some(EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
                    value: 0.0,
                    dimension: left.dimension,
                    integral: match (left.integral, right.integral) {
                        (Some(left), Some(right)) if left == right => Some(left),
                        _ => None,
                    },
                    known_value: match (left.known_value, right.known_value) {
                        (Some(left), Some(right)) if left == right => Some(left),
                        _ => None,
                    },
                }))
            }
            (EvaluatedFormulaValue::Boolean(left), EvaluatedFormulaValue::Boolean(right)) => Some(
                EvaluatedFormulaValue::Boolean(match (left.known_value(), right.known_value()) {
                    (Some(left), Some(right)) if left == right => {
                        EvaluatedFormulaBoolean::known(left)
                    }
                    _ => EvaluatedFormulaBoolean::unknown(),
                }),
            ),
            (EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) => {
                Some(EvaluatedFormulaValue::String(
                    if left.known && right.known && left.value == right.value {
                        EvaluatedFormulaString::known(&left.value)
                    } else {
                        EvaluatedFormulaString::unknown()
                    },
                ))
            }
            _ => None,
        }
    }

    fn same_value_type(left: &EvaluatedFormulaValue, right: &EvaluatedFormulaValue) -> Option<()> {
        match (left, right) {
            (EvaluatedFormulaValue::Scalar(left), EvaluatedFormulaValue::Scalar(right))
                if left.dimension == right.dimension =>
            {
                Some(())
            }
            (EvaluatedFormulaValue::Boolean(_), EvaluatedFormulaValue::Boolean(_))
            | (EvaluatedFormulaValue::String(_), EvaluatedFormulaValue::String(_)) => Some(()),
            _ => None,
        }
    }

    fn scalar_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        if self.evaluate {
            finite_scalar(value)
        } else {
            Some(static_unknown_result(0.0, FormulaDimension::SCALAR))
        }
    }

    fn angle_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        if self.evaluate {
            finite_angle(value)
        } else {
            Some(static_unknown_result(0.0, FormulaDimension::ANGLE))
        }
    }

    fn integral_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        if self.evaluate {
            finite_scalar(value)
        } else {
            Some(static_integral_result(0.0, FormulaDimension::SCALAR))
        }
    }

    fn disjunction(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.conjunction(depth)?;
        loop {
            self.skip_whitespace();
            if !self.consume_keyword("or") {
                return Some(value);
            }
            let evaluate = self.evaluate;
            let static_check = self.static_check;
            let left = value.boolean()?;
            self.evaluate = evaluate && !left.value();
            self.static_check = static_check && (!left.is_known() || !left.value());
            let right = self.conjunction(depth)?;
            let right = right.boolean()?;
            self.evaluate = evaluate;
            self.static_check = static_check;
            value = EvaluatedFormulaValue::Boolean(if evaluate {
                EvaluatedFormulaBoolean::known(left.value() || right.value())
            } else {
                left.or(right)
            });
        }
    }

    fn conjunction(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.comparison(depth)?;
        loop {
            self.skip_whitespace();
            if !self.consume_keyword("and") {
                return Some(value);
            }
            let evaluate = self.evaluate;
            let static_check = self.static_check;
            let left = value.boolean()?;
            self.evaluate = evaluate && left.value();
            self.static_check = static_check && (!left.is_known() || left.value());
            let right = self.comparison(depth)?;
            let right = right.boolean()?;
            self.evaluate = evaluate;
            self.static_check = static_check;
            value = EvaluatedFormulaValue::Boolean(if evaluate {
                EvaluatedFormulaBoolean::known(left.value() && right.value())
            } else {
                left.and(right)
            });
        }
    }

    fn comparison(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let left = self.sum(depth)?;
        self.skip_whitespace();
        let operator = ["==", "<>", ">=", "<=", ">", "<"]
            .into_iter()
            .find(|operator| self.remaining().starts_with(operator));
        let Some(operator) = operator else {
            return Some(left);
        };
        self.at += operator.len();
        let right = self.sum(depth)?;
        let (value, known) = match (operator, left, right) {
            ("==", EvaluatedFormulaValue::Boolean(left), EvaluatedFormulaValue::Boolean(right)) => {
                (
                    left.value() == right.value(),
                    left.is_known() && right.is_known(),
                )
            }
            ("<>", EvaluatedFormulaValue::Boolean(left), EvaluatedFormulaValue::Boolean(right)) => {
                (
                    left.value() != right.value(),
                    left.is_known() && right.is_known(),
                )
            }
            ("==", EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) => {
                (left.value == right.value, left.known && right.known)
            }
            ("<>", EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) => {
                (left.value != right.value, left.known && right.known)
            }
            (
                operator,
                EvaluatedFormulaValue::Scalar(left),
                EvaluatedFormulaValue::Scalar(right),
            ) if left.dimension == right.dimension => (
                match operator {
                    "==" => left.value == right.value,
                    "<>" => left.value != right.value,
                    ">=" => left.value >= right.value,
                    "<=" => left.value <= right.value,
                    ">" => left.value > right.value,
                    "<" => left.value < right.value,
                    _ => unreachable!(),
                },
                left.known_value.is_some() && right.known_value.is_some(),
            ),
            _ => return None,
        };
        Some(EvaluatedFormulaValue::Boolean(if known {
            EvaluatedFormulaBoolean::known(value)
        } else {
            EvaluatedFormulaBoolean::unknown()
        }))
    }

    fn sum(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.product(depth)?;
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Some(value);
            };
            if !matches!(operator, b'+' | b'-') {
                return Some(value);
            }
            self.at += 1;
            let right = self.product(depth)?;
            if operator == b'+' {
                if let (EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) =
                    (&value, &right)
                {
                    let known = left.known && right.known;
                    let joined = if known {
                        let mut joined =
                            String::with_capacity(left.value.len().checked_add(right.value.len())?);
                        joined.push_str(&left.value);
                        joined.push_str(&right.value);
                        joined
                    } else {
                        String::new()
                    };
                    value = EvaluatedFormulaValue::String(EvaluatedFormulaString {
                        value: joined,
                        known,
                    });
                    continue;
                }
            }
            if operator == b'-' {
                if let (EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) =
                    (&value, &right)
                {
                    if (self.evaluate || self.static_check) && right.known && right.value.is_empty()
                    {
                        return None;
                    }
                    let known = left.known && right.known && !right.value.is_empty();
                    let string_value = if known {
                        left.value.replace(&right.value, "")
                    } else {
                        String::new()
                    };
                    value = EvaluatedFormulaValue::String(EvaluatedFormulaString {
                        value: string_value,
                        known,
                    });
                    continue;
                }
            }
            let mut left = value.scalar()?;
            let right = right.scalar()?;
            if left.dimension != right.dimension {
                return None;
            }
            let left_known = left.known_value;
            let right_known = right.known_value;
            let integral = if self.evaluate {
                None
            } else {
                static_all_integral(left.integral, right.integral)
            };
            let result_value = if operator == b'+' {
                left.value + right.value
            } else {
                left.value - right.value
            };
            if self.evaluate && !result_value.is_finite() {
                return None;
            }
            let known_value = if self.evaluate {
                Some(result_value)
            } else {
                left_known
                    .zip(right_known)
                    .map(|(left, right)| {
                        if operator == b'+' {
                            left + right
                        } else {
                            left - right
                        }
                    })
                    .filter(|value| value.is_finite())
            };
            if self.static_check
                && left_known.is_some()
                && right_known.is_some()
                && known_value.is_none()
            {
                return None;
            }
            left.value = if result_value.is_finite() {
                result_value
            } else {
                0.0
            };
            left.integral = if self.evaluate {
                finite_integrality(result_value)
            } else {
                integral
            };
            left.known_value = known_value;
            value = EvaluatedFormulaValue::Scalar(left);
        }
    }

    fn product(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.unary(depth)?;
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Some(value);
            };
            if !matches!(operator, b'*' | b'/') {
                return Some(value);
            }
            self.at += 1;
            let left = value.scalar()?;
            let right = self.unary(depth)?.scalar()?;
            let left_known = left.known_value;
            let right_known = right.known_value;
            if self.static_check
                && operator == b'/'
                && right_known.is_some_and(|value| value == 0.0)
            {
                return None;
            }
            let known_value = if operator == b'*' {
                left_known
                    .zip(right_known)
                    .map(|(left, right)| left * right)
                    .filter(|value| value.is_finite())
            } else if right.value == 0.0 {
                None
            } else if self.evaluate {
                Some(left.value / right.value)
            } else {
                left_known
                    .zip(right_known)
                    .map(|(left, right)| left / right)
                    .filter(|value| value.is_finite())
            };
            if self.static_check
                && left_known.is_some()
                && right_known.is_some()
                && known_value.is_none()
            {
                return None;
            }
            let result = if operator == b'*' {
                EvaluatedFormulaScalar {
                    value: left.value * right.value,
                    dimension: left.dimension.product(right.dimension)?,
                    integral: if self.evaluate {
                        None
                    } else {
                        static_all_integral(left.integral, right.integral)
                    },
                    known_value,
                }
            } else {
                if self.evaluate && right.value == 0.0 {
                    return None;
                }
                EvaluatedFormulaScalar {
                    value: if right.value == 0.0 {
                        0.0
                    } else {
                        left.value / right.value
                    },
                    dimension: left.dimension.quotient(right.dimension)?,
                    integral: None,
                    known_value,
                }
            };
            if self.evaluate && !result.value.is_finite() {
                return None;
            }
            value = EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
                value: if result.value.is_finite() {
                    result.value
                } else {
                    0.0
                },
                dimension: result.dimension,
                integral: if self.evaluate {
                    finite_integrality(result.value)
                } else {
                    result.integral
                },
                known_value: result.known_value,
            });
        }
    }

    fn unary(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        self.skip_whitespace();
        if self.consume_keyword("not") {
            let value = self.unary(Self::nested_depth(depth)?)?.boolean()?;
            return Some(EvaluatedFormulaValue::Boolean(value.not()));
        }
        match self.peek()? {
            b'+' => {
                self.at += 1;
                self.unary(Self::nested_depth(depth)?)
                    .and_then(EvaluatedFormulaValue::scalar)
                    .map(EvaluatedFormulaValue::Scalar)
            }
            b'-' => {
                self.at += 1;
                let mut value = self.unary(Self::nested_depth(depth)?)?.scalar()?;
                value.value = -value.value;
                value.known_value = value.known_value.map(|value| -value);
                Some(EvaluatedFormulaValue::Scalar(value))
            }
            _ => self.power(depth),
        }
    }

    fn power(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let base = self.postfix(depth)?;
        self.skip_whitespace();
        if !self.remaining().starts_with("**") {
            return Some(base);
        }
        self.at += 2;
        let base = base.scalar()?;
        let exponent = self.unary(Self::nested_depth(depth)?)?.scalar()?;
        if exponent.dimension != FormulaDimension::SCALAR {
            return None;
        }

        let dimension = if base.dimension == FormulaDimension::SCALAR {
            FormulaDimension::SCALAR
        } else {
            let exponent_value = exponent.known_value?;
            if exponent_value.fract() != 0.0
                || exponent_value < f64::from(i32::MIN)
                || exponent_value > f64::from(i32::MAX)
            {
                return None;
            }
            base.dimension.power(exponent_value as i32)?
        };
        let value = base.value.powf(exponent.value);
        let known_value = if self.evaluate {
            Some(value)
        } else {
            base.known_value
                .zip(exponent.known_value)
                .map(|(base, exponent)| base.powf(exponent))
                .filter(|value| value.is_finite())
        };
        if self.static_check
            && base.known_value.is_some()
            && exponent.known_value.is_some()
            && known_value.is_none()
        {
            return None;
        }
        (value.is_finite() || !self.evaluate).then_some(EvaluatedFormulaValue::Scalar(
            EvaluatedFormulaScalar {
                value: if value.is_finite() { value } else { 0.0 },
                dimension,
                integral: if self.evaluate {
                    finite_integrality(value)
                } else {
                    None
                },
                known_value,
            },
        ))
    }

    fn primary(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        self.skip_whitespace();
        if self.peek()? == b'"' {
            return self
                .string_literal()
                .map(EvaluatedFormulaString::known)
                .map(EvaluatedFormulaValue::String);
        }
        if self.consume_keyword("true") {
            return Some(EvaluatedFormulaValue::Boolean(
                EvaluatedFormulaBoolean::known(true),
            ));
        }
        if self.consume_keyword("false") {
            return Some(EvaluatedFormulaValue::Boolean(
                EvaluatedFormulaBoolean::known(false),
            ));
        }
        if self.peek()? == b'(' {
            self.at += 1;
            let value = self.conditional(Self::nested_depth(depth)?)?;
            self.skip_whitespace();
            (self.peek()? == b')').then_some(())?;
            self.at += 1;
            return Some(value);
        }
        if self.peek()? == b'#' {
            return self.symbol();
        }
        if self.remaining().starts_with("PI")
            && self
                .source
                .as_bytes()
                .get(self.at + 2)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            self.at += 2;
            return Some(EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
                value: std::f64::consts::PI,
                dimension: FormulaDimension::SCALAR,
                integral: Some(false),
                known_value: Some(std::f64::consts::PI),
            }));
        }
        if self.remaining().starts_with('E')
            && self
                .source
                .as_bytes()
                .get(self.at + 1)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            self.at += 1;
            return finite_scalar(std::f64::consts::E).map(EvaluatedFormulaValue::Scalar);
        }
        if self.peek()?.is_ascii_alphabetic() {
            return self.function_call(depth);
        }
        self.literal().map(EvaluatedFormulaValue::Scalar)
    }

    fn postfix(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.primary(depth)?;
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'.') {
                return Some(value);
            }
            self.at += 1;
            let method_start = self.at;
            while self
                .peek()
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                self.at += 1;
            }
            (self.at > method_start).then_some(())?;
            let method = &self.source[method_start..self.at];
            let arguments = self.function_arguments(Self::nested_depth(depth)?)?;
            value = match (method, value, arguments.as_slice()) {
                ("Length", EvaluatedFormulaValue::String(value), []) => {
                    let length = u32::try_from(value.value.chars().count()).ok()?;
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && value.known) {
                            finite_scalar(f64::from(length))?
                        } else {
                            static_integral_result(0.0, FormulaDimension::SCALAR)
                        },
                    )
                }
                (
                    "Search",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::String(needle)],
                ) => EvaluatedFormulaValue::Scalar(
                    if self.evaluate || (self.static_check && value.known && needle.known) {
                        let index = Self::search_string(&value.value, &needle.value, 0, true)?;
                        finite_scalar(index as f64)?
                    } else {
                        static_integral_result(0.0, FormulaDimension::SCALAR)
                    },
                ),
                (
                    "Search",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::String(needle), EvaluatedFormulaValue::Scalar(start)],
                ) => {
                    let start_value = *start;
                    let start = self.string_index(start_value)?;
                    let known = value.known && needle.known && start_value.known_value.is_some();
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && known) {
                            let index =
                                Self::search_string(&value.value, &needle.value, start, true)?;
                            finite_scalar(index as f64)?
                        } else {
                            static_integral_result(0.0, FormulaDimension::SCALAR)
                        },
                    )
                }
                (
                    "Search",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::String(needle), EvaluatedFormulaValue::Scalar(start), EvaluatedFormulaValue::Boolean(forward)],
                ) => {
                    let start = self.string_index(*start)?;
                    EvaluatedFormulaValue::Scalar(if self.evaluate {
                        let index = Self::search_string(
                            &value.value,
                            &needle.value,
                            start,
                            forward.value(),
                        )?;
                        finite_scalar(index as f64)?
                    } else {
                        static_integral_result(0.0, FormulaDimension::SCALAR)
                    })
                }
                (
                    "Extract",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::Scalar(start), EvaluatedFormulaValue::Scalar(length)],
                ) => {
                    let start_value = *start;
                    let length_value = *length;
                    let start = self.string_index(start_value)?;
                    let length = self.string_index(length_value)?;
                    let known = value.known
                        && start_value.known_value.is_some()
                        && length_value.known_value.is_some();
                    let string_value = if self.evaluate || (self.static_check && known) {
                        let end = start.checked_add(length)?;
                        let start = Self::string_boundary(&value.value, start)?;
                        let end = Self::string_boundary(&value.value, end)?;
                        value.value[start..end].to_string()
                    } else {
                        String::new()
                    };
                    EvaluatedFormulaValue::String(EvaluatedFormulaString {
                        value: string_value,
                        known: self.evaluate || (self.static_check && known),
                    })
                }
                ("ToReal", EvaluatedFormulaValue::String(value), []) => {
                    EvaluatedFormulaValue::Scalar(
                        if self.evaluate || (self.static_check && value.known) {
                            finite_scalar(value.value.parse::<f64>().ok()?)?
                        } else {
                            static_unknown_result(0.0, FormulaDimension::SCALAR)
                        },
                    )
                }
                _ => return None,
            };
        }
    }

    fn string_index(&self, value: EvaluatedFormulaScalar) -> Option<usize> {
        (value.dimension == FormulaDimension::SCALAR).then_some(())?;
        if (self.static_check || self.evaluate) && !value.satisfies_source_type("Integer") {
            return None;
        }
        if self.static_check && value.known_value.is_some_and(|value| value < 0.0) {
            return None;
        }
        if !self.evaluate {
            return value
                .known_value
                .and_then(|value| usize::try_from(value as i64).ok())
                .or(Some(0));
        }
        usize::try_from(value.value as i64).ok()
    }

    fn string_boundary(value: &str, index: usize) -> Option<usize> {
        if index == value.chars().count() {
            Some(value.len())
        } else {
            value.char_indices().nth(index).map(|(offset, _)| offset)
        }
    }

    fn search_string(value: &str, needle: &str, start: usize, forward: bool) -> Option<i64> {
        let character_count = value.chars().count();
        if start > character_count {
            return Some(-1);
        }
        let byte_offset = if forward {
            let start_byte = Self::string_boundary(value, start)?;
            value[start_byte..]
                .find(needle)
                .map(|offset| start_byte + offset)
        } else {
            let end_character = character_count.checked_sub(start)?;
            let end_byte = Self::string_boundary(value, end_character)?;
            value[..end_byte].rfind(needle)
        };
        byte_offset.map_or(Some(-1), |offset| {
            i64::try_from(value[..offset].chars().count()).ok()
        })
    }

    fn string_literal(&mut self) -> Option<String> {
        (self.peek()? == b'"').then_some(())?;
        self.at += 1;
        let start = self.at;
        while let Some(character) = self.source.get(self.at..)?.chars().next() {
            if character == '"' {
                let value = self.source.get(start..self.at)?.to_string();
                self.at += character.len_utf8();
                return Some(value);
            }
            if character.is_control() || character == '\\' {
                return None;
            }
            self.at += character.len_utf8();
        }
        None
    }

    fn function_call(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let function_start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let function = &self.source[function_start..self.at];
        let arguments = self.function_arguments(Self::nested_depth(depth)?)?;

        if function == "ReplaceSubText" {
            let [EvaluatedFormulaValue::String(source), EvaluatedFormulaValue::String(from), EvaluatedFormulaValue::String(to)] =
                arguments.as_slice()
            else {
                return None;
            };
            if (self.evaluate || self.static_check) && from.known && from.value.is_empty() {
                return None;
            }
            let known = source.known && from.known && to.known && !from.value.is_empty();
            let value = if self.evaluate || (self.static_check && known) {
                source.value.replace(&from.value, &to.value)
            } else {
                String::new()
            };
            return Some(EvaluatedFormulaValue::String(EvaluatedFormulaString {
                value,
                known: self.evaluate || (self.static_check && known),
            }));
        }

        if function == "ToString" {
            let [EvaluatedFormulaValue::Scalar(value)] = arguments.as_slice() else {
                return None;
            };
            if (self.static_check || self.evaluate) && !value.satisfies_source_type("Integer") {
                return None;
            }
            let known = value.known_value.is_some();
            let string_value = if self.evaluate || (self.static_check && known) {
                format!("{:.0}", value.value)
            } else {
                String::new()
            };
            return Some(EvaluatedFormulaValue::String(EvaluatedFormulaString {
                value: string_value,
                known: self.evaluate || (self.static_check && known),
            }));
        }

        if matches!(function, "ToUpper" | "ToLower") {
            let [EvaluatedFormulaValue::String(value)] = arguments.as_slice() else {
                return None;
            };
            let known = value.known;
            let string_value = if self.evaluate || (self.static_check && known) {
                if function == "ToUpper" {
                    value.value.to_uppercase()
                } else {
                    value.value.to_lowercase()
                }
            } else {
                String::new()
            };
            return Some(EvaluatedFormulaValue::String(EvaluatedFormulaString {
                value: string_value,
                known: self.evaluate || (self.static_check && known),
            }));
        }

        if function == "round" && arguments.len() == 3 {
            let [EvaluatedFormulaValue::Scalar(value), EvaluatedFormulaValue::String(unit), EvaluatedFormulaValue::Scalar(digits)] =
                arguments.as_slice()
            else {
                return None;
            };
            if !matches!(
                value.dimension,
                FormulaDimension::LENGTH | FormulaDimension::ANGLE
            ) || digits.dimension != FormulaDimension::SCALAR
            {
                return None;
            }
            let unit_spec = formula_unit(&unit.value);
            if let Some((unit_dimension, _)) = unit_spec {
                if value.dimension != unit_dimension {
                    return None;
                }
            } else if self.evaluate || (self.static_check && unit.known) {
                return None;
            }
            if (self.static_check || self.evaluate) && !digits.satisfies_source_type("Integer") {
                return None;
            }
            if self.static_check
                && digits
                    .known_value
                    .is_some_and(|value| value < 0.0 || value > f64::from(i32::MAX))
            {
                return None;
            }
            if !self.evaluate {
                return Some(EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
                    value: 0.0,
                    dimension: value.dimension,
                    integral: None,
                    known_value: None,
                }));
            }
            let (_, unit_scale) = unit_spec?;
            if digits.value < 0.0 || digits.value > f64::from(i32::MAX) {
                return None;
            }
            let quantum = unit_scale * 10.0_f64.powi(-(digits.value as i32));
            let rounded = if quantum == 0.0 {
                value.value
            } else {
                let scaled = value.value / quantum;
                if scaled.is_finite() {
                    scaled.round_ties_even() * quantum
                } else {
                    value.value
                }
            };
            return rounded.is_finite().then_some(EvaluatedFormulaValue::Scalar(
                EvaluatedFormulaScalar {
                    value: rounded,
                    dimension: value.dimension,
                    integral: finite_integrality(rounded),
                    known_value: Some(rounded),
                },
            ));
        }

        let arguments = arguments
            .into_iter()
            .map(EvaluatedFormulaValue::scalar)
            .collect::<Option<Vec<_>>>()?;

        if matches!(function, "min" | "max") {
            let mut arguments = arguments.into_iter();
            let mut result = arguments.next()?;
            for argument in arguments {
                if result.dimension != argument.dimension {
                    return None;
                }
                result.value = if function == "min" {
                    result.value.min(argument.value)
                } else {
                    result.value.max(argument.value)
                };
                result.integral = if self.evaluate {
                    finite_integrality(result.value)
                } else {
                    static_all_integral(result.integral, argument.integral)
                };
                result.known_value = if self.evaluate {
                    Some(result.value)
                } else {
                    result
                        .known_value
                        .zip(argument.known_value)
                        .map(|(result, argument)| {
                            if function == "min" {
                                result.min(argument)
                            } else {
                                result.max(argument)
                            }
                        })
                };
            }
            return Some(EvaluatedFormulaValue::Scalar(result));
        }

        if matches!(function, "LinearInterpolation" | "CubicInterpolation") {
            let [start, end, fraction] = arguments.as_slice() else {
                return None;
            };
            if start.dimension != end.dimension || fraction.dimension != FormulaDimension::SCALAR {
                return None;
            }
            let fraction_value = if function == "CubicInterpolation" {
                fraction.value * fraction.value * (3.0 - 2.0 * fraction.value)
            } else {
                fraction.value
            };
            let value = start.value + (end.value - start.value) * fraction_value;
            let known_value = if self.evaluate {
                Some(value)
            } else {
                start
                    .known_value
                    .zip(end.known_value)
                    .zip(fraction.known_value)
                    .map(|((start, end), fraction)| {
                        if function == "CubicInterpolation" {
                            let fraction = fraction * fraction * (3.0 - 2.0 * fraction);
                            start + (end - start) * fraction
                        } else {
                            start + (end - start) * fraction
                        }
                    })
                    .filter(|value| value.is_finite())
            };
            if self.static_check
                && start.known_value.is_some()
                && end.known_value.is_some()
                && fraction.known_value.is_some()
                && known_value.is_none()
            {
                return None;
            }
            return (value.is_finite() || !self.evaluate).then_some(EvaluatedFormulaValue::Scalar(
                EvaluatedFormulaScalar {
                    value: if value.is_finite() { value } else { 0.0 },
                    dimension: start.dimension,
                    integral: if self.evaluate {
                        finite_integrality(value)
                    } else {
                        None
                    },
                    known_value,
                },
            ));
        }

        let (first, second) = match arguments.as_slice() {
            [first] => (*first, None),
            [first, second] => (*first, Some(*second)),
            _ => return None,
        };
        let value = match (function, first, second) {
            ("sin", argument, None)
                if matches!(
                    argument.dimension,
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                self.scalar_result(argument.value.sin())
            }
            ("cos", argument, None)
                if matches!(
                    argument.dimension,
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                self.scalar_result(argument.value.cos())
            }
            ("tan", argument, None)
                if matches!(
                    argument.dimension,
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                self.scalar_result(argument.value.tan())
            }
            ("asin", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value
                            .is_none_or(|value| (-1.0..=1.0).contains(&value))) =>
            {
                self.angle_result(argument.value.asin())
            }
            ("acos", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value
                            .is_none_or(|value| (-1.0..=1.0).contains(&value))) =>
            {
                self.angle_result(argument.value.acos())
            }
            ("atan", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.angle_result(argument.value.atan())
            }
            ("log", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument.known_value.is_none_or(|value| value > 0.0))
                    && (!self.evaluate || argument.value > 0.0) =>
            {
                self.scalar_result(if self.evaluate {
                    argument.value.log10()
                } else {
                    0.0
                })
            }
            ("ln", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument.known_value.is_none_or(|value| value > 0.0))
                    && (!self.evaluate || argument.value > 0.0) =>
            {
                self.scalar_result(if self.evaluate {
                    argument.value.ln()
                } else {
                    0.0
                })
            }
            ("exp", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value
                            .is_none_or(|value| value.exp().is_finite())) =>
            {
                self.scalar_result(argument.value.exp())
            }
            ("sinh", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value
                            .is_none_or(|value| value.sinh().is_finite())) =>
            {
                self.scalar_result(argument.value.sinh())
            }
            ("cosh", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value
                            .is_none_or(|value| value.cosh().is_finite())) =>
            {
                self.scalar_result(argument.value.cosh())
            }
            ("tanh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.tanh())
            }
            ("asinh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.asinh())
            }
            ("acosh", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument.known_value.is_none_or(|value| value >= 1.0)) =>
            {
                self.scalar_result(argument.value.acosh())
            }
            ("atanh", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
                    && (!self.static_check
                        || argument
                            .known_value
                            .is_none_or(|value| (-1.0..1.0).contains(&value))) =>
            {
                self.scalar_result(argument.value.atanh())
            }
            ("ceil", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.integral_result(argument.value.ceil())
            }
            ("floor", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.integral_result(argument.value.floor())
            }
            ("int", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.integral_result(argument.value.trunc())
            }
            ("round", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.integral_result(argument.value.round_ties_even())
            }
            ("mod", dividend, Some(divisor))
                if dividend.dimension == FormulaDimension::SCALAR
                    && divisor.dimension == FormulaDimension::SCALAR
                    && (!self.static_check && !self.evaluate
                        || divisor.satisfies_source_type("Integer"))
                    && (!self.static_check || divisor.known_value != Some(0.0))
                    && (!self.evaluate || divisor.value != 0.0) =>
            {
                let result = if self.evaluate {
                    dividend.value.trunc() % divisor.value
                } else {
                    0.0
                };
                if self.evaluate {
                    self.scalar_result(result)
                } else {
                    Some(EvaluatedFormulaScalar {
                        value: result,
                        dimension: FormulaDimension::SCALAR,
                        integral: static_all_integral(dividend.integral, divisor.integral),
                        known_value: dividend.known_value.zip(divisor.known_value).and_then(
                            |(dividend, divisor)| {
                                (divisor != 0.0).then_some(dividend.trunc() % divisor)
                            },
                        ),
                    })
                }
            }
            ("abs", argument, None) => {
                if self.evaluate {
                    finite_integrality(argument.value.abs()).map(|integral| {
                        EvaluatedFormulaScalar {
                            value: argument.value.abs(),
                            dimension: argument.dimension,
                            integral: Some(integral),
                            known_value: Some(argument.value.abs()),
                        }
                    })
                } else {
                    Some(EvaluatedFormulaScalar {
                        value: 0.0,
                        dimension: argument.dimension,
                        integral: argument.integral,
                        known_value: argument.known_value.map(f64::abs),
                    })
                }
            }
            ("sqrt", argument, None)
                if (!self.static_check
                    || argument.known_value.is_none_or(|value| value >= 0.0))
                    && (!self.evaluate || argument.value >= 0.0) =>
            {
                Some(EvaluatedFormulaScalar {
                    value: if self.evaluate {
                        argument.value.sqrt()
                    } else {
                        0.0
                    },
                    dimension: argument.dimension.square_root()?,
                    integral: if self.evaluate {
                        finite_integrality(argument.value.sqrt())
                    } else {
                        None
                    },
                    known_value: if self.evaluate {
                        Some(argument.value.sqrt())
                    } else {
                        argument
                            .known_value
                            .map(f64::sqrt)
                            .filter(|value| value.is_finite())
                    },
                })
            }
            _ => None,
        }?;
        Some(EvaluatedFormulaValue::Scalar(value))
    }

    fn function_arguments(&mut self, depth: usize) -> Option<Vec<EvaluatedFormulaValue>> {
        self.skip_whitespace();
        (self.peek()? == b'(').then_some(())?;
        self.at += 1;
        let mut arguments = Vec::with_capacity(2);
        self.skip_whitespace();
        if self.peek()? == b')' {
            self.at += 1;
            return Some(arguments);
        }
        loop {
            arguments.push(self.conditional(depth)?);
            self.skip_whitespace();
            if self.peek()? == b')' {
                self.at += 1;
                break;
            }
            (self.peek()? == b',' && arguments.len() < MAX_FORMULA_FUNCTION_ARGUMENTS)
                .then_some(())?;
            self.at += 1;
        }
        Some(arguments)
    }

    fn symbol(&mut self) -> Option<EvaluatedFormulaValue> {
        let start = self.at;
        self.at += 1;
        let digits = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        (self.at > digits && self.peek()? == b'_').then_some(())?;
        self.at += 1;
        let name_end = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
        if self.peek() == Some(b'/') {
            self.at += 1;
            let ordinal = self.at;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
            (self.at > ordinal).then_some(())?;
        } else {
            self.at = name_end;
        }
        self.bindings.get(&self.source[start..name_end]).cloned()
    }

    fn literal(&mut self) -> Option<EvaluatedFormulaScalar> {
        let start = self.at;
        let mut saw_digit = false;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            saw_digit = true;
            self.at += 1;
        }
        if self.peek() == Some(b'.') {
            self.at += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                saw_digit = true;
                self.at += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            let exponent = self.at;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.at += 1;
            }
            (self.at > exponent).then_some(())?;
        }
        saw_digit.then_some(())?;
        let mut value = self.source[start..self.at].parse::<f64>().ok()?;
        let unit_boundary = self.at;
        self.skip_whitespace();
        let Some(unit) = [
            "micron", "mile", "yard", "grad", "rad", "deg", "mm", "cm", "km", "ft", "in", "m",
        ]
        .into_iter()
        .find(|unit| self.remaining().starts_with(unit)) else {
            self.at = unit_boundary;
            return value.is_finite().then_some(EvaluatedFormulaScalar {
                value,
                dimension: FormulaDimension::SCALAR,
                integral: finite_integrality(value),
                known_value: Some(value),
            });
        };
        let (dimension, scale) = formula_unit(unit)?;
        self.at += unit.len();
        value *= scale;
        value.is_finite().then_some(EvaluatedFormulaScalar {
            value,
            dimension,
            integral: finite_integrality(value),
            known_value: Some(value),
        })
    }

    fn skip_whitespace(&mut self) {
        while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        if !self.remaining().starts_with(keyword) {
            return false;
        }
        let before_is_identifier = self
            .source
            .as_bytes()
            .get(self.at.wrapping_sub(1))
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        let after_is_identifier = self
            .source
            .as_bytes()
            .get(self.at + keyword.len())
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
        if before_is_identifier || after_is_identifier {
            return false;
        }
        self.at += keyword.len();
        true
    }

    fn peek(&self) -> Option<u8> {
        self.source.as_bytes().get(self.at).copied()
    }

    fn remaining(&self) -> &str {
        &self.source[self.at..]
    }

    fn nested_depth(depth: usize) -> Option<usize> {
        (depth < MAX_FORMULA_EXPRESSION_DEPTH).then_some(depth + 1)
    }
}

fn evaluate_formula_expression<'a>(
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
) -> Option<EvaluatedFormulaValue> {
    evaluate_formula_expression_with_mode(source, bindings, true)
}

fn evaluate_formula_expression_with_mode<'a>(
    source: &'a str,
    bindings: &BTreeMap<&'a str, EvaluatedFormulaValue>,
    evaluate: bool,
) -> Option<EvaluatedFormulaValue> {
    FormulaExpressionParser {
        source,
        at: 0,
        bindings,
        evaluate,
        static_check: !evaluate,
    }
    .parse()
}

fn static_formula_value(parameter_type: FormulaParameterType) -> Option<EvaluatedFormulaValue> {
    Some(match parameter_type {
        FormulaParameterType::Length => EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
            value: 0.0,
            dimension: FormulaDimension::LENGTH,
            integral: None,
            known_value: None,
        }),
        FormulaParameterType::Angle => EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
            value: 0.0,
            dimension: FormulaDimension::ANGLE,
            integral: None,
            known_value: None,
        }),
        FormulaParameterType::Real => EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
            value: 0.5,
            dimension: FormulaDimension::SCALAR,
            integral: None,
            known_value: None,
        }),
        FormulaParameterType::Integer => EvaluatedFormulaValue::Scalar(EvaluatedFormulaScalar {
            value: 0.0,
            dimension: FormulaDimension::SCALAR,
            integral: Some(true),
            known_value: None,
        }),
        FormulaParameterType::Boolean => {
            EvaluatedFormulaValue::Boolean(EvaluatedFormulaBoolean::unknown())
        }
        FormulaParameterType::String => {
            EvaluatedFormulaValue::String(EvaluatedFormulaString::unknown())
        }
    })
}

fn typed_parameter_evaluation(
    source_type: &str,
    evaluation: &crate::native::CatiaEntityEvaluation,
) -> Option<TypedParameterEvaluation> {
    canonical_parameter_type(source_type)?;
    let bits = match evaluation {
        crate::native::CatiaEntityEvaluation::Unset => {
            return Some(TypedParameterEvaluation::Unset);
        }
        crate::native::CatiaEntityEvaluation::Scalar { bits } => bits,
    };
    if matches!(source_type, "Boolean" | "String") {
        return None;
    }
    let value = f64::from_bits(*bits);
    if !value.is_finite() {
        return None;
    }
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

fn canonical_parameter_type(source_type: &str) -> Option<FormulaParameterType> {
    match source_type {
        "LENGTH" => Some(FormulaParameterType::Length),
        "ANGLE" => Some(FormulaParameterType::Angle),
        "Real" | "R" => Some(FormulaParameterType::Real),
        "Integer" | "I" => Some(FormulaParameterType::Integer),
        "Boolean" => Some(FormulaParameterType::Boolean),
        "String" => Some(FormulaParameterType::String),
        _ => None,
    }
}

fn neutral_parameter_id(native_id: &str) -> ParameterId {
    ParameterId(crate::design_feature::neutral_history_id(
        native_id,
        "parameter",
    ))
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    fn unset_candidate(parameter_type: FormulaParameterType) -> FormulaParameterCandidate {
        FormulaParameterCandidate {
            parameter: DesignParameter {
                id: ParameterId("parameter".to_string()),
                owner: None,
                ordinal: 0,
                name: "Value".to_string(),
                expression: String::new(),
                display: None,
                value: None,
                dependencies: Vec::new(),
                properties: BTreeMap::new(),
                pmi: None,
                native_ref: Some("native-parameter".to_string()),
            },
            parameter_type,
            role: FormulaParameterRole::Input,
            source_order: 1,
        }
    }

    #[test]
    fn unset_parameter_candidates_require_one_canonical_type() {
        assert!(formula_parameter_candidates_agree(
            &unset_candidate(FormulaParameterType::Real),
            &unset_candidate(FormulaParameterType::Real)
        ));
        assert!(!formula_parameter_candidates_agree(
            &unset_candidate(FormulaParameterType::Length),
            &unset_candidate(FormulaParameterType::Real)
        ));
    }

    #[test]
    fn string_parameter_expressions_match_literal_grammar() {
        let literal = ParameterValue::String("Cilas Evans".to_string());
        assert_eq!(parameter_expression(&literal), "\"Cilas Evans\"");
        assert!(string_literal_expression("").is_some_and(|expression| expression == "\"\""));
        for value in ["quote\"", "backslash\\", "line\n", "control\u{0085}"] {
            assert!(string_literal_expression(value).is_none(), "{value:?}");
            assert!(parameter_expression(&ParameterValue::String(value.to_string())).is_empty());
        }
        assert_eq!(
            evaluate_formula_expression("\"Cilas Evans\"", &BTreeMap::new())
                .and_then(EvaluatedFormulaValue::string),
            Some("Cilas Evans".to_string())
        );
    }

    #[test]
    fn legacy_output_assignment_requires_one_exact_assignment() {
        let value = evaluate_legacy_output_assignment("#1_ = 2 + 3", "#1_")
            .and_then(EvaluatedFormulaValue::scalar)
            .expect("numeric assignment result");
        assert_eq!(value.value, 5.0);
        assert!(evaluate_legacy_output_assignment("#1_ == 5", "#1_").is_none());
        assert!(evaluate_legacy_output_assignment("#1_ = 2 = 3", "#1_").is_none());
        assert!(evaluate_legacy_output_assignment("#2_ = 5", "#1_").is_none());
        assert_eq!(
            evaluate_legacy_output_assignment("#1_ = \"a=b\"", "#1_")
                .and_then(EvaluatedFormulaValue::string),
            Some("a=b".to_string())
        );
    }

    #[test]
    fn static_formula_check_preserves_type_closure_without_values() {
        let bindings = BTreeMap::from([
            (
                "#1_",
                static_formula_value(FormulaParameterType::Length).expect("length type"),
            ),
            (
                "#2_",
                static_formula_value(FormulaParameterType::Integer).expect("integer type"),
            ),
            (
                "#3_",
                static_formula_value(FormulaParameterType::Boolean).expect("Boolean type"),
            ),
            (
                "#4_",
                static_formula_value(FormulaParameterType::String).expect("String type"),
            ),
            (
                "#5_",
                static_formula_value(FormulaParameterType::Real).expect("Real type"),
            ),
        ]);

        assert!(
            evaluate_formula_expression_with_mode("#1_ /2+1mm", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type("LENGTH"))
        );
        assert!(
            evaluate_formula_expression_with_mode("#3_ ? #2_ ; 1", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type("Integer"))
        );
        assert!(
            evaluate_formula_expression_with_mode("#4_", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type("String"))
        );
        assert!(
            evaluate_formula_expression_with_mode("ToString(#2_)", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type("String"))
        );
        assert!(
            evaluate_formula_expression_with_mode("(#1_) / #2_", &bindings, false)
                .is_some_and(|value| value.satisfies_source_type("LENGTH"))
        );
        assert!(
            evaluate_formula_expression_with_mode("#5_ + 0", &bindings, false)
                .is_none_or(|value| !value.satisfies_source_type("Integer"))
        );
        assert!(evaluate_formula_expression_with_mode("#1_ ** #2_", &bindings, false).is_none());
        assert!(
            evaluate_formula_expression_with_mode("#1_ ** 2", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| {
                    value.dimension
                        == FormulaDimension {
                            length: 2,
                            angle: 0,
                        }
                })
        );
    }

    #[test]
    fn static_formula_check_does_not_use_placeholder_values_as_facts() {
        let bindings = BTreeMap::from([
            (
                "#1_",
                static_formula_value(FormulaParameterType::Length).expect("length type"),
            ),
            (
                "#2_",
                static_formula_value(FormulaParameterType::Integer).expect("integer type"),
            ),
            (
                "#3_",
                static_formula_value(FormulaParameterType::Real).expect("real type"),
            ),
            (
                "#4_",
                static_formula_value(FormulaParameterType::String).expect("string type"),
            ),
        ]);

        let unknown_predicate = evaluate_formula_expression_with_mode("#3_ > 1", &bindings, false)
            .and_then(EvaluatedFormulaValue::boolean)
            .expect("Boolean comparison type");
        assert_eq!(unknown_predicate.known_value(), None);

        let length = evaluate_formula_expression_with_mode("#4_.Length()", &bindings, false)
            .and_then(EvaluatedFormulaValue::scalar)
            .expect("string length type");
        assert_eq!(length.integral, Some(true));
        assert_eq!(length.known_value, None);

        let known_length =
            evaluate_formula_expression_with_mode("\"text\".Length()", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("known string length");
        assert_eq!(known_length.known_value, Some(4.0));

        let known_real =
            evaluate_formula_expression_with_mode("\"12.5\".ToReal()", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("known string-to-real value");
        assert_eq!(known_real.known_value, Some(12.5));

        let parsed = evaluate_formula_expression_with_mode("#4_.ToReal()", &bindings, false)
            .and_then(EvaluatedFormulaValue::scalar)
            .expect("string-to-real type");
        assert_eq!(parsed.known_value, None);

        for expression in [
            "#4_ + \"suffix\"",
            "#4_.Search(\"x\")",
            "#4_.Search(\"x\", #2_)",
            "#4_.Extract(#2_, 1)",
            "ReplaceSubText(#4_, \"x\", \"y\")",
            "ToUpper(#4_)",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_some(),
                "{expression}"
            );
        }

        for expression in [
            "false and (1 / 0 > 2)",
            "true or (1 / 0 > 2)",
            "false ? 1 / 0 ; 5",
            "true ? 5 ; 1 / 0",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_some(),
                "{expression}"
            );
        }
        for expression in [
            "#3_ > 1 and (1 / 0 > 2)",
            "#3_ > 1 ? 5 ; 1 / 0",
            "false ? 1 / 0 ; 1mm",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_none(),
                "{expression}"
            );
        }

        for expression in [
            "ToString(#3_)",
            "#4_.Extract(#3_, 1)",
            "round(#1_, \"mm\", #3_)",
            "mod(2, #3_)",
            "1 / 0",
            "1e308 + 1e308",
            "1e308 * 1e308",
            "LinearInterpolation(1e308, -1e308, 0.5)",
            "exp(10000)",
            "sinh(10000)",
            "cosh(10000)",
            "acosh(0)",
            "atanh(1)",
            "#4_.Extract(-1, 1)",
            "\"\".ToReal()",
            "\"not a number\".ToReal()",
            "\"text\".Extract(3, 2)",
            "ReplaceSubText(\"text\", \"\", \"x\")",
            "sqrt(-1)",
        ] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_none(),
                "{expression}"
            );
        }
        assert!(evaluate_formula_expression_with_mode("1 * 0", &bindings, false).is_some());
        for expression in ["exp(#3_)", "acosh(#3_)", "atanh(#3_)"] {
            assert!(
                evaluate_formula_expression_with_mode(expression, &bindings, false).is_some(),
                "{expression}"
            );
        }

        assert!(
            evaluate_formula_expression_with_mode("round(#1_, \"mm\", #2_)", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| value.dimension == FormulaDimension::LENGTH)
        );
        assert!(
            evaluate_formula_expression_with_mode("round(#1_, #4_, #2_)", &bindings, false)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| value.dimension == FormulaDimension::LENGTH)
        );
        assert!(evaluate_formula_expression_with_mode(
            "#3_ > 1 ? round(#1_, #4_, #2_) ; 5mm",
            &bindings,
            false,
        )
        .and_then(EvaluatedFormulaValue::scalar)
        .is_some_and(|value| value.dimension == FormulaDimension::LENGTH));
        assert!(
            evaluate_formula_expression_with_mode("round(#3_, #4_, #2_)", &bindings, false)
                .is_none()
        );
    }

    #[test]
    fn typed_numeric_evaluations_require_finite_values() {
        for source_type in ["LENGTH", "ANGLE", "Real", "R", "Integer", "I"] {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let evaluation = crate::native::CatiaEntityEvaluation::Scalar {
                    bits: value.to_bits(),
                };
                assert!(
                    typed_parameter_evaluation(source_type, &evaluation).is_none(),
                    "{source_type} accepted non-finite value {value:?}"
                );
            }
        }

        assert!(matches!(
            typed_parameter_evaluation(
                "LENGTH",
                &crate::native::CatiaEntityEvaluation::Scalar {
                    bits: 12.5_f64.to_bits(),
                }
            ),
            Some(TypedParameterEvaluation::Value(ParameterValue::Length(
                Length(12.5)
            )))
        ));
        assert!(matches!(
            typed_parameter_evaluation(
                "Integer",
                &crate::native::CatiaEntityEvaluation::Scalar {
                    bits: (-7.0_f64).to_bits(),
                }
            ),
            Some(TypedParameterEvaluation::Value(ParameterValue::Integer(-7)))
        ));
    }

    #[test]
    fn formula_function_argument_count_is_bounded() {
        let bindings = BTreeMap::new();
        for (argument_count, accepted) in [(128, true), (129, false)] {
            let expression = format!("max({})", vec!["1"; argument_count].join(","));
            assert_eq!(
                evaluate_formula_expression(&expression, &bindings).is_some(),
                accepted,
                "{argument_count}"
            );
        }
    }

    #[test]
    fn formula_dimensioned_rounding_uses_a_compatible_unit_and_decimal_precision() {
        let bindings = BTreeMap::new();
        for (expression, expected, dimension) in [
            ("round(12.333mm,\"mm\",1)", 12.3, FormulaDimension::LENGTH),
            ("round(1234mm,\"cm\",0)", 1_230.0, FormulaDimension::LENGTH),
            (
                "round(45.54deg,\"deg\",1)",
                45.5_f64.to_radians(),
                FormulaDimension::ANGLE,
            ),
        ] {
            let value = evaluate_formula_expression(expression, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("dimensioned rounded value");
            assert!(value.dimension == dimension, "{expression}");
            assert!(
                (value.value - expected).abs() <= f64::EPSILON * expected.abs(),
                "{expression}: {} != {expected}",
                value.value
            );
        }
        for expression in [
            "round(12.3,\"mm\",1)",
            "round(12.3mm,\"deg\",1)",
            "round(12.3mm,\"unknown\",1)",
            "round(12.3mm,\"mm\",-1)",
            "round(12.3mm,\"mm\",1.5)",
            "round(12.3mm,\"mm\",1mm)",
            "round(12.3mm,\"mm\")",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_interpolation_preserves_the_endpoint_dimension() {
        let bindings = BTreeMap::new();
        for (expression, expected, dimension) in [
            (
                "LinearInterpolation(10mm,30mm,0.25)",
                15.0,
                FormulaDimension::LENGTH,
            ),
            (
                "CubicInterpolation(10deg,30deg,0.5)",
                20.0_f64.to_radians(),
                FormulaDimension::ANGLE,
            ),
            (
                "LinearInterpolation(10,30,1.25)",
                35.0,
                FormulaDimension::SCALAR,
            ),
        ] {
            let value = evaluate_formula_expression(expression, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("typed interpolation");
            assert!(value.dimension == dimension, "{expression}");
            assert!(
                (value.value - expected).abs() <= f64::EPSILON * expected.abs().max(1.0),
                "{expression}: {} != {expected}",
                value.value
            );
        }
        for expression in [
            "LinearInterpolation(10mm,30deg,0.25)",
            "LinearInterpolation(10mm,30mm,0.25mm)",
            "CubicInterpolation(10deg,30,0.5)",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_ternary_depth_is_bounded() {
        let bindings = BTreeMap::new();
        for (depth, accepted) in [(128, true), (129, false)] {
            let expression = format!("{}1{}", "true ? ".repeat(depth), " ; 1".repeat(depth));
            assert_eq!(
                evaluate_formula_expression(&expression, &bindings).is_some(),
                accepted,
                "{depth}"
            );
        }
    }

    #[test]
    fn formula_comparisons_and_logical_operators_are_typed_and_precedenced() {
        let bindings = BTreeMap::new();
        assert!(
            evaluate_formula_expression("(3 > 2) and (1mm <= 2mm)", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value)
                .is_some_and(|value| value)
        );
        assert_eq!(
            evaluate_formula_expression("false or true and false", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(false)
        );
        assert_eq!(
            evaluate_formula_expression("1mm == 1cm", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(false)
        );
        assert_eq!(
            evaluate_formula_expression("true <> false", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(true)
        );
        assert_eq!(
            evaluate_formula_expression("not false and false or not (1 > 2)", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value),
            Some(true)
        );
        for expression in ["not 1", "not \"false\"", "notable", "not"] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_boolean_operators_and_ternaries_evaluate_lazily() {
        let bindings = BTreeMap::new();
        for (expression, expected) in [
            ("false and (1 / 0 > 2)", false),
            ("true or (sqrt(-1) > 2)", true),
            ("false and (asin(2) > 0rad)", false),
            ("true or (exp(10000) > 0)", true),
            ("false and (1e308 * 1e308 > 0)", false),
            ("true ? 5 ; 1 / 0", true),
            ("false ? sqrt(-1) ; 5", true),
            ("true ? 5 ; (-1) ** 0.5", true),
            ("false ? false ? 1 / 0 ; 2 ; 3", true),
            ("true ? 5 ; mod(2, 1.5)", true),
        ] {
            let value = evaluate_formula_expression(expression, &bindings)
                .expect("lazy expression is complete");
            match value {
                EvaluatedFormulaValue::Boolean(value) => {
                    assert_eq!(value.value(), expected, "{expression}");
                }
                EvaluatedFormulaValue::Scalar(value) => {
                    assert_eq!(
                        value.value,
                        if expression.ends_with("; 3") {
                            3.0
                        } else {
                            5.0
                        }
                    );
                }
                EvaluatedFormulaValue::String(_) => panic!("unexpected string for {expression}"),
            }
        }
        assert_eq!(
            evaluate_formula_expression(
                "true ? \"selected\" ; ReplaceSubText(\"text\",\"\",\"x\")",
                &bindings,
            )
            .and_then(EvaluatedFormulaValue::string)
            .as_deref(),
            Some("selected")
        );
        assert_eq!(
            evaluate_formula_expression("false ? ToString(1.5) ; \"selected\"", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("selected")
        );
        for expression in [
            "true ? \"selected\" ; \"text\".Extract(1.5, 2)",
            "true ? 5 ; \"text\".Search(\"e\", 1.5)",
            "true ? 5 ; \"not a number\".ToReal()",
            "true ? \"selected\" ; \"text\" - \"\"",
            "true ? 5mm ; round(12.3mm,\"mm\",-1)",
            "true ? 5mm ; round(12.3mm,\"unknown\",1)",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_some(),
                "{expression}"
            );
        }
        assert_eq!(
            evaluate_formula_expression("true ? \"selected\" ; \"text\".Extract(-1,1)", &bindings,)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("selected")
        );
        assert!(
            evaluate_formula_expression("false ? \"not a number\".ToReal() ; 5", &bindings,)
                .and_then(EvaluatedFormulaValue::scalar)
                .is_some_and(|value| value.value == 5.0)
        );
    }

    #[test]
    fn formula_ternaries_require_boolean_predicates_and_common_branch_types() {
        let bindings = BTreeMap::new();
        for expression in [
            "1 ? 2 ; 3",
            "false ? false ; 1",
            "true ? 1 ;",
            "true ? 1 : 2",
            "true and (1 / 0 > 2)",
            "false or (sqrt(-1) > 2)",
            "true ? 1 / 0 ; 5",
            "false ? 5 ; exp(10000)",
            "true ? 5 ; \"text\".Search(\"t\", 1mm)",
            "true ? \"selected\" ; \"text\".Extract(1mm, 1)",
            "true ? 5mm ; round(12.3mm,\"deg\",1)",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
        for expression in ["true ? 1mm ; 1rad", "false ? 1mm ; 1rad", "true ? 1 ; 1mm"] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
        for (expression, expected_dimension) in [
            ("true ? 1mm ; 2mm", FormulaDimension::LENGTH),
            ("false ? 1rad ; 2rad", FormulaDimension::ANGLE),
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings)
                    .and_then(EvaluatedFormulaValue::scalar)
                    .is_some_and(|value| value.dimension == expected_dimension),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_string_operations_preserve_typed_values() {
        let bindings = BTreeMap::from([
            (
                "#1_",
                EvaluatedFormulaValue::String(EvaluatedFormulaString::known("Cilas Evans")),
            ),
            (
                "#2_",
                EvaluatedFormulaValue::String(EvaluatedFormulaString::known("Evans")),
            ),
            (
                "#3_",
                EvaluatedFormulaValue::Scalar(finite_scalar(-1.0).expect("finite integer")),
            ),
        ]);
        assert_eq!(
            evaluate_formula_expression("#1_.Length()", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(|value| value.value),
            Some(11.0)
        );
        assert_eq!(
            evaluate_formula_expression("#1_ .Search(#2_)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(|value| value.value),
            Some(6.0)
        );
        assert_eq!(
            evaluate_formula_expression("#1_.Search(\"missing\")", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(|value| value.value),
            Some(-1.0)
        );
        assert_eq!(
            evaluate_formula_expression("ReplaceSubText(#1_,\"Cilas\",\"Easy\")", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("Easy Evans")
        );
        assert_eq!(
            evaluate_formula_expression("\"Revision\" + ToString(#3_)", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("Revision-1")
        );
        assert!(
            evaluate_formula_expression("\"Cilas Evans\" == #1_", &bindings)
                .and_then(EvaluatedFormulaValue::boolean)
                .map(EvaluatedFormulaBoolean::value)
                .is_some_and(|value| value)
        );
        assert_eq!(
            evaluate_formula_expression("\"Cilas Evans Evans\".Search(\"Evans\",7)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(|value| value.value),
            Some(12.0)
        );
        assert_eq!(
            evaluate_formula_expression(
                "\"Cilas Evans Evans\".Search(\"Evans\",0,false)",
                &bindings,
            )
            .and_then(EvaluatedFormulaValue::scalar)
            .map(|value| value.value),
            Some(12.0)
        );
        assert_eq!(
            evaluate_formula_expression("\"é猫x猫\".Search(\"猫\",2)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(|value| value.value),
            Some(3.0)
        );
        assert_eq!(
            evaluate_formula_expression("\"text\".Search(\"t\",5)", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(|value| value.value),
            Some(-1.0)
        );
        assert_eq!(
            evaluate_formula_expression("\"é猫x\".Extract(1,1)", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("猫")
        );
        assert_eq!(
            evaluate_formula_expression("\"é猫x\".Extract(3,0)", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("")
        );
        assert_eq!(
            evaluate_formula_expression("ToUpper(\"Mixed Straße\")", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("MIXED STRASSE")
        );
        assert_eq!(
            evaluate_formula_expression("ToLower(\"Mixed Case\")", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("mixed case")
        );
        assert_eq!(
            evaluate_formula_expression("\"12.5\".ToReal()", &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .map(|value| value.value),
            Some(12.5)
        );
        assert_eq!(
            evaluate_formula_expression("\"AAxxAA\" - \"AA\"", &bindings)
                .and_then(EvaluatedFormulaValue::string)
                .as_deref(),
            Some("xx")
        );
    }

    #[test]
    fn formula_string_operations_reject_untyped_or_incomplete_forms() {
        let bindings = BTreeMap::new();
        for expression in [
            "\"unterminated",
            "\"unsupported\\\\escape\"",
            "\"control\u{0085}\"",
            "\"text\" + 1",
            "ToString(1.5)",
            "ReplaceSubText(\"text\",\"\",\"x\")",
            "\"text\".Search(1)",
            "\"text\".Search(\"t\",-1)",
            "\"text\".Length(1)",
            "\"text\".Extract(1)",
            "\"text\".Extract(-1,1)",
            "\"text\".Extract(3,2)",
            "\"not a number\".ToReal()",
            "ToUpper(1)",
            "\"text\" - \"\"",
            "\"text\".Unknown()",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_logical_operators_reject_mixed_types_and_chained_comparisons() {
        let bindings = BTreeMap::new();
        for expression in [
            "1mm > 1rad",
            "true + 1",
            "true and 1",
            "false and 1",
            "true or 1",
            "1 < 2 < 3",
            "true >= false",
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
    }

    #[test]
    fn formula_length_literals_normalize_every_admitted_unit_to_millimetres() {
        let bindings = BTreeMap::new();
        for (literal, expected) in [
            ("1micron", 0.001),
            ("1mile", 1_609_344.0),
            ("1yard", 914.4),
            ("1mm", 1.0),
            ("1cm", 10.0),
            ("1km", 1_000_000.0),
            ("1ft", 304.8),
            ("1in", 25.4),
            ("1m", 1_000.0),
        ] {
            let actual = evaluate_formula_expression(literal, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("complete length literal");
            assert_eq!(actual.value, expected, "{literal}");
            assert!(actual.dimension == FormulaDimension::LENGTH, "{literal}");
        }
    }

    #[test]
    fn formula_angle_literals_normalize_every_admitted_unit_to_radians() {
        let bindings = BTreeMap::new();
        for (literal, expected) in [
            ("1rad", 1.0),
            ("1grad", std::f64::consts::PI / 200.0),
            ("1deg", std::f64::consts::PI / 180.0),
        ] {
            let actual = evaluate_formula_expression(literal, &bindings)
                .and_then(EvaluatedFormulaValue::scalar)
                .expect("complete angle literal");
            assert_eq!(actual.value, expected, "{literal}");
            assert!(actual.dimension == FormulaDimension::ANGLE, "{literal}");
        }
    }
}

#[cfg(test)]
mod tests;
