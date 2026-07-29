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
        .filter_map(|entity| entity.formula_relation.as_ref()?.parameter.as_deref())
        .fold(
            HashMap::<ParameterId, usize>::new(),
            |mut counts, parameter| {
                *counts.entry(neutral_parameter_id(parameter)).or_default() += 1;
                counts
            },
        );
    let legacy_transfer = collect_legacy_parameters(native, &mut candidates);

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
                        EvaluatedFormulaScalar::from_parameter_value(&value),
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

fn collect_legacy_parameters(
    native: &CatiaNative,
    candidates: &mut BTreeMap<ParameterId, FormulaParameterCandidate>,
) -> LegacyParameterTransfer {
    let mut transfer = LegacyParameterTransfer::default();
    for run in &native.legacy_entity_runs {
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
    fn from_parameter_value(value: &ParameterValue) -> Self {
        match value {
            ParameterValue::Length(Length(value)) => Self {
                value: *value,
                dimension: FormulaDimension::LENGTH,
            },
            ParameterValue::Angle(Angle(value)) => Self {
                value: *value,
                dimension: FormulaDimension::ANGLE,
            },
            ParameterValue::Real(value) => Self {
                value: *value,
                dimension: FormulaDimension::SCALAR,
            },
            ParameterValue::Integer(value) => Self {
                value: *value as f64,
                dimension: FormulaDimension::SCALAR,
            },
            ParameterValue::Boolean(_) | ParameterValue::String(_) => unreachable!(),
        }
    }

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

    fn agrees_with(self, evaluation: &TypedParameterEvaluation) -> bool {
        match evaluation {
            TypedParameterEvaluation::Unset => true,
            TypedParameterEvaluation::Value(value) => {
                let stored = Self::from_parameter_value(value);
                self.dimension == stored.dimension && self.value == stored.value
            }
        }
    }
}

struct FormulaExpressionParser<'a, 'b> {
    source: &'a str,
    at: usize,
    bindings: &'b BTreeMap<&'a str, EvaluatedFormulaScalar>,
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
    fn parse(mut self) -> Option<EvaluatedFormulaScalar> {
        let value = self.sum(0)?;
        self.skip_whitespace();
        (self.at == self.source.len() && value.value.is_finite()).then_some(value)
    }

    fn sum(&mut self, depth: usize) -> Option<EvaluatedFormulaScalar> {
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
            if value.dimension != right.dimension {
                return None;
            }
            value.value = if operator == b'+' {
                value.value + right.value
            } else {
                value.value - right.value
            };
            if !value.value.is_finite() {
                return None;
            }
        }
    }

    fn product(&mut self, depth: usize) -> Option<EvaluatedFormulaScalar> {
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
            let right = self.unary(depth)?;
            value = if operator == b'*' {
                EvaluatedFormulaScalar {
                    value: value.value * right.value,
                    dimension: value.dimension.product(right.dimension)?,
                }
            } else {
                if right.value == 0.0 {
                    return None;
                }
                EvaluatedFormulaScalar {
                    value: value.value / right.value,
                    dimension: value.dimension.quotient(right.dimension)?,
                }
            };
            if !value.value.is_finite() {
                return None;
            }
        }
    }

    fn unary(&mut self, depth: usize) -> Option<EvaluatedFormulaScalar> {
        self.skip_whitespace();
        match self.peek()? {
            b'+' => {
                self.at += 1;
                self.unary(Self::nested_depth(depth)?)
            }
            b'-' => {
                self.at += 1;
                let mut value = self.unary(Self::nested_depth(depth)?)?;
                value.value = -value.value;
                Some(value)
            }
            _ => self.power(depth),
        }
    }

    fn power(&mut self, depth: usize) -> Option<EvaluatedFormulaScalar> {
        let base = self.primary(depth)?;
        self.skip_whitespace();
        if !self.remaining().starts_with("**") {
            return Some(base);
        }
        self.at += 2;
        let exponent = self.unary(Self::nested_depth(depth)?)?;
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
        value
            .is_finite()
            .then_some(EvaluatedFormulaScalar { value, dimension })
    }

    fn primary(&mut self, depth: usize) -> Option<EvaluatedFormulaScalar> {
        self.skip_whitespace();
        if self.peek()? == b'(' {
            self.at += 1;
            let value = self.sum(Self::nested_depth(depth)?)?;
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
            return Some(EvaluatedFormulaScalar {
                value: std::f64::consts::PI,
                dimension: FormulaDimension::SCALAR,
            });
        }
        if self.remaining().starts_with('E')
            && self
                .source
                .as_bytes()
                .get(self.at + 1)
                .is_none_or(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        {
            self.at += 1;
            return finite_scalar(std::f64::consts::E);
        }
        if self.peek()?.is_ascii_alphabetic() {
            return self.function_call(depth);
        }
        self.literal()
    }

    fn function_call(&mut self, depth: usize) -> Option<EvaluatedFormulaScalar> {
        let function_start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let function = &self.source[function_start..self.at];
        self.skip_whitespace();
        (self.peek()? == b'(').then_some(())?;
        self.at += 1;
        let argument_depth = Self::nested_depth(depth)?;
        let mut arguments = Vec::with_capacity(2);
        loop {
            arguments.push(self.sum(argument_depth)?);
            self.skip_whitespace();
            if self.peek()? == b')' {
                self.at += 1;
                break;
            }
            (self.peek()? == b',' && arguments.len() < MAX_FORMULA_FUNCTION_ARGUMENTS)
                .then_some(())?;
            self.at += 1;
        }

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
            return Some(result);
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
            return finite_scalar(start.value + (end.value - start.value) * fraction.value);
        }

        let (first, second) = match arguments.as_slice() {
            [first] => (*first, None),
            [first, second] => (*first, Some(*second)),
            _ => return None,
        };
        match (function, first, second) {
            ("sin", argument, None)
                if matches!(
                    argument.dimension,
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                finite_scalar(argument.value.sin())
            }
            ("cos", argument, None)
                if matches!(
                    argument.dimension,
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                finite_scalar(argument.value.cos())
            }
            ("tan", argument, None)
                if matches!(
                    argument.dimension,
                    FormulaDimension::ANGLE | FormulaDimension::SCALAR
                ) =>
            {
                finite_scalar(argument.value.tan())
            }
            ("asin", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_angle(argument.value.asin())
            }
            ("acos", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_angle(argument.value.acos())
            }
            ("atan", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_angle(argument.value.atan())
            }
            ("log", argument, None)
                if argument.dimension == FormulaDimension::SCALAR && argument.value > 0.0 =>
            {
                finite_scalar(argument.value.log10())
            }
            ("ln", argument, None)
                if argument.dimension == FormulaDimension::SCALAR && argument.value > 0.0 =>
            {
                finite_scalar(argument.value.ln())
            }
            ("exp", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.exp())
            }
            ("sinh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.sinh())
            }
            ("cosh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.cosh())
            }
            ("tanh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.tanh())
            }
            ("asinh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.asinh())
            }
            ("acosh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.acosh())
            }
            ("atanh", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.atanh())
            }
            ("ceil", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.ceil())
            }
            ("floor", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.floor())
            }
            ("int", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.trunc())
            }
            ("round", argument, None) if argument.dimension == FormulaDimension::SCALAR => {
                finite_scalar(argument.value.round_ties_even())
            }
            ("mod", dividend, Some(divisor))
                if dividend.dimension == FormulaDimension::SCALAR
                    && divisor.satisfies_source_type("Integer")
                    && divisor.value != 0.0 =>
            {
                finite_scalar(dividend.value.trunc() % divisor.value)
            }
            ("abs", argument, None) => Some(EvaluatedFormulaScalar {
                value: argument.value.abs(),
                dimension: argument.dimension,
            }),
            ("sqrt", argument, None) if argument.value >= 0.0 => Some(EvaluatedFormulaScalar {
                value: argument.value.sqrt(),
                dimension: argument.dimension.square_root()?,
            }),
            _ => None,
        }
    }

    fn symbol(&mut self) -> Option<EvaluatedFormulaScalar> {
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
        self.bindings.get(&self.source[start..name_end]).copied()
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
        let dimension = if self.remaining().starts_with("mm") {
            self.at += 2;
            FormulaDimension::LENGTH
        } else if self.remaining().starts_with("cm") {
            self.at += 2;
            value *= 10.0;
            FormulaDimension::LENGTH
        } else if self.remaining().starts_with('m') {
            self.at += 1;
            value *= 1_000.0;
            FormulaDimension::LENGTH
        } else if self.remaining().starts_with("rad") {
            self.at += 3;
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
    bindings: &BTreeMap<&'a str, EvaluatedFormulaScalar>,
) -> Option<EvaluatedFormulaScalar> {
    FormulaExpressionParser {
        source,
        at: 0,
        bindings,
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
}
