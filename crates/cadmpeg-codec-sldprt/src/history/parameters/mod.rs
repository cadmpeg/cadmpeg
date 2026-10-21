// SPDX-License-Identifier: Apache-2.0
//! Native Keywords parameter projection and equation evaluation.

use crate::classification::{classify, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureHistory};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        DesignParameter, DimensionDisplay, Expression, ExpressionToken, FeatureDefinition,
        FeatureId, FeatureOperation, FeatureTreeNodeRole, ParameterId, ParameterValue,
    },
    scalar::{Angle, FiniteReal, Length},
};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

use crate::history::classify::{
    feature_family, feature_input_class, is_chamfer, is_extrude, is_fillet, is_offset_plane,
    HistoryMetadata, EQUATION_DRIVEN_TOKEN,
};
use crate::history::literals::{
    dimension_display, format_angle_rad, format_f64_literal, format_length_mm,
    format_length_number, format_parameter_value, parse_angle_rad, parse_dimension_display_length,
    parse_parameter_literal, parse_dimension_length_mm,
};
use crate::history::project::pattern::{pattern_form, NativePatternClass};
use crate::history::project::{
    neutral_feature_id, neutral_parameter_id, projected_parameter_names,
};

const EPS_PARAMETERS_EQUIVALENT_PARAMETER_VALUES_E9: f64 = 1.0e-9;

#[cfg(test)]
mod alias_tests;
pub(crate) mod eval;
#[cfg(test)]
mod literal_tests;

use self::eval::{exact_integer_f64, ParameterExpressionParser};

pub(crate) fn project_parameters(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<DesignParameter>, CodecError> {
    let mut feature_names = HashMap::new();
    let mut global_owners = HashSet::new();
    let mut parameters = Vec::new();
    let mut source_aliases = HashMap::new();
    for history in histories {
        let metadata = HistoryMetadata::new(&history.features);
        for feature in &history.features {
            if metadata.contains(feature) {
                continue;
            }
            if !feature.name.is_empty() {
                let id = neutral_feature_id(&feature.id)?;
                let name = feature.name.clone();
                feature_names.insert(id, name);
            }
            if feature.kind.eq_ignore_ascii_case("EquationDriven") {
                let owner = neutral_feature_id(&feature.id)?;
                global_owners.insert(owner);
            }
            for (ordinal, name) in projected_parameter_names(feature).into_iter().enumerate() {
                let expression = &feature.parameters[name.as_str()];
                let display = dimension_display(expression);
                let mut properties = feature
                    .dimension_properties
                    .get(&name)
                    .cloned()
                    .unwrap_or_default();
                let value = source_parameter_value(feature, &name);
                let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
                    CodecError::malformed("SLDPRT feature parameter ordinal exceeds u32")
                })?;
                let driving = properties
                    .get("Driven")
                    .is_none_or(|value| !matches!(value.as_str(), "true" | "1"));
                let id = neutral_parameter_id(feature, ordinal)?;
                // Decode-only source spelling. Canonical expressions retain
                // the identity token and never this duplicate alias property.
                if let Some(alias) = properties.remove("EquationId") {
                    source_aliases.insert(id.clone(), alias);
                }
                let parameter = DesignParameter {
                    id,
                    owner: Some(neutral_feature_id(&feature.id)?),
                    ordinal: ordinal_u32,
                    properties: {
                        properties.remove("Value");
                        properties.remove("Driven");
                        properties
                    },
                    name,
                    expression: Expression::text(strip_display(expression, display)),
                    driving,
                    display,
                    value,
                    native_ref: None,
                    pmi: None,
                };
                parameters.push(parameter);
            }
        }
    }
    tokenize_parameter_expressions(
        &mut parameters,
        &feature_names,
        &global_owners,
        &source_aliases,
    )?;
    evaluate_parameter_expressions(ctx, &mut parameters, &feature_names, &global_owners)?;
    for parameter in parameters
        .iter_mut()
        .filter(|parameter| parameter.value.is_none())
    {
        parameter.value = parameter
            .expression
            .literal_text()
            .and_then(|expression| text_parameter_literal(&parameter.name, &expression));
    }
    Ok(parameters)
}

fn text_parameter_literal(name: &str, expression: &str) -> Option<ParameterValue> {
    bare_text_parameter_literal(expression)
        .or_else(|| formatted_text_dimension_literal(name, expression))
}

fn bare_text_parameter_literal(expression: &str) -> Option<ParameterValue> {
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
    let identifiers = expression_identifier_tokens(expression)?;
    if identifiers.iter().any(definite_parameter_reference) {
        return None;
    }
    Some(ParameterValue::String(expression.to_owned()))
}

fn formatted_text_dimension_literal(name: &str, expression: &str) -> Option<ParameterValue> {
    formatted_text_dimension_value(name, expression)
        .map(|value| ParameterValue::String(value.to_owned()))
}

fn formatted_text_dimension_value<'a>(name: &str, expression: &'a str) -> Option<&'a str> {
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
    (tags > 0 && !rest.contains('>')).then_some(expression)
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
        .filter(|feature| is_global_parameter_owner(feature))
        .map(|feature| feature.id.clone())
        .collect()
}

pub(crate) fn is_global_parameter_owner(feature: &cadmpeg_ir::features::Feature) -> bool {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::Native { kind, .. }) => {
            kind.as_str().eq_ignore_ascii_case(EQUATION_DRIVEN_TOKEN)
        }
        FeatureDefinition::Operation(FeatureOperation::TreeNode { role, .. }) => {
            *role == FeatureTreeNodeRole::Equations
        }
        _ => false,
    }
}

/// Replace evaluable expressions with canonical literals in a temporary history projection.
///
/// Retained native histories keep their source expressions.
pub(super) fn apply_evaluated_parameters(
    ctx: &DecodeContext<'_>,
    histories: &mut [FeatureHistory],
) -> Result<(), CodecError> {
    let evaluated = project_parameters(ctx, histories)?;
    for feature in histories
        .iter_mut()
        .flat_map(|history| &mut history.features)
    {
        let owner = neutral_feature_id(&feature.id)?;
        let mut replacements = Vec::new();
        for (name, expression) in &feature.parameters {
            if parse_native_parameter_literal(feature, name.as_str(), expression).is_some() {
                continue;
            }
            let value = evaluated
                .iter()
                .rev()
                .find(|parameter| {
                    parameter.value.is_some()
                        && parameter.owner.as_ref() == Some(&owner)
                        && parameter.name == name.as_str()
                })
                .and_then(|parameter| parameter.value.as_ref());
            let Some(value) = value else {
                continue;
            };
            let value = match value {
                ParameterValue::String(value) => value.clone(),
                _ => format_parameter_value(value),
            };
            let name = cadmpeg_core::text::NonBlankString::new(name.as_str().to_owned())
                .ok_or_else(|| CodecError::malformed("blank SLDPRT evaluated parameter name"))?;
            replacements.push((name, value));
        }
        for (name, value) in replacements {
            feature.parameters.insert(name, value);
        }
    }
    Ok(())
}

/// Read one source parameter cache during typed source projection.
/// Canonical neutral consumers resolve their `Dimension` through the document.
pub(crate) fn source_parameter_value(feature: &Feature, name: &str) -> Option<ParameterValue> {
    let expression = feature.parameters.get(name)?;
    let display = dimension_display(expression);
    let parse_value = |value: &str| match display {
        Some(DimensionDisplay::Diameter | DimensionDisplay::Radius) => {
            parse_dimension_display_length(value).map(ParameterValue::Length)
        }
        None => parse_native_parameter_literal(feature, name, value),
    };
    feature
        .dimension_properties
        .get(name)
        .and_then(|properties| properties.get("Value"))
        .and_then(|value| parse_value(value))
        .or_else(|| parse_value(expression))
}

pub(crate) fn parse_native_parameter_literal(
    feature: &Feature,
    name: &str,
    expression: &str,
) -> Option<ParameterValue> {
    if feature
        .parameters
        .get(name)
        .and_then(|source| dimension_display(source))
        .is_some()
    {
        return parse_dimension_display_length(expression).map(ParameterValue::Length);
    }
    if classify(feature) == Some(FeatureClass::Sketch) {
        if let Ok(Some(controls)) =
            crate::history::project::solid::hole_profile_parameter_values(feature)
        {
            if let Some(role) = controls.get(name) {
                return match role {
                    ParameterValue::Length(_) => {
                        parse_dimension_length_mm(expression).map(ParameterValue::Length)
                    }
                    ParameterValue::Angle(_) => {
                        parse_angle_rad(expression).map(ParameterValue::Angle)
                    }
                    _ => None,
                };
            }
        }
    }
    if native_parameter_is_length(feature, name, Some(expression)) {
        return parse_dimension_length_mm(expression).map(ParameterValue::Length);
    }
    parse_parameter_literal(expression)
}

pub(super) fn native_parameter_is_length(
    feature: &Feature,
    name: &str,
    expression: Option<&str>,
) -> bool {
    let cosmetic_thread = classify(feature) == Some(FeatureClass::CosmeticThread);
    if is_chamfer(feature) && ordered_chamfer_length_control(feature, name) {
        return true;
    }
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
            Some(NativePatternClass::Linear | NativePatternClass::CurveDriven)
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

/// The ordered one- or two-control chamfer schema used by operation projection.
fn ordered_chamfer_length_control(feature: &Feature, name: &str) -> bool {
    let controls = feature
        .content
        .iter()
        .filter_map(|content| match content {
            crate::records::FeatureContent::Dimension(name) => feature
                .parameters
                .get(name.as_str())
                .map(|value| (name.as_str(), value.as_str())),
            crate::records::FeatureContent::Feature(_)
            | crate::records::FeatureContent::Text(_) => None,
        })
        .collect::<Vec<_>>();
    match controls.as_slice() {
        [(control, _)] => *control == name,
        [(first, first_value), (second, second_value)] => {
            let first_angle =
                crate::history::literals::parse_bounded_angle_rad(first_value).is_some();
            let second_angle =
                crate::history::literals::parse_bounded_angle_rad(second_value).is_some();
            (name == *first && !first_angle) || (name == *second && !second_angle)
        }
        _ => false,
    }
}

/// The expression of a native scalar `value` in metres or radians, in the
/// display form of `expression`, or `None` when the value in its unit is not
/// finite.
pub(crate) fn format_native_scalar(
    feature: &Feature,
    name: &str,
    value: f64,
    expression: Option<&str>,
) -> Option<String> {
    if let Some(display) = expression.and_then(dimension_display) {
        let prefix = match display {
            DimensionDisplay::Diameter => expression
                .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;"),
            DimensionDisplay::Radius => expression
                .filter(|value| value.trim().starts_with("&lt;MOD-RHO&gt;"))
                .map_or("<MOD-RHO>", |_| "&lt;MOD-RHO&gt;"),
        };
        Some(format!(
            "{prefix}{}",
            format_length_number(Length::new(value * 1000.0)?)
        ))
    } else if native_parameter_is_length(feature, name, expression) {
        Length::new(value * 1000.0).map(format_length_mm)
    } else if expression.and_then(parse_angle_rad).is_some() {
        Angle::new(value).map(format_angle_rad)
    } else {
        FiniteReal::new(value).map(format_f64_literal)
    }
}

fn strip_display(expression: &str, display: Option<DimensionDisplay>) -> &str {
    let expression = expression.trim();
    match display {
        Some(DimensionDisplay::Diameter) => expression
            .strip_prefix("<MOD-DIAM>")
            .unwrap_or(expression)
            .trim(),
        Some(DimensionDisplay::Radius) => expression.strip_prefix('R').unwrap_or(expression).trim(),
        None => expression,
    }
}

fn tokenize_parameter_expressions(
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    source_aliases: &HashMap<ParameterId, String>,
) -> Result<(), CodecError> {
    let mut aliases = ParameterAliases::new(parameters, feature_names, global_owners);
    aliases.insert_source_spellings(parameters, feature_names, global_owners, source_aliases);
    for parameter in parameters {
        let Some(text) = parameter.expression.literal_text() else {
            continue;
        };
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let spans = expression_identifier_tokens(&text)
            .unwrap_or_default()
            .into_iter()
            .filter(|token| !token.is_syntax())
            .filter_map(|token| {
                let id = aliases.get(token.value()).and_then(Option::as_ref)?.clone();
                let end = text.len() - token.following.len();
                Some(((end - token.raw.len())..end, id))
            })
            .collect::<Vec<_>>();
        parameter.expression =
            Expression::from_spans(&text, spans).map_err(CodecError::malformed)?;
    }
    Ok(())
}

/// Render only current parameter identities and names in SolidWorks syntax.
pub(crate) fn render_parameter_expression(
    parameter: &DesignParameter,
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> Option<String> {
    if let Some(value) = &parameter.value {
        if parameter.expression == value.literal_expression()
            && parameter.expression.literal_value_as(value).as_ref() == Some(value)
        {
            return Some(format_parameter_value(value));
        }
    }
    parameter.expression.render_with(|id| {
        let target = parameters.iter().find(|target| &target.id == id)?;
        let mut name = target.name.to_string();
        if target.owner != parameter.owner
            && !target
                .owner
                .as_ref()
                .is_some_and(|owner| global_owners.contains(owner))
        {
            if let Some(owner) = target
                .owner
                .as_ref()
                .and_then(|owner| feature_names.get(owner))
            {
                name = format!("{name}@{owner}");
            }
        }
        Some(render_identifier(&name))
    })
}

pub(crate) fn render_identifier(name: &str) -> String {
    let mut characters = name.chars();
    let unquoted = characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && characters
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '.'));
    if unquoted {
        name.to_owned()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}

#[cfg(test)]
fn parameter_aliases(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    expression_owner: Option<&FeatureId>,
) -> HashMap<String, Option<ParameterId>> {
    ParameterAliases::new(parameters, feature_names, global_owners).materialize(expression_owner)
}

fn insert_parameter_alias(
    aliases: &mut HashMap<String, Option<ParameterId>>,
    alias: String,
    parameter: &ParameterId,
) {
    if let Some(candidate) = aliases.get_mut(&alias) {
        if candidate
            .as_ref()
            .is_some_and(|existing| existing != parameter)
        {
            *candidate = None;
        }
    } else {
        aliases.insert(alias, Some(parameter.clone()));
    }
}

struct ParameterAliases {
    global: HashMap<String, Option<ParameterId>>,
    exact: HashMap<String, Option<ParameterId>>,
    document_local: HashMap<String, Option<ParameterId>>,
    feature_local: HashMap<FeatureId, HashMap<String, Option<ParameterId>>>,
}

impl ParameterAliases {
    pub(super) fn new(
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
            insert_parameter_alias(
                &mut aliases.exact,
                parameter.id.as_str().to_owned(),
                &parameter.id,
            );
            let unqualified = [Some(parameter.name.as_str())];
            if let Some(owner_name) = parameter
                .owner
                .as_ref()
                .and_then(|owner| feature_names.get(owner))
            {
                let qualified = format!("{}@{owner_name}", parameter.name);
                insert_parameter_alias(&mut aliases.exact, qualified, &parameter.id);
            }
            if parameter
                .owner
                .as_ref()
                .is_some_and(|owner| global_owners.contains(owner))
            {
                for alias in unqualified.into_iter().flatten() {
                    insert_parameter_alias(&mut aliases.global, alias.to_owned(), &parameter.id);
                }
            }
            let local = if let Some(owner) = parameter.owner.as_ref() {
                aliases.feature_local.entry(owner.clone()).or_default()
            } else {
                &mut aliases.document_local
            };
            for alias in unqualified.into_iter().flatten() {
                insert_parameter_alias(local, alias.to_owned(), &parameter.id);
            }
        }
        aliases
    }

    /// Source aliases exist only while converting source text to identity tokens.
    fn insert_source_spellings(
        &mut self,
        parameters: &[DesignParameter],
        feature_names: &HashMap<FeatureId, String>,
        global_owners: &HashSet<FeatureId>,
        spellings: &HashMap<ParameterId, String>,
    ) {
        for parameter in parameters {
            let Some(spelling) = spellings.get(&parameter.id) else {
                continue;
            };
            if let Some(owner_name) = parameter
                .owner
                .as_ref()
                .and_then(|owner| feature_names.get(owner))
            {
                let qualified = if spelling.contains('@') {
                    spelling.clone()
                } else {
                    format!("{spelling}@{owner_name}")
                };
                insert_parameter_alias(&mut self.exact, qualified, &parameter.id);
            }
            if spelling.contains('@') {
                continue;
            }
            if parameter
                .owner
                .as_ref()
                .is_some_and(|owner| global_owners.contains(owner))
            {
                insert_parameter_alias(&mut self.global, spelling.clone(), &parameter.id);
            }
            let local = parameter
                .owner
                .as_ref()
                .map_or(&mut self.document_local, |owner| {
                    self.feature_local.entry(owner.clone()).or_default()
                });
            insert_parameter_alias(local, spelling.clone(), &parameter.id);
        }
    }

    pub(super) fn for_owner<'a>(&'a self, owner: Option<&'a FeatureId>) -> ParameterAliasView<'a> {
        ParameterAliasView {
            aliases: self,
            owner,
        }
    }

    #[cfg(test)]
    pub(super) fn materialize(
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
pub(in crate::history) struct ParameterAliasView<'a> {
    aliases: &'a ParameterAliases,
    owner: Option<&'a FeatureId>,
}

impl ParameterAliasView<'_> {
    pub(super) fn get(&self, alias: &str) -> Option<&Option<ParameterId>> {
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

fn evaluate_parameter_expressions(
    ctx: &DecodeContext<'_>,
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> Result<(), CodecError> {
    let expressions = parameters
        .iter()
        .filter_map(|parameter| {
            render_parameter_expression(parameter, parameters, feature_names, global_owners)
                .map(|expression| (parameter.id.clone(), expression))
        })
        .collect::<HashMap<_, _>>();
    let aliases = ParameterAliases::new(parameters, feature_names, global_owners);
    let mut values = HashMap::new();
    for parameter in parameters.iter() {
        if let Some(value) = &parameter.value {
            insert_parameter_value(&mut values, &parameter.id, value);
        }
    }
    loop {
        let mut changed = false;
        for parameter in parameters
            .iter_mut()
            .filter(|parameter| parameter.value.is_none())
        {
            let aliases = aliases.for_owner(parameter.owner.as_ref());
            let Some(expression) = expressions.get(&parameter.id) else {
                continue;
            };
            let Some(value) =
                ParameterExpressionParser::new(ctx, expression, aliases, &values).parse()?
            else {
                continue;
            };
            insert_parameter_value(&mut values, &parameter.id, &value);
            parameter.value = Some(value);
            changed = true;
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

fn insert_parameter_value(
    values: &mut HashMap<ParameterId, ParameterValue>,
    id: &ParameterId,
    value: &ParameterValue,
) {
    let value = value.clone();
    values.insert(id.clone(), value);
}

pub(crate) fn parameters_with_unresolved_references(
    parameters: &[DesignParameter],
    _feature_names: &HashMap<FeatureId, String>,
    _global_owners: &HashSet<FeatureId>,
) -> usize {
    parameters
        .iter()
        .filter(|parameter| {
            parameter.expression.tokens.iter().any(|token| {
                let ExpressionToken::Text(text) = token else {
                    return false;
                };
                expression_identifier_tokens(text).is_none_or(|tokens| {
                    tokens.iter().any(|identifier| {
                        !identifier.is_syntax() && definite_parameter_reference(identifier)
                    })
                })
            })
        })
        .count()
}

pub(crate) fn parameters_with_unevaluable_expressions(
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> Result<usize, CodecError> {
    if parameters.is_empty() {
        return Ok(0);
    }
    let aliases = ParameterAliases::new(parameters, feature_names, global_owners);
    let mut states = parameter_value_states(parameters, configurations, false)?;
    let mut count = 0;
    for parameter in parameters {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let Some(expression) =
            render_parameter_expression(parameter, parameters, feature_names, global_owners)
        else {
            count += 1;
            continue;
        };
        for values in &mut states {
            let own = values.remove_entry(&parameter.id);
            let evaluated =
                match ParameterExpressionParser::new(ctx, &expression, aliases, values).parse()? {
                    Some(value) => Some(value),
                    None => text_parameter_literal(&parameter.name, &expression),
                };
            if let Some((id, value)) = own {
                values.insert(id, value);
            }
            if evaluated.is_none() {
                count += 1;
                break;
            }
        }
    }
    Ok(count)
}

pub(crate) fn parameters_with_incoherent_dependencies(
    parameters: &[DesignParameter],
    _feature_names: &HashMap<FeatureId, String>,
    _global_owners: &HashSet<FeatureId>,
) -> usize {
    let ids = parameters
        .iter()
        .map(|parameter| &parameter.id)
        .collect::<HashSet<_>>();
    parameters
        .iter()
        .filter(|parameter| {
            parameter
                .expression
                .dependencies()
                .any(|id| !ids.contains(id))
        })
        .count()
}

pub(crate) fn parameters_with_incoherent_evaluated_values(
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> Result<usize, CodecError> {
    if parameters.is_empty() {
        return Ok(0);
    }
    let aliases = ParameterAliases::new(parameters, feature_names, global_owners);
    let mut states = parameter_value_states(parameters, configurations, true)?;
    let mut count = 0;
    for parameter in parameters
        .iter()
        .filter(|parameter| parameter.expression.dependencies().next().is_some())
    {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let Some(expression) =
            render_parameter_expression(parameter, parameters, feature_names, global_owners)
        else {
            count += 1;
            continue;
        };
        for values in &mut states {
            let own = values.remove_entry(&parameter.id);
            let evaluated =
                ParameterExpressionParser::new(ctx, &expression, aliases, values).parse()?;
            let incoherent =
                own.as_ref()
                    .zip(evaluated.as_ref())
                    .is_some_and(|((_, actual), evaluated)| {
                        !equivalent_parameter_values(actual, evaluated)
                    });
            if let Some((id, value)) = own {
                values.insert(id, value);
            }
            if incoherent {
                count += 1;
                break;
            }
        }
    }
    Ok(count)
}

fn parameter_value_states(
    parameters: &[DesignParameter],
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
    include_global: bool,
) -> Result<Vec<HashMap<ParameterId, ParameterValue>>, CodecError> {
    let make_state = |configuration: Option<&cadmpeg_ir::features::DesignConfiguration>| -> Result<_, CodecError> {
        let mut values = HashMap::new();
        for parameter in parameters {
            if let Some(value) = &parameter.value { insert_parameter_value(&mut values, &parameter.id, value); }
        }
        if let Some(configuration) = configuration {
            for (id, value) in &configuration.parameter_values { insert_parameter_value(&mut values, id, value); }
        }
        Ok(values)
    };
    let mut states = Vec::new();
    if include_global || configurations.is_empty() {
        let state = make_state(None)?;
        states.push(state);
    }
    for configuration in configurations {
        let state = make_state(Some(configuration))?;
        states.push(state);
    }
    Ok(states)
}

fn equivalent_parameter_values(left: &ParameterValue, right: &ParameterValue) -> bool {
    let close = |left: f64, right: f64| {
        (left - right).abs()
            <= EPS_PARAMETERS_EQUIVALENT_PARAMETER_VALUES_E9 * (1.0 + left.abs().max(right.abs()))
    };
    match (left, right) {
        (ParameterValue::Length(left), ParameterValue::Length(right)) => {
            close(left.get(), right.get())
        }
        (ParameterValue::Angle(left), ParameterValue::Angle(right)) => {
            close(left.get(), right.get())
        }
        (ParameterValue::Real(left), ParameterValue::Real(right)) => close(left.get(), right.get()),
        (ParameterValue::Integer(left), ParameterValue::Integer(right)) => left == right,
        (ParameterValue::Boolean(left), ParameterValue::Boolean(right)) => left == right,
        (ParameterValue::Integer(integer), ParameterValue::Real(real))
        | (ParameterValue::Real(real), ParameterValue::Integer(integer)) => {
            exact_integer_f64(*integer) == Some(real.get())
        }
        _ => false,
    }
}

pub(super) fn definite_parameter_reference(identifier: &ExpressionIdentifier<'_>) -> bool {
    identifier.is_quoted()
        || identifier.value().contains('@')
        || identifier.value().strip_prefix('D').is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> {
    expression_identifier_tokens(expression)
        .unwrap_or_default()
        .into_iter()
        .filter(|token| !token.is_syntax())
        .map(|token| token.value().to_owned())
        .collect::<Vec<_>>()
        .into_iter()
}

/// One identifier token of the expression it borrows from.
pub(super) struct ExpressionIdentifier<'a> {
    raw: &'a str,
    following: &'a str,
    value: Cow<'a, str>,
    quoted: bool,
}

impl<'a> ExpressionIdentifier<'a> {
    /// The token spanning `start..end`, which must be a quoted run around a nonempty name.
    fn quoted(expression: &'a str, start: usize, end: usize) -> Option<Self> {
        let raw = expression.get(start..end)?;
        let following = expression.get(end..)?;
        let inner = raw
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .filter(|inner| !inner.is_empty())?;
        let value = if inner.contains("\"\"") {
            Cow::Owned(inner.replace("\"\"", "\""))
        } else {
            Cow::Borrowed(inner)
        };
        Some(Self {
            raw,
            following,
            value,
            quoted: true,
        })
    }

    /// The unquoted token spanning `start..end`.
    fn plain(expression: &'a str, start: usize, end: usize) -> Option<Self> {
        let raw = expression.get(start..end)?;
        Some(Self {
            raw,
            following: expression.get(end..)?,
            value: Cow::Borrowed(raw),
            quoted: false,
        })
    }

    /// The identifier text, with the quotes and doubled quotes resolved.
    pub(super) fn value(&self) -> &str {
        &self.value
    }

    /// Whether the source spelled this identifier in quotes.
    pub(super) fn is_quoted(&self) -> bool {
        self.quoted
    }

    /// Whether the token is expression syntax — a literal, a constant, or a function name
    /// applied to a following argument list — rather than a name to resolve.
    pub(super) fn is_syntax(&self) -> bool {
        if self.quoted {
            return false;
        }
        if self
            .value()
            .starts_with(|character: char| character.is_ascii_digit() || character == '.')
        {
            return true;
        }
        if self.value().eq_ignore_ascii_case("pi")
            || self.value().eq_ignore_ascii_case("true")
            || self.value().eq_ignore_ascii_case("false")
        {
            return true;
        }
        eval::ParameterFunction::parse(self.value()).is_some()
            && self.following.trim_start().starts_with('(')
    }
}

pub(super) fn expression_identifier_tokens(
    expression: &str,
) -> Option<Vec<ExpressionIdentifier<'_>>> {
    let mut identifiers = Vec::new();
    let mut at = 0;
    while let Some(character) = expression[at..].chars().next() {
        let rest = &expression[at..];
        if rest.starts_with('"') {
            let mut cursor = at + 1;
            let mut closed = false;
            while let Some(character) = expression[cursor..].chars().next() {
                let quoted = &expression[cursor..];
                if quoted.starts_with("\"\"") {
                    cursor += 2;
                } else if quoted.starts_with('"') {
                    cursor += 1;
                    closed = true;
                    break;
                } else {
                    cursor += character.len_utf8();
                }
            }
            if closed {
                if let Some(identifier) = ExpressionIdentifier::quoted(expression, at, cursor) {
                    identifiers.push(identifier);
                }
                at = cursor;
                continue;
            }
            return None;
        }

        if character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.') {
            let end = rest
                .find(|candidate: char| {
                    !(candidate.is_ascii_alphanumeric()
                        || matches!(candidate, '_' | '@' | '$' | '.'))
                })
                .unwrap_or(rest.len());
            if let Some(identifier) = ExpressionIdentifier::plain(expression, at, at + end) {
                identifiers.push(identifier);
            }
            at += end;
        } else {
            at += character.len_utf8();
        }
    }
    Some(identifiers)
}
