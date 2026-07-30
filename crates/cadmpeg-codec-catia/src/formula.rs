// SPDX-License-Identifier: Apache-2.0
//! Transfer of complete, evaluable CATIA formula programs to neutral parameters.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Angle, DesignParameter, Length, ParameterId, ParameterValue};
use cadmpeg_ir::{Annotations, Exactness};

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
    let mut programs = Vec::<FormulaProgramCandidate>::new();
    let formula_definition_counts = native
        .entity_records
        .iter()
        .filter(|entity| {
            graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
        })
        .filter_map(|entity| entity.formula_relation.as_ref()?.parameter.as_deref())
        .fold(
            HashMap::<ParameterId, usize>::new(),
            |mut counts, parameter| {
                *counts.entry(neutral_parameter_id(parameter)).or_default() += 1;
                counts
            },
        );
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

    for formula_entity in native.entity_records.iter().filter(|entity| {
        graph_scope.is_none_or(|scope| scope.contains(entity.object_graph.as_str()))
    }) {
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
        let mut expression_bindings = BTreeMap::new();
        let mut all_inputs_complete = true;
        for dependency in &formula.parameter_dependencies {
            let Some(input) = signature.inputs.iter().find(|input| {
                dependency
                    .symbol
                    .strip_prefix(&input.parameter)
                    .is_some_and(|suffix| {
                        suffix.is_empty() || suffix.starts_with(char::is_whitespace)
                    })
            }) else {
                all_inputs_complete = false;
                continue;
            };
            let [parameter] = dependency.candidates.as_slice() else {
                all_inputs_complete = false;
                continue;
            };
            let Some(entity) = entities.get(parameter.as_str()) else {
                all_inputs_complete = false;
                continue;
            };
            let Some(parameter) = &entity.parameter_value else {
                all_inputs_complete = false;
                continue;
            };
            let Some(evaluation) =
                typed_parameter_evaluation(&input.input_type, &parameter.evaluation)
            else {
                all_inputs_complete = false;
                continue;
            };
            let parameter_type = canonical_parameter_type(&input.input_type)
                .expect("typed evaluation requires a supported type");
            used_inputs.insert(input.parameter.as_str());
            let id = neutral_parameter_id(&entity.id);
            if dependencies.contains(&id) {
                continue;
            }
            dependencies.push(id.clone());
            let (expression, value) = match evaluation {
                TypedParameterEvaluation::Unset => {
                    all_inputs_complete = false;
                    (String::new(), None)
                }
                TypedParameterEvaluation::Value(value) => {
                    expression_bindings.insert(
                        input.parameter.as_str(),
                        EvaluatedFormulaValue::from_parameter_value(&value),
                    );
                    (parameter_expression(&value), Some(value))
                }
            };
            transferred.push(FormulaParameterCandidate {
                parameter: DesignParameter {
                    id,
                    owner: None,
                    ordinal: 0,
                    name: parameter.name.value.clone(),
                    expression,
                    display: None,
                    value,
                    dependencies: Vec::new(),
                    properties: parameter_properties(parameter_type),
                    pmi: None,
                    native_ref: Some(entity.id.clone()),
                },
                parameter_type,
                formula_output: false,
                input_fallback: None,
                source_order: entity.byte_offset,
            });
        }
        let formula_complete = all_inputs_complete
            && used_inputs.len() == signature.inputs.len()
            && dependencies.len() == signature.inputs.len();
        let evaluated_expression = formula_complete
            .then(|| {
                evaluate_formula_expression(&expression.expression.value, &expression_bindings)
            })
            .flatten()
            .filter(|value| value.satisfies_source_type(&signature.result_type));
        let input_parameters = transferred
            .iter()
            .map(|candidate| (candidate.parameter.clone(), candidate.parameter_type))
            .collect::<Vec<_>>();
        if let Some(output) = formula
            .parameter
            .as_deref()
            .filter(|_| evaluated_expression.is_some())
            .and_then(|id| entities.get(id))
        {
            if let Some(output_value) = &output.parameter_value {
                let output_id = neutral_parameter_id(&output.id);
                if !dependencies.contains(&output_id) {
                    if let Some(value) =
                        typed_parameter_evaluation(&signature.result_type, &output_value.evaluation)
                    {
                        let parameter_type = canonical_parameter_type(&signature.result_type)
                            .expect("typed evaluation requires a supported type");
                        if evaluated_expression
                            .as_ref()
                            .is_some_and(|evaluated| evaluated.agrees_with(&value))
                        {
                            programs.push(FormulaProgramCandidate {
                                formula_entity: formula_entity.id.clone(),
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
                                    properties: parameter_properties(parameter_type),
                                    pmi: None,
                                    native_ref: Some(output.id.clone()),
                                },
                                parameter_type,
                                formula_output: true,
                                input_fallback: None,
                                source_order: output.byte_offset,
                            });
                        }
                    }
                }
            }
        }

        for mut candidate in transferred {
            match candidates.get(&candidate.parameter.id) {
                Some(existing) if !formula_parameter_candidates_agree(existing, &candidate) => {
                    match (existing.formula_output, candidate.formula_output) {
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
                Some(existing) if !existing.formula_output && candidate.formula_output => {
                    candidate.input_fallback =
                        Some((existing.parameter.clone(), existing.parameter_type));
                    candidates.insert(candidate.parameter.id.clone(), candidate);
                }
                Some(existing) if existing.formula_output && !candidate.formula_output => {
                    candidates
                        .get_mut(&candidate.parameter.id)
                        .expect("candidate exists")
                        .input_fallback
                        .get_or_insert((candidate.parameter, candidate.parameter_type));
                }
                Some(_) => {}
                None => {
                    candidates.insert(candidate.parameter.id.clone(), candidate);
                }
            }
        }
    }

    for id in &conflicting_inputs {
        match candidates.get_mut(id) {
            Some(candidate) if candidate.formula_output => {
                candidate.input_fallback = None;
            }
            Some(_) => {
                candidates.remove(id);
            }
            None => {}
        }
    }
    candidates.retain(|id, candidate| {
        match (candidate.formula_output, formula_definition_counts.get(id)) {
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
        if !candidate.formula_output {
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
    let mut consumed_entity_records = candidates
        .values()
        .filter_map(|candidate| candidate.parameter.native_ref.clone())
        .collect::<HashSet<_>>();
    for program in programs {
        if candidates
            .get(&program.output)
            .is_some_and(|candidate| candidate.formula_output)
            && program
                .inputs
                .iter()
                .all(|input| candidates.contains_key(input))
        {
            consumed_entity_records.insert(program.formula_entity);
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
    for candidate in &parameters {
        annotations
            .exactness
            .entry(candidate.parameter.id.0.clone())
            .or_default()
            .fields
            .insert("properties".to_string(), Exactness::Derived);
        if !candidate.formula_output && candidate.parameter.dependencies.is_empty() {
            annotations
                .exactness
                .entry(candidate.parameter.id.0.clone())
                .or_default()
                .fields
                .insert("expression".to_string(), Exactness::Derived);
        }
    }
    let transferred = parameters.len();
    ir.model
        .parameters
        .extend(parameters.into_iter().map(|candidate| candidate.parameter));
    FormulaTransfer {
        formula_parameter_count: transferred.saturating_sub(legacy_transfer.parameters),
        legacy_parameter_count: legacy_transfer.parameters,
        legacy_selector_parameter_count: legacy_transfer.selector_parameters,
        legacy_formula_count: legacy_transfer.formulas,
        consumed_object_records,
    }
}

#[derive(Default)]
pub(crate) struct FormulaTransfer {
    pub(crate) formula_parameter_count: usize,
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
                        properties: parameter_properties(parameter_type),
                        pmi: None,
                        native_ref: Some(run.id.clone()),
                    },
                    parameter_type,
                    formula_output: false,
                    input_fallback: None,
                    source_order: scalar.byte_offset,
                },
            );
            parameters_by_entity
                .entry(scalar.entity_id)
                .or_default()
                .push(id);
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
            candidates.insert(
                id.clone(),
                FormulaParameterCandidate {
                    parameter: DesignParameter {
                        id: id.clone(),
                        owner: None,
                        ordinal: 0,
                        name: name.clone(),
                        expression: String::new(),
                        display: None,
                        value: Some(ParameterValue::String(string.value.clone())),
                        dependencies: Vec::new(),
                        properties: parameter_properties("String"),
                        pmi: None,
                        native_ref: Some(run.id.clone()),
                    },
                    parameter_type: "String",
                    formula_output: false,
                    input_fallback: None,
                    source_order: string.byte_offset,
                },
            );
            parameters_by_entity
                .entry(string.entity_id)
                .or_default()
                .push(id);
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
                        properties: parameter_properties("Integer"),
                        pmi: None,
                        native_ref: Some(run.id.clone()),
                    },
                    parameter_type: "Integer",
                    formula_output: false,
                    input_fallback: None,
                    source_order: integer.byte_offset,
                },
            );
            parameters_by_entity
                .entry(integer.entity_id)
                .or_default()
                .push(id);
            transfer.parameters += 1;
            transfer.selector_parameters += usize::from(selected);
        }
        let mut relations_by_parameter =
            HashMap::<u32, Vec<&crate::native::CatiaLegacyRelation>>::new();
        for relation in &run.relations {
            if relation.inputs.is_empty() && relation.output.is_none() {
                if let Some(parameter) = relation.parameter_entity_id {
                    relations_by_parameter
                        .entry(parameter)
                        .or_default()
                        .push(relation);
                }
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
            let Some(candidate) = candidates.get_mut(parameter) else {
                continue;
            };
            if canonical_parameter_type(&relation.result_type) != Some(candidate.parameter_type) {
                continue;
            }
            let bindings = BTreeMap::new();
            let Some(evaluated) = evaluate_formula_expression(&relation.expression, &bindings)
                .filter(|value| value.satisfies_source_type(&relation.result_type))
            else {
                continue;
            };
            if let Some(stored) = candidate.parameter.value.clone() {
                if !evaluated.agrees_with(&TypedParameterEvaluation::Value(stored)) {
                    continue;
                }
            }
            candidate
                .parameter
                .expression
                .clone_from(&relation.expression);
            candidate.formula_output = true;
            transfer.formulas += 1;
        }
    }
    transfer
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

#[cfg(test)]
mod tests {
    use super::{outer_container_in_scope, LegacyModelingScope};
    use crate::native::CatiaOuterContainerBinding;

    fn binding(stream_name: &str) -> CatiaOuterContainerBinding {
        CatiaOuterContainerBinding {
            data_offset: 10,
            ordinal: 2,
            class_name: "CATPrtCont".to_string(),
            base_class: "CATProdCont".to_string(),
            stream_name: stream_name.to_string(),
        }
    }

    #[test]
    fn legacy_parameter_scope_requires_the_exact_modeling_container() {
        let part = binding("part");
        let other_part = binding("other-part");

        assert!(outer_container_in_scope(
            Some(&part),
            LegacyModelingScope::Container(&part)
        ));
        assert!(!outer_container_in_scope(
            Some(&other_part),
            LegacyModelingScope::Container(&part)
        ));
        assert!(!outer_container_in_scope(
            None,
            LegacyModelingScope::Container(&part)
        ));
        assert!(!outer_container_in_scope(
            Some(&part),
            LegacyModelingScope::Unresolved
        ));
    }

    #[test]
    fn legacy_parameter_scope_admits_unbound_fragment_runs() {
        assert!(outer_container_in_scope(
            None,
            LegacyModelingScope::Unbounded
        ));
    }
}

struct FormulaParameterCandidate {
    parameter: DesignParameter,
    parameter_type: &'static str,
    formula_output: bool,
    input_fallback: Option<(DesignParameter, &'static str)>,
    source_order: u64,
}

struct FormulaProgramCandidate {
    formula_entity: String,
    expression_entity: String,
    output: ParameterId,
    inputs: Vec<ParameterId>,
    input_parameters: Vec<(DesignParameter, &'static str)>,
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

fn formula_parameter_candidate_accepts_input(
    candidate: &FormulaParameterCandidate,
    input: &(DesignParameter, &'static str),
) -> bool {
    if candidate.parameter_type != input.1 {
        return false;
    }
    if candidate.formula_output {
        formula_parameter_matches_input(&candidate.parameter, &input.0)
    } else {
        candidate.parameter == input.0
    }
}

fn demote_formula_output(candidate: &mut FormulaParameterCandidate) -> bool {
    let Some((input, parameter_type)) = candidate.input_fallback.take() else {
        return false;
    };
    candidate.parameter = input;
    candidate.parameter_type = parameter_type;
    candidate.formula_output = false;
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
        ParameterValue::Boolean(_) | ParameterValue::String(_) => unreachable!(),
    }
}

fn parameter_properties(parameter_type: &'static str) -> BTreeMap<String, String> {
    BTreeMap::from([("value_type".to_string(), parameter_type.to_string())])
}

#[derive(Clone, Copy, PartialEq, Eq)]
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

#[derive(Clone, Copy)]
struct EvaluatedFormulaScalar {
    value: f64,
    dimension: FormulaDimension,
}

impl EvaluatedFormulaScalar {
    fn satisfies_source_type(self, source_type: &str) -> bool {
        match source_type {
            "LENGTH" => self.dimension == FormulaDimension::LENGTH,
            "ANGLE" => self.dimension == FormulaDimension::ANGLE,
            "Real" | "R" => self.dimension == FormulaDimension::SCALAR,
            "Integer" | "I" => {
                self.dimension == FormulaDimension::SCALAR
                    && self.value.fract() == 0.0
                    && self.value >= i64::MIN as f64
                    && self.value < -(i64::MIN as f64)
            }
            _ => false,
        }
    }
}

#[derive(Clone)]
enum EvaluatedFormulaValue {
    Scalar(EvaluatedFormulaScalar),
    Boolean(bool),
    String(String),
}

impl EvaluatedFormulaValue {
    fn from_parameter_value(value: &ParameterValue) -> Self {
        match value {
            ParameterValue::Length(Length(value)) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value,
                dimension: FormulaDimension::LENGTH,
            }),
            ParameterValue::Angle(Angle(value)) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value,
                dimension: FormulaDimension::ANGLE,
            }),
            ParameterValue::Real(value) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value,
                dimension: FormulaDimension::SCALAR,
            }),
            ParameterValue::Integer(value) => Self::Scalar(EvaluatedFormulaScalar {
                value: *value as f64,
                dimension: FormulaDimension::SCALAR,
            }),
            ParameterValue::Boolean(value) => Self::Boolean(*value),
            ParameterValue::String(value) => Self::String(value.clone()),
        }
    }

    fn scalar(self) -> Option<EvaluatedFormulaScalar> {
        match self {
            Self::Scalar(value) => Some(value),
            Self::Boolean(_) | Self::String(_) => None,
        }
    }

    fn boolean(self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(value),
            Self::Scalar(_) | Self::String(_) => None,
        }
    }

    #[cfg(test)]
    fn string(self) -> Option<String> {
        match self {
            Self::String(value) => Some(value),
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
                (Self::Boolean(left), ParameterValue::Boolean(right)) => left == right,
                (Self::String(left), ParameterValue::String(right)) => left == right,
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
}

const MAX_FORMULA_EXPRESSION_DEPTH: usize = 128;
const MAX_FORMULA_FUNCTION_ARGUMENTS: usize = 128;

fn finite_scalar(value: f64) -> Option<EvaluatedFormulaScalar> {
    value.is_finite().then_some(EvaluatedFormulaScalar {
        value,
        dimension: FormulaDimension::SCALAR,
    })
}

fn finite_angle(value: f64) -> Option<EvaluatedFormulaScalar> {
    value.is_finite().then_some(EvaluatedFormulaScalar {
        value,
        dimension: FormulaDimension::ANGLE,
    })
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
        self.evaluate = evaluate && predicate;
        let when_true = self.conditional(Self::nested_depth(depth)?)?;
        self.skip_whitespace();
        (self.peek()? == b';').then_some(())?;
        self.at += 1;
        self.evaluate = evaluate && !predicate;
        let when_false = self.conditional(Self::nested_depth(depth)?)?;
        self.evaluate = evaluate;
        Self::same_value_type(&when_true, &when_false)?;
        Some(if !evaluate || predicate {
            when_true
        } else {
            when_false
        })
    }

    fn same_value_type(left: &EvaluatedFormulaValue, right: &EvaluatedFormulaValue) -> Option<()> {
        match (left, right) {
            (EvaluatedFormulaValue::Scalar(_), EvaluatedFormulaValue::Scalar(_)) => Some(()),
            (EvaluatedFormulaValue::Boolean(_), EvaluatedFormulaValue::Boolean(_))
            | (EvaluatedFormulaValue::String(_), EvaluatedFormulaValue::String(_)) => Some(()),
            _ => None,
        }
    }

    fn scalar_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        finite_scalar(if self.evaluate { value } else { 0.0 })
    }

    fn angle_result(&self, value: f64) -> Option<EvaluatedFormulaScalar> {
        finite_angle(if self.evaluate { value } else { 0.0 })
    }

    fn disjunction(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        let mut value = self.conjunction(depth)?;
        loop {
            self.skip_whitespace();
            if !self.consume_keyword("or") {
                return Some(value);
            }
            let evaluate = self.evaluate;
            let left = value.boolean()?;
            self.evaluate = evaluate && !left;
            let right = self.conjunction(depth)?;
            let right = right.boolean()?;
            self.evaluate = evaluate;
            value = EvaluatedFormulaValue::Boolean(if evaluate { left || right } else { false });
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
            let left = value.boolean()?;
            self.evaluate = evaluate && left;
            let right = self.comparison(depth)?;
            let right = right.boolean()?;
            self.evaluate = evaluate;
            value = EvaluatedFormulaValue::Boolean(if evaluate { left && right } else { false });
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
        let value = match (operator, left, right) {
            ("==", EvaluatedFormulaValue::Boolean(left), EvaluatedFormulaValue::Boolean(right)) => {
                left == right
            }
            ("<>", EvaluatedFormulaValue::Boolean(left), EvaluatedFormulaValue::Boolean(right)) => {
                left != right
            }
            ("==", EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) => {
                left == right
            }
            ("<>", EvaluatedFormulaValue::String(left), EvaluatedFormulaValue::String(right)) => {
                left != right
            }
            (
                operator,
                EvaluatedFormulaValue::Scalar(left),
                EvaluatedFormulaValue::Scalar(right),
            ) if left.dimension == right.dimension => match operator {
                "==" => left.value == right.value,
                "<>" => left.value != right.value,
                ">=" => left.value >= right.value,
                "<=" => left.value <= right.value,
                ">" => left.value > right.value,
                "<" => left.value < right.value,
                _ => unreachable!(),
            },
            _ => return None,
        };
        Some(EvaluatedFormulaValue::Boolean(value))
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
                    let mut joined = String::with_capacity(left.len().checked_add(right.len())?);
                    joined.push_str(left);
                    joined.push_str(right);
                    value = EvaluatedFormulaValue::String(joined);
                    continue;
                }
            }
            let mut left = value.scalar()?;
            let right = right.scalar()?;
            if left.dimension != right.dimension {
                return None;
            }
            left.value = if operator == b'+' {
                left.value + right.value
            } else {
                left.value - right.value
            };
            if self.evaluate && !left.value.is_finite() {
                return None;
            }
            if !left.value.is_finite() {
                left.value = 0.0;
            }
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
            let result = if operator == b'*' {
                EvaluatedFormulaScalar {
                    value: left.value * right.value,
                    dimension: left.dimension.product(right.dimension)?,
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
            });
        }
    }

    fn unary(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        self.skip_whitespace();
        if self.consume_keyword("not") {
            let value = self.unary(Self::nested_depth(depth)?)?.boolean()?;
            return Some(EvaluatedFormulaValue::Boolean(!value));
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
            if exponent.value.fract() != 0.0
                || exponent.value < f64::from(i32::MIN)
                || exponent.value > f64::from(i32::MAX)
            {
                return None;
            }
            base.dimension.power(exponent.value as i32)?
        };
        let value = base.value.powf(exponent.value);
        (value.is_finite() || !self.evaluate).then_some(EvaluatedFormulaValue::Scalar(
            EvaluatedFormulaScalar {
                value: if value.is_finite() { value } else { 0.0 },
                dimension,
            },
        ))
    }

    fn primary(&mut self, depth: usize) -> Option<EvaluatedFormulaValue> {
        self.skip_whitespace();
        if self.peek()? == b'"' {
            return self.string_literal().map(EvaluatedFormulaValue::String);
        }
        if self.consume_keyword("true") {
            return Some(EvaluatedFormulaValue::Boolean(true));
        }
        if self.consume_keyword("false") {
            return Some(EvaluatedFormulaValue::Boolean(false));
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
                    let length = u32::try_from(value.chars().count()).ok()?;
                    EvaluatedFormulaValue::Scalar(finite_scalar(f64::from(length))?)
                }
                (
                    "Search",
                    EvaluatedFormulaValue::String(value),
                    [EvaluatedFormulaValue::String(needle)],
                ) => {
                    let index = if let Some(byte_offset) = value.find(needle.as_str()) {
                        i64::try_from(value[..byte_offset].chars().count()).ok()?
                    } else {
                        -1
                    };
                    EvaluatedFormulaValue::Scalar(finite_scalar(index as f64)?)
                }
                _ => return None,
            };
        }
    }

    fn string_literal(&mut self) -> Option<String> {
        (self.peek()? == b'"').then_some(())?;
        self.at += 1;
        let start = self.at;
        while let Some(byte) = self.peek() {
            if byte == b'"' {
                let value = self.source.get(start..self.at)?.to_string();
                self.at += 1;
                return Some(value);
            }
            if byte.is_ascii_control() || byte == b'\\' {
                return None;
            }
            self.at += 1;
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
            if self.evaluate && from.is_empty() {
                return None;
            }
            return Some(EvaluatedFormulaValue::String(if self.evaluate {
                source.replace(from, to)
            } else {
                String::new()
            }));
        }

        if function == "ToString" {
            let [EvaluatedFormulaValue::Scalar(value)] = arguments.as_slice() else {
                return None;
            };
            if self.evaluate && !value.satisfies_source_type("Integer") {
                return None;
            }
            return Some(EvaluatedFormulaValue::String(if self.evaluate {
                format!("{:.0}", value.value)
            } else {
                String::new()
            }));
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
            }
            return Some(EvaluatedFormulaValue::Scalar(result));
        }

        if function == "LinearInterpolation" {
            let [start, end, fraction] = arguments.as_slice() else {
                return None;
            };
            if [start, end, fraction]
                .into_iter()
                .any(|argument| argument.dimension != FormulaDimension::SCALAR)
            {
                return None;
            }
            return self
                .scalar_result(start.value + (end.value - start.value) * fraction.value)
                .map(EvaluatedFormulaValue::Scalar);
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
            ("asin", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.angle_result(argument.value.asin())
            }
            ("acos", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.angle_result(argument.value.acos())
            }
            ("atan", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.angle_result(argument.value.atan())
            }
            ("log", argument, None)
                if argument.dimension == FormulaDimension::SCALAR
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
                    && (!self.evaluate || argument.value > 0.0) =>
            {
                self.scalar_result(if self.evaluate {
                    argument.value.ln()
                } else {
                    0.0
                })
            }
            ("exp", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.exp())
            }
            ("sinh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.sinh())
            }
            ("cosh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.cosh())
            }
            ("tanh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.tanh())
            }
            ("asinh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.asinh())
            }
            ("acosh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.acosh())
            }
            ("atanh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.atanh())
            }
            ("ceil", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.ceil())
            }
            ("floor", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.floor())
            }
            ("int", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.trunc())
            }
            ("round", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                self.scalar_result(argument.value.round_ties_even())
            }
            ("mod", dividend, Some(divisor))
                if dividend.dimension == FormulaDimension::SCALAR
                    && divisor.dimension == FormulaDimension::SCALAR
                    && (!self.evaluate
                        || (divisor.satisfies_source_type("Integer") && divisor.value != 0.0)) =>
            {
                self.scalar_result(if self.evaluate {
                    dividend.value.trunc() % divisor.value
                } else {
                    0.0
                })
            }
            ("abs", argument, None) => Some(EvaluatedFormulaScalar {
                value: argument.value.abs(),
                dimension: argument.dimension,
            }),
            ("sqrt", argument, None) if !self.evaluate || argument.value >= 0.0 => {
                Some(EvaluatedFormulaScalar {
                    value: if self.evaluate {
                        argument.value.sqrt()
                    } else {
                        0.0
                    },
                    dimension: argument.dimension.square_root()?,
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
        let dimension = if let Some((unit, millimetres)) = [
            ("micron", 0.001),
            ("mile", 1_609_344.0),
            ("yard", 914.4),
            ("mm", 1.0),
            ("cm", 10.0),
            ("km", 1_000_000.0),
            ("ft", 304.8),
            ("in", 25.4),
            ("m", 1_000.0),
        ]
        .into_iter()
        .find(|(unit, _)| self.remaining().starts_with(unit))
        {
            self.at += unit.len();
            value *= millimetres;
            FormulaDimension::LENGTH
        } else if self.remaining().starts_with("rad") {
            self.at += 3;
            FormulaDimension::ANGLE
        } else if self.remaining().starts_with("grad") {
            self.at += 4;
            value *= std::f64::consts::PI / 200.0;
            FormulaDimension::ANGLE
        } else if self.remaining().starts_with("deg") {
            self.at += 3;
            value = value.to_radians();
            FormulaDimension::ANGLE
        } else {
            self.at = unit_boundary;
            FormulaDimension::SCALAR
        };
        value
            .is_finite()
            .then_some(EvaluatedFormulaScalar { value, dimension })
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
    FormulaExpressionParser {
        source,
        at: 0,
        bindings,
        evaluate: true,
    }
    .parse()
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

fn canonical_parameter_type(source_type: &str) -> Option<&'static str> {
    match source_type {
        "LENGTH" => Some("LENGTH"),
        "ANGLE" => Some("ANGLE"),
        "Real" | "R" => Some("Real"),
        "Integer" | "I" => Some("Integer"),
        "Boolean" => Some("Boolean"),
        "String" => Some("String"),
        _ => None,
    }
}

fn neutral_parameter_id(native_id: &str) -> ParameterId {
    ParameterId(format!("{native_id}:parameter"))
}

#[cfg(test)]
mod parser_tests {
    use super::*;

    fn unset_candidate(parameter_type: &'static str) -> FormulaParameterCandidate {
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
            formula_output: false,
            input_fallback: None,
            source_order: 1,
        }
    }

    #[test]
    fn unset_parameter_candidates_require_one_canonical_type() {
        assert!(formula_parameter_candidates_agree(
            &unset_candidate("Real"),
            &unset_candidate("Real")
        ));
        assert!(!formula_parameter_candidates_agree(
            &unset_candidate("LENGTH"),
            &unset_candidate("Real")
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
                .is_some_and(|value| value)
        );
        assert_eq!(
            evaluate_formula_expression("false or true and false", &bindings)
                .and_then(EvaluatedFormulaValue::boolean),
            Some(false)
        );
        assert_eq!(
            evaluate_formula_expression("1mm == 1cm", &bindings)
                .and_then(EvaluatedFormulaValue::boolean),
            Some(false)
        );
        assert_eq!(
            evaluate_formula_expression("true <> false", &bindings)
                .and_then(EvaluatedFormulaValue::boolean),
            Some(true)
        );
        assert_eq!(
            evaluate_formula_expression("not false and false or not (1 > 2)", &bindings)
                .and_then(EvaluatedFormulaValue::boolean),
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
                    assert_eq!(value, expected, "{expression}");
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
        ] {
            assert!(
                evaluate_formula_expression(expression, &bindings).is_none(),
                "{expression}"
            );
        }
        for (expression, expected_dimension) in [
            ("true ? 1mm ; 1rad", FormulaDimension::LENGTH),
            ("false ? 1mm ; 1rad", FormulaDimension::ANGLE),
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
                EvaluatedFormulaValue::String("Cilas Evans".to_string()),
            ),
            ("#2_", EvaluatedFormulaValue::String("Evans".to_string())),
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
                .is_some_and(|value| value)
        );
    }

    #[test]
    fn formula_string_operations_reject_untyped_or_incomplete_forms() {
        let bindings = BTreeMap::new();
        for expression in [
            "\"unterminated",
            "\"unsupported\\\\escape\"",
            "\"text\" - \"text\"",
            "\"text\" + 1",
            "ToString(1.5)",
            "ReplaceSubText(\"text\",\"\",\"x\")",
            "\"text\".Search(1)",
            "\"text\".Length(1)",
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
