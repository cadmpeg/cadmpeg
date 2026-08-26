// SPDX-License-Identifier: Apache-2.0
//! Native Keywords parameter projection and equation evaluation.

use crate::classification::{classify, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureHistory};
use cadmpeg_ir::features::{
    Angle, DesignParameter, DimensionDisplay, FeatureDefinition, FeatureId, FeatureTreeNodeRole,
    Length, ParameterId, ParameterValue, PatternForm,
};
use std::collections::{HashMap, HashSet};

use crate::history::classify::{
    feature_family, feature_input_class, is_chamfer, is_extrude, is_fillet,
    is_history_metadata_record, is_offset_plane, EQUATION_DRIVEN_TOKEN,
};
use crate::history::literals::{
    dimension_display, format_angle_rad, format_f64_literal, format_length_mm,
    format_parameter_value, parse_angle_rad, parse_dimension_display_length,
    parse_parameter_literal, parse_positive_dimension_length_mm,
};
use crate::history::project::{
    neutral_feature_id, neutral_parameter_id, pattern_form, projected_parameter_names,
};

const EPS_PARAMETERS_EQUIVALENT_PARAMETER_VALUES_E9: f64 = 1.0e-9;

mod eval;
pub(crate) use eval::*;

pub fn project_parameters(histories: &[FeatureHistory]) -> Vec<DesignParameter> {
    let feature_names = histories
        .iter()
        .flat_map(|history| {
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
        })
        .filter(|feature| !feature.name.is_empty())
        .map(|feature| (neutral_feature_id(&feature.id), feature.name.clone()))
        .collect::<HashMap<_, _>>();
    let global_owners = histories
        .iter()
        .flat_map(|history| {
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
        })
        .filter(|feature| feature.kind.eq_ignore_ascii_case("EquationDriven"))
        .map(|feature| neutral_feature_id(&feature.id))
        .collect::<HashSet<_>>();
    let mut parameters = histories
        .iter()
        .flat_map(|history| {
            history
                .features
                .iter()
                .filter(|feature| !is_history_metadata_record(feature, &history.features))
        })
        .flat_map(|feature| {
            projected_parameter_names(feature)
                .into_iter()
                .enumerate()
                .map(move |(ordinal, name)| {
                    let expression = &feature.parameters[&name];
                    let display = dimension_display(expression);
                    let properties = feature
                        .dimension_properties
                        .get(&name)
                        .cloned()
                        .unwrap_or_default();
                    let parse_value = |value: &str| match display {
                        Some(DimensionDisplay::Diameter | DimensionDisplay::Radius) => {
                            parse_dimension_display_length(value)
                                .map(|value| ParameterValue::Length(Length(value)))
                        }
                        None => parse_native_parameter_literal(feature, &name, value),
                    };
                    let value = properties
                        .get("Value")
                        .and_then(|value| parse_value(value))
                        .or_else(|| parse_value(expression));
                    DesignParameter {
                        id: neutral_parameter_id(feature, ordinal),
                        owner: Some(neutral_feature_id(&feature.id)),
                        ordinal: ordinal as u32,
                        properties,
                        name,
                        expression: expression.clone(),
                        display,
                        value,
                        dependencies: Vec::new(),
                        native_ref: None,
                        pmi: None,
                    }
                })
        })
        .collect::<Vec<_>>();
    populate_parameter_dependencies(&mut parameters, &feature_names, &global_owners);
    order_parameters_by_dependencies(&mut parameters);
    evaluate_parameter_expressions(&mut parameters, &feature_names, &global_owners);
    for parameter in parameters
        .iter_mut()
        .filter(|parameter| parameter.value.is_none())
    {
        parameter.value = text_parameter_literal(&parameter.name, &parameter.expression);
    }
    parameters
}

pub(crate) fn text_parameter_literal(name: &str, expression: &str) -> Option<ParameterValue> {
    bare_text_parameter_literal(expression)
        .or_else(|| formatted_text_dimension_literal(name, expression))
}

pub(crate) fn bare_text_parameter_literal(expression: &str) -> Option<ParameterValue> {
    let expression = expression.trim();
    if expression.is_empty()
        || expression.chars().any(|character| {
            matches!(
                character,
                '+' | '-' | '*' | '/' | '^' | '=' | '<' | '>' | '(' | ')' | ','
            )
        })
    {
        return None;
    }
    let identifiers = expression_identifier_tokens(expression);
    if identifiers.unclosed_quote
        || identifiers
            .identifiers
            .iter()
            .any(definite_parameter_reference)
    {
        return None;
    }
    Some(ParameterValue::String(expression.to_owned()))
}

pub(crate) fn formatted_text_dimension_literal(
    name: &str,
    expression: &str,
) -> Option<ParameterValue> {
    let suffix = name.strip_prefix("TXD")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let expression = expression.trim();
    let mut rest = expression;
    let mut tags = 0usize;
    while let Some(start) = rest.find('<') {
        if rest[..start].contains('>') {
            return None;
        }
        let after_start = &rest[start + 1..];
        let end = after_start.find('>')?;
        let tag = &after_start[..end];
        if tag.trim().is_empty() || tag.contains('<') {
            return None;
        }
        tags += 1;
        rest = &after_start[end + 1..];
    }
    (tags > 0 && !rest.contains('>')).then(|| ParameterValue::String(expression.to_owned()))
}

/// Features whose parameters are document-global equation-manager values.
///
/// The equations container reaches the neutral arena either as a typed
/// feature-tree node or, when no role evidence identifies it, as a retained
/// native record carrying the operation-family token. Both forms own global
/// parameters, so both must be recognized here; otherwise the write path
/// recomputes dependency edges against an empty owner set and rejects the
/// document.
pub(crate) fn global_parameter_owners(
    features: &[cadmpeg_ir::features::Feature],
) -> HashSet<FeatureId> {
    features
        .iter()
        .filter(|feature| match &feature.definition {
            FeatureDefinition::Native { kind, .. } => {
                kind.eq_ignore_ascii_case(EQUATION_DRIVEN_TOKEN)
            }
            FeatureDefinition::TreeNode { role, .. } => *role == FeatureTreeNodeRole::Equations,
            _ => false,
        })
        .map(|feature| feature.id.clone())
        .collect()
}

/// Replace evaluable expressions with canonical literals in a temporary history projection.
///
/// Retained native histories keep their source expressions.
pub(crate) fn apply_evaluated_parameters(histories: &mut [FeatureHistory]) {
    let evaluated = project_parameters(histories)
        .into_iter()
        .filter_map(|parameter| {
            parameter
                .value
                .map(|value| ((parameter.owner, parameter.name), value))
        })
        .collect::<HashMap<_, _>>();
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let owner = neutral_feature_id(&feature.id);
        let replacements = feature
            .parameters
            .iter()
            .filter(|(name, expression)| {
                parse_native_parameter_literal(feature, name, expression).is_none()
            })
            .filter_map(|(name, _)| {
                evaluated
                    .get(&(Some(owner.clone()), name.clone()))
                    .map(|value| (name.clone(), format_parameter_value(value)))
            })
            .collect::<Vec<_>>();
        for (name, value) in replacements {
            feature.parameters.insert(name, value);
        }
    }
}

pub(crate) fn parse_native_parameter_literal(
    feature: &Feature,
    name: &str,
    expression: &str,
) -> Option<ParameterValue> {
    if native_parameter_is_length(feature, name, Some(expression)) {
        return parse_positive_dimension_length_mm(expression)
            .map(|value| ParameterValue::Length(Length(value)));
    }
    parse_parameter_literal(expression)
}

pub(crate) fn native_parameter_is_length(
    feature: &Feature,
    name: &str,
    expression: Option<&str>,
) -> bool {
    let cosmetic_thread = classify(feature) == Some(FeatureClass::CosmeticThread);
    match name {
        "D1" => {
            is_extrude(feature)
                || is_fillet(feature)
                || is_chamfer(feature)
                || feature_family(feature, "Shell")
                || feature_family(feature, "Thicken")
                || feature_family(feature, "Thickness")
                || feature_input_class(feature, NativeClassKind::Thicken)
                || matches!(
                    classify(feature),
                    Some(
                        FeatureClass::Dome
                            | FeatureClass::Rib
                            | FeatureClass::OffsetSurface
                            | FeatureClass::ExtendSurface
                            | FeatureClass::RuledSurface
                    )
                )
                || (classify(feature) == Some(FeatureClass::MoveFace)
                    && feature.properties.get("Mode").is_some_and(|mode| {
                        mode.eq_ignore_ascii_case("Offset")
                            || mode.eq_ignore_ascii_case("Translate")
                    }))
                || is_offset_plane(feature)
                || cosmetic_thread
        }
        "D2" if cosmetic_thread => true,
        "D2" if is_chamfer(feature) => {
            expression.is_none_or(|value| parse_angle_rad(value).is_none())
        }
        "D3" if matches!(
            pattern_form(feature),
            Some(PatternForm::Linear | PatternForm::CurveDriven)
        ) =>
        {
            true
        }
        _ => {
            is_extrude(feature)
                && matches!(
                    feature.properties.get("EndCondition").map(String::as_str),
                    Some("Blind" | "Symmetric")
                )
                && feature.parameters.len() == 1
                && feature.parameters.contains_key(name)
        }
    }
}

pub(crate) fn format_native_scalar(
    feature: &Feature,
    name: &str,
    value: f64,
    expression: Option<&str>,
) -> String {
    if let Some(display) = expression.and_then(dimension_display) {
        let prefix = match display {
            DimensionDisplay::Diameter => expression
                .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;"),
            DimensionDisplay::Radius => expression
                .filter(|value| value.trim().starts_with("&lt;MOD-RHO&gt;"))
                .map_or("<MOD-RHO>", |_| "&lt;MOD-RHO&gt;"),
        };
        format!("{prefix}{}", format_f64_literal(value * 1000.0))
    } else if native_parameter_is_length(feature, name, expression) {
        format_length_mm(value * 1000.0)
    } else if expression.and_then(parse_angle_rad).is_some() {
        format_angle_rad(value)
    } else {
        format_f64_literal(value)
    }
}

pub(crate) fn populate_parameter_dependencies(
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    for parameter in parameters.iter_mut() {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let mut seen = std::collections::HashSet::new();
        parameter.dependencies = expression_identifiers(&parameter.expression)
            .filter_map(|identifier| aliases.get(&identifier).and_then(Clone::clone))
            .filter(|dependency| dependency != &parameter.id && seen.insert(dependency.clone()))
            .collect();
    }
}

pub(crate) fn order_parameters_by_dependencies(parameters: &mut [DesignParameter]) {
    let mut seen_owners = std::collections::HashSet::new();
    let owner_order = parameters
        .iter()
        .map(|parameter| parameter.owner.clone())
        .filter(|owner| seen_owners.insert(owner.clone()))
        .collect::<Vec<_>>();
    let parameter_owners = parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect::<HashMap<_, _>>();
    for owner in owner_order {
        let mut remaining = parameters
            .iter()
            .enumerate()
            .filter(|(_, parameter)| parameter.owner == owner)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut ordered = Vec::<usize>::with_capacity(remaining.len());
        let mut ordered_ids = std::collections::HashSet::new();
        while !remaining.is_empty() {
            let Some(position) = remaining.iter().position(|index| {
                parameters[*index].dependencies.iter().all(|dependency| {
                    parameter_owners
                        .get(dependency)
                        .is_none_or(|dependency_owner| dependency_owner != &owner)
                        || ordered_ids.contains(dependency)
                })
            }) else {
                ordered.clear();
                break;
            };
            let index = remaining.remove(position);
            ordered_ids.insert(parameters[index].id.clone());
            ordered.push(index);
        }
        for (ordinal, index) in ordered.into_iter().enumerate() {
            parameters[index].ordinal = ordinal as u32;
        }
    }
}

#[cfg(test)]
pub(crate) fn parameter_aliases(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    expression_owner: Option<&FeatureId>,
) -> HashMap<String, Option<ParameterId>> {
    ParameterAliases::new(parameters, feature_names, global_owners).materialize(expression_owner)
}

pub(crate) fn insert_parameter_alias(
    aliases: &mut HashMap<String, Option<ParameterId>>,
    alias: String,
    parameter: &ParameterId,
) {
    aliases
        .entry(alias)
        .and_modify(|candidate| {
            if candidate
                .as_ref()
                .is_some_and(|existing| existing != parameter)
            {
                *candidate = None;
            }
        })
        .or_insert_with(|| Some(parameter.clone()));
}

pub(crate) struct ParameterAliases {
    global: HashMap<String, Option<ParameterId>>,
    exact: HashMap<String, Option<ParameterId>>,
    document_local: HashMap<String, Option<ParameterId>>,
    feature_local: HashMap<FeatureId, HashMap<String, Option<ParameterId>>>,
}

impl ParameterAliases {
    pub(crate) fn new(
        parameters: &[DesignParameter],
        feature_names: &HashMap<FeatureId, String>,
        global_owners: &HashSet<FeatureId>,
    ) -> Self {
        let mut aliases = Self {
            global: HashMap::new(),
            exact: HashMap::new(),
            document_local: HashMap::new(),
            feature_local: HashMap::new(),
        };
        for parameter in parameters {
            insert_parameter_alias(&mut aliases.exact, parameter.id.0.clone(), &parameter.id);
            let mut unqualified = vec![parameter.name.clone()];
            if let Some(equation_id) = parameter
                .properties
                .get("EquationId")
                .filter(|equation_id| !equation_id.contains('@'))
            {
                unqualified.push(equation_id.clone());
            }
            if let Some(owner_name) = parameter
                .owner
                .as_ref()
                .and_then(|owner| feature_names.get(owner))
            {
                insert_parameter_alias(
                    &mut aliases.exact,
                    format!("{}@{owner_name}", parameter.name),
                    &parameter.id,
                );
                if let Some(equation_id) = parameter.properties.get("EquationId") {
                    let qualified = if equation_id.contains('@') {
                        equation_id.clone()
                    } else {
                        format!("{equation_id}@{owner_name}")
                    };
                    insert_parameter_alias(&mut aliases.exact, qualified, &parameter.id);
                }
            }
            if parameter
                .owner
                .as_ref()
                .is_some_and(|owner| global_owners.contains(owner))
            {
                for alias in &unqualified {
                    insert_parameter_alias(&mut aliases.global, alias.clone(), &parameter.id);
                }
            }
            let local = parameter
                .owner
                .as_ref()
                .map(|owner| aliases.feature_local.entry(owner.clone()).or_default())
                .unwrap_or(&mut aliases.document_local);
            for alias in unqualified {
                insert_parameter_alias(local, alias, &parameter.id);
            }
        }
        aliases
    }

    pub(crate) fn for_owner<'a>(&'a self, owner: Option<&'a FeatureId>) -> ParameterAliasView<'a> {
        ParameterAliasView {
            aliases: self,
            owner,
        }
    }

    #[cfg(test)]
    pub(crate) fn materialize(
        &self,
        owner: Option<&FeatureId>,
    ) -> HashMap<String, Option<ParameterId>> {
        let mut aliases = self.global.clone();
        aliases.extend(
            owner
                .and_then(|owner| self.feature_local.get(owner))
                .unwrap_or(&self.document_local)
                .clone(),
        );
        aliases.extend(self.exact.clone());
        aliases
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ParameterAliasView<'a> {
    aliases: &'a ParameterAliases,
    owner: Option<&'a FeatureId>,
}

impl ParameterAliasView<'_> {
    pub(crate) fn get(&self, alias: &str) -> Option<&Option<ParameterId>> {
        self.aliases
            .exact
            .get(alias)
            .or_else(|| {
                self.owner
                    .and_then(|owner| self.aliases.feature_local.get(owner))
                    .unwrap_or(&self.aliases.document_local)
                    .get(alias)
            })
            .or_else(|| self.aliases.global.get(alias))
    }
}

pub(crate) fn parameter_aliases_by_owner(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> ParameterAliases {
    ParameterAliases::new(parameters, feature_names, global_owners)
}

pub(crate) fn evaluate_parameter_expressions(
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    let mut values = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .value
                .clone()
                .map(|value| (parameter.id.clone(), value))
        })
        .collect::<HashMap<_, _>>();
    loop {
        let mut changed = false;
        for parameter in parameters
            .iter_mut()
            .filter(|parameter| parameter.value.is_none())
        {
            let aliases = aliases.for_owner(parameter.owner.as_ref());
            let Some(value) =
                ParameterExpressionParser::new(&parameter.expression, aliases, &values).parse()
            else {
                continue;
            };
            if !parameter_value_is_finite(&value) {
                continue;
            }
            values.insert(parameter.id.clone(), value.clone());
            parameter.value = Some(value);
            changed = true;
        }
        if !changed {
            break;
        }
    }
}

pub(crate) fn parameters_with_unresolved_references(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> usize {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    parameters
        .iter()
        .filter(|parameter| {
            let aliases = aliases.for_owner(parameter.owner.as_ref());
            let parsed = expression_identifier_tokens(&parameter.expression);
            parsed.unclosed_quote
                || parsed
                    .identifiers
                    .into_iter()
                    .filter(|identifier| {
                        !expression_identifier_is_syntax(&parameter.expression, identifier)
                    })
                    .filter(definite_parameter_reference)
                    .any(|identifier| {
                        aliases
                            .get(&identifier.value)
                            .and_then(Clone::clone)
                            .is_none_or(|dependency| dependency == parameter.id)
                    })
        })
        .count()
}

pub(crate) fn parameters_with_unevaluable_expressions(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    let mut states = parameter_value_states(parameters, configurations, false);
    parameters
        .iter()
        .filter(|parameter| {
            let aliases = aliases.for_owner(parameter.owner.as_ref());
            states.iter_mut().any(|values| {
                let own = values.remove(&parameter.id);
                let evaluated =
                    ParameterExpressionParser::new(&parameter.expression, aliases, values)
                        .parse()
                        .or_else(|| text_parameter_literal(&parameter.name, &parameter.expression))
                        .filter(parameter_value_is_finite);
                if let Some(value) = own {
                    values.insert(parameter.id.clone(), value);
                }
                evaluated.is_none()
            })
        })
        .count()
}

pub(crate) fn parameters_with_incoherent_dependencies(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> usize {
    let mut projected = parameters.to_vec();
    populate_parameter_dependencies(&mut projected, feature_names, global_owners);
    parameters
        .iter()
        .zip(projected)
        .filter(|(actual, projected)| actual.dependencies != projected.dependencies)
        .count()
}

pub(crate) fn parameters_with_incoherent_evaluated_values(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> usize {
    let aliases = parameter_aliases_by_owner(parameters, feature_names, global_owners);
    let mut states = parameter_value_states(parameters, configurations, true);
    parameters
        .iter()
        .filter(|parameter| !parameter.dependencies.is_empty())
        .filter(|parameter| {
            let aliases = aliases.for_owner(parameter.owner.as_ref());
            states.iter_mut().any(|values| {
                let actual = values.remove(&parameter.id);
                let evaluated =
                    ParameterExpressionParser::new(&parameter.expression, aliases, values)
                        .parse()
                        .filter(parameter_value_is_finite);
                if let Some(value) = actual.clone() {
                    values.insert(parameter.id.clone(), value);
                }
                actual.zip(evaluated).is_some_and(|(actual, evaluated)| {
                    !equivalent_parameter_values(&actual, &evaluated)
                })
            })
        })
        .count()
}

pub(crate) fn parameter_value_states(
    parameters: &[DesignParameter],
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
    include_global: bool,
) -> Vec<HashMap<ParameterId, ParameterValue>> {
    let global_values = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .value
                .clone()
                .map(|value| (parameter.id.clone(), value))
        })
        .collect::<HashMap<_, _>>();
    let mut states = include_global
        .then(|| global_values.clone())
        .into_iter()
        .collect::<Vec<_>>();
    states.extend(configurations.iter().map(|configuration| {
        let mut values = global_values.clone();
        values.extend(configuration.parameter_values.clone());
        values
    }));
    if states.is_empty() {
        states.push(global_values);
    }
    states
}

pub(crate) fn equivalent_parameter_values(left: &ParameterValue, right: &ParameterValue) -> bool {
    let close = |left: f64, right: f64| {
        (left - right).abs()
            <= EPS_PARAMETERS_EQUIVALENT_PARAMETER_VALUES_E9 * (1.0 + left.abs().max(right.abs()))
    };
    match (left, right) {
        (ParameterValue::Length(Length(left)), ParameterValue::Length(Length(right)))
        | (ParameterValue::Angle(Angle(left)), ParameterValue::Angle(Angle(right)))
        | (ParameterValue::Real(left), ParameterValue::Real(right)) => close(*left, *right),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => left == right,
        (ParameterValue::Boolean(left), ParameterValue::Boolean(right)) => left == right,
        (ParameterValue::Integer(integer), ParameterValue::Real(real))
        | (ParameterValue::Real(real), ParameterValue::Integer(integer)) => {
            exact_integer_f64(*integer) == Some(*real)
        }
        _ => false,
    }
}

pub(crate) fn definite_parameter_reference(identifier: &ExpressionIdentifier) -> bool {
    identifier.quoted
        || identifier.value.contains('@')
        || identifier.value.strip_prefix('D').is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(crate) fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> + '_ {
    expression_identifier_tokens(expression)
        .identifiers
        .into_iter()
        .filter(|token| !expression_identifier_is_syntax(expression, token))
        .map(|token| token.value)
}

pub(crate) fn expression_identifier_is_syntax(
    expression: &str,
    identifier: &ExpressionIdentifier,
) -> bool {
    if identifier.quoted {
        return false;
    }
    if identifier
        .value
        .starts_with(|character: char| character.is_ascii_digit() || character == '.')
    {
        return true;
    }
    if identifier.value.eq_ignore_ascii_case("pi")
        || identifier.value.eq_ignore_ascii_case("true")
        || identifier.value.eq_ignore_ascii_case("false")
    {
        return true;
    }
    let is_function = matches!(
        identifier.value.to_ascii_lowercase().as_str(),
        "iif"
            | "abs"
            | "sin"
            | "cos"
            | "tan"
            | "sec"
            | "cosec"
            | "cotan"
            | "arcsin"
            | "arccos"
            | "atn"
            | "arcsec"
            | "arccosec"
            | "arccotan"
            | "exp"
            | "log"
            | "sqr"
            | "int"
            | "sgn"
    );
    is_function && expression[identifier.end..].trim_start().starts_with('(')
}

pub(crate) struct ParsedExpressionIdentifiers {
    pub(crate) identifiers: Vec<ExpressionIdentifier>,
    pub(crate) unclosed_quote: bool,
}

pub(crate) struct ExpressionIdentifier {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) value: String,
    pub(crate) quoted: bool,
}

pub(crate) fn expression_identifier_tokens(expression: &str) -> ParsedExpressionIdentifiers {
    let mut identifiers = Vec::new();
    let mut at = 0;
    while at < expression.len() {
        let rest = &expression[at..];
        if rest.starts_with('"') {
            let mut value = String::new();
            let mut cursor = at + 1;
            let mut closed = false;
            while cursor < expression.len() {
                let quoted = &expression[cursor..];
                if quoted.starts_with("\"\"") {
                    value.push('"');
                    cursor += 2;
                } else if quoted.starts_with('"') {
                    cursor += 1;
                    closed = true;
                    break;
                } else {
                    let character = quoted.chars().next().expect("nonempty suffix");
                    value.push(character);
                    cursor += character.len_utf8();
                }
            }
            if closed {
                if !value.is_empty() {
                    identifiers.push(ExpressionIdentifier {
                        start: at,
                        end: cursor,
                        value,
                        quoted: true,
                    });
                }
                at = cursor;
                continue;
            }
            return ParsedExpressionIdentifiers {
                identifiers,
                unclosed_quote: true,
            };
        }

        let Some(character) = rest.chars().next() else {
            break;
        };
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.') {
            let end = rest
                .find(|candidate: char| {
                    !(candidate.is_ascii_alphanumeric()
                        || matches!(candidate, '_' | '@' | '$' | '.'))
                })
                .unwrap_or(rest.len());
            identifiers.push(ExpressionIdentifier {
                start: at,
                end: at + end,
                value: rest[..end].to_string(),
                quoted: false,
            });
            at += end;
        } else {
            at += character.len_utf8();
        }
    }
    ParsedExpressionIdentifiers {
        identifiers,
        unclosed_quote: false,
    }
}
