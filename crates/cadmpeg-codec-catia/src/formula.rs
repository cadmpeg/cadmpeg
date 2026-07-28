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
        let mut expression_bindings = BTreeMap::new();
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
            expression_bindings.insert(
                input.parameter.as_str(),
                EvaluatedFormulaScalar::from_parameter_value(&value),
            );
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
        let evaluated_expression = formula_complete
            .then(|| {
                evaluate_formula_expression(&expression.expression.value, &expression_bindings)
            })
            .flatten()
            .filter(|value| value.satisfies_source_type(&signature.result_type));
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
                        if evaluated_expression
                            .as_ref()
                            .is_some_and(|evaluated| evaluated.agrees_with(&value))
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
pub(crate) struct FormulaTransfer {
    pub(crate) parameter_count: usize,
    pub(crate) consumed_object_records: HashSet<String>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormulaDimension {
    Scalar,
    Length,
    Angle,
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
                dimension: FormulaDimension::Length,
            },
            ParameterValue::Angle(Angle(value)) => Self {
                value: *value,
                dimension: FormulaDimension::Angle,
            },
            ParameterValue::Real(value) => Self {
                value: *value,
                dimension: FormulaDimension::Scalar,
            },
            ParameterValue::Integer(value) => Self {
                value: *value as f64,
                dimension: FormulaDimension::Scalar,
            },
            ParameterValue::Boolean(_) | ParameterValue::String(_) => unreachable!(),
        }
    }

    fn satisfies_source_type(self, source_type: &str) -> bool {
        match source_type {
            "LENGTH" => self.dimension == FormulaDimension::Length,
            "ANGLE" => self.dimension == FormulaDimension::Angle,
            "Real" | "R" => self.dimension == FormulaDimension::Scalar,
            "Integer" | "I" => {
                self.dimension == FormulaDimension::Scalar
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

impl FormulaExpressionParser<'_, '_> {
    fn parse(mut self) -> Option<EvaluatedFormulaScalar> {
        let value = self.sum()?;
        self.skip_whitespace();
        (self.at == self.source.len() && value.value.is_finite()).then_some(value)
    }

    fn sum(&mut self) -> Option<EvaluatedFormulaScalar> {
        let mut value = self.product()?;
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Some(value);
            };
            if !matches!(operator, b'+' | b'-') {
                return Some(value);
            }
            self.at += 1;
            let right = self.product()?;
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

    fn product(&mut self) -> Option<EvaluatedFormulaScalar> {
        let mut value = self.unary()?;
        loop {
            self.skip_whitespace();
            let Some(operator) = self.peek() else {
                return Some(value);
            };
            if !matches!(operator, b'*' | b'/') {
                return Some(value);
            }
            self.at += 1;
            let right = self.unary()?;
            value = if operator == b'*' {
                match (value.dimension, right.dimension) {
                    (FormulaDimension::Scalar, dimension) => EvaluatedFormulaScalar {
                        value: value.value * right.value,
                        dimension,
                    },
                    (dimension, FormulaDimension::Scalar) => EvaluatedFormulaScalar {
                        value: value.value * right.value,
                        dimension,
                    },
                    _ => return None,
                }
            } else {
                if right.value == 0.0 {
                    return None;
                }
                match (value.dimension, right.dimension) {
                    (dimension, FormulaDimension::Scalar) => EvaluatedFormulaScalar {
                        value: value.value / right.value,
                        dimension,
                    },
                    (left_dimension, right_dimension) if left_dimension == right_dimension => {
                        EvaluatedFormulaScalar {
                            value: value.value / right.value,
                            dimension: FormulaDimension::Scalar,
                        }
                    }
                    _ => return None,
                }
            };
            if !value.value.is_finite() {
                return None;
            }
        }
    }

    fn unary(&mut self) -> Option<EvaluatedFormulaScalar> {
        self.skip_whitespace();
        match self.peek()? {
            b'+' => {
                self.at += 1;
                self.unary()
            }
            b'-' => {
                self.at += 1;
                let mut value = self.unary()?;
                value.value = -value.value;
                Some(value)
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Option<EvaluatedFormulaScalar> {
        self.skip_whitespace();
        if self.peek()? == b'(' {
            self.at += 1;
            let value = self.sum()?;
            self.skip_whitespace();
            (self.peek()? == b')').then_some(())?;
            self.at += 1;
            return Some(value);
        }
        if self.peek()? == b'#' {
            return self.symbol();
        }
        if self.peek()?.is_ascii_alphabetic() {
            return self.function_call();
        }
        self.literal()
    }

    fn function_call(&mut self) -> Option<EvaluatedFormulaScalar> {
        let function_start = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_alphabetic()) {
            self.at += 1;
        }
        let function = &self.source[function_start..self.at];
        self.skip_whitespace();
        (self.peek()? == b'(').then_some(())?;
        self.at += 1;
        let argument = self.sum()?;
        self.skip_whitespace();
        (self.peek()? == b')').then_some(())?;
        self.at += 1;
        (argument.dimension == FormulaDimension::Angle).then_some(())?;
        let value = match function {
            "sin" => argument.value.sin(),
            "cos" => argument.value.cos(),
            _ => return None,
        };
        value.is_finite().then_some(EvaluatedFormulaScalar {
            value,
            dimension: FormulaDimension::Scalar,
        })
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
        (self.peek()? == b'/').then_some(())?;
        self.at += 1;
        let ordinal = self.at;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.at += 1;
        }
        (self.at > ordinal).then_some(())?;
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
        let dimension = if self.remaining().starts_with("mm") {
            self.at += 2;
            FormulaDimension::Length
        } else if self.remaining().starts_with("rad") {
            self.at += 3;
            FormulaDimension::Angle
        } else if self.remaining().starts_with("deg") {
            self.at += 3;
            value = value.to_radians();
            FormulaDimension::Angle
        } else {
            FormulaDimension::Scalar
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
