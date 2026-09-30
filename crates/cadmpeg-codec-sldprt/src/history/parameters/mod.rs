// SPDX-License-Identifier: Apache-2.0
//! Native Keywords parameter projection and equation evaluation.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use crate::classification::{classify, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureHistory};
use cadmpeg_ir::{
    features::{
        DesignParameter, DimensionDisplay, FeatureDefinition, FeatureId, FeatureOperation,
        FeatureTreeNodeRole, ParameterId, ParameterValue,
    },
    scalar::{Angle, FiniteReal, Length},
};
use std::collections::{HashMap, HashSet};

use crate::history::classify::{
    feature_family, feature_input_class, is_chamfer, is_extrude, is_fillet,
    is_history_metadata_record, is_offset_plane, EQUATION_DRIVEN_TOKEN,
};
use crate::history::literals::{
    dimension_display, format_angle_rad, format_f64_literal, format_length_mm,
    format_length_number, format_parameter_value, parse_angle_rad, parse_dimension_display_length,
    parse_parameter_literal, parse_positive_dimension_length_mm,
};
use crate::history::project::pattern::{pattern_form, NativePatternClass};
use crate::history::project::{
    copy_projected_feature_id, copy_projected_feature_properties, copy_projected_feature_text,
    neutral_feature_id_charged, neutral_parameter_id, projected_parameter_names,
};

const EPS_PARAMETERS_EQUIVALENT_PARAMETER_VALUES_E9: f64 = 1.0e-9;

#[cfg(test)]
mod alias_tests;
pub(crate) mod eval;
#[cfg(test)]
mod literal_tests;

use self::eval::{exact_integer_f64, ParameterExpressionParser};

pub(crate) fn project_parameters(ctx: &DecodeContext<'_>, histories: &[FeatureHistory]) -> Result<Vec<DesignParameter>, CodecError> {
    let mut feature_names = HashMap::new();
    let mut global_owners = HashSet::new();
    let mut parameters = Vec::new();
    for history in histories {
        for feature in &history.features {
            ctx.charge_work(history.features.len() as u64, "classify SLDPRT parameter owners")?;
            if is_history_metadata_record(feature, &history.features) { continue; }
            if !feature.name.is_empty() {
                let id = neutral_feature_id_charged(ctx, &feature.id)?;
                let name = copy_projected_feature_text(ctx, &feature.name)?;
                if !feature_names.contains_key(&id) {
                    ctx.charge_collection_items(1, "index SLDPRT parameter owner names")?;
                    feature_names.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(
                        "index SLDPRT parameter owner names", u64::MAX - 1, u64::MAX,
                    ))?;
                }
                feature_names.insert(id, name);
            }
            if feature.kind.eq_ignore_ascii_case("EquationDriven") {
                let owner = neutral_feature_id_charged(ctx, &feature.id)?;
                if !global_owners.contains(&owner) {
                    ctx.charge_collection_items(1, "index SLDPRT global parameter owners")?;
                    global_owners.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(
                        "index SLDPRT global parameter owners", u64::MAX - 1, u64::MAX,
                    ))?;
                    global_owners.insert(owner);
                }
            }
            for (ordinal, name) in projected_parameter_names(ctx, feature)?.into_iter().enumerate() {
                let expression = &feature.parameters[name.as_str()];
                let display = dimension_display(expression);
                let properties = feature.dimension_properties.get(&name)
                    .map(|properties| copy_projected_feature_properties(ctx, properties,
                        "collect SLDPRT projected parameter properties")).transpose()?.unwrap_or_default();
                let parse_value = |value: &str| match display {
                    Some(DimensionDisplay::Diameter | DimensionDisplay::Radius) => {
                        parse_dimension_display_length(value).map(ParameterValue::Length)
                    }
                    None => parse_native_parameter_literal(feature, &name, value),
                };
                let value = properties.get("Value").and_then(|value| parse_value(value))
                    .or_else(|| parse_value(expression));
                let ordinal_u32 = u32::try_from(ordinal).map_err(|_| ctx.refuse_codec_limit(
                    "index SLDPRT parameter ordinal", u64::from(u32::MAX), ordinal as u64,
                ))?;
                let parameter = DesignParameter {
                    id: neutral_parameter_id(ctx, feature, ordinal)?,
                    owner: Some(neutral_feature_id_charged(ctx, &feature.id)?),
                    ordinal: ordinal_u32, properties, name,
                    expression: crate::text_admission::format_retained(ctx, format_args!("{expression}"), "retain SLDPRT parameter expression")?,
                    display, value,
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    native_ref: None, pmi: None,
                };
                ctx.reserve_collection_vec(&mut parameters, 1, "collect SLDPRT projected parameters")?;
                parameters.push(parameter);
            }
        }
    }
    populate_parameter_dependencies(ctx, &mut parameters, &feature_names, &global_owners)?;
    order_parameters_by_dependencies(ctx, &mut parameters)?;
    evaluate_parameter_expressions(ctx, &mut parameters, &feature_names, &global_owners)?;
    for parameter in parameters.iter_mut().filter(|parameter| parameter.value.is_none()) {
        parameter.value = text_parameter_literal(ctx, &parameter.name, &parameter.expression)?;
    }
    Ok(parameters)
}

fn text_parameter_literal(ctx: &DecodeContext<'_>, name: &str, expression: &str) -> Result<Option<ParameterValue>, CodecError> {
    match bare_text_parameter_literal(ctx, expression)? {
        Some(value) => Ok(Some(value)),
        None => formatted_text_dimension_literal(ctx, name, expression),
    }
}

fn bare_text_parameter_literal(ctx: &DecodeContext<'_>, expression: &str) -> Result<Option<ParameterValue>, CodecError> {
    ctx.charge_work(expression.len() as u64, "parse SLDPRT text parameter literal")?;
    let expression = expression.trim();
    if expression.is_empty() || expression.chars().any(|character| {
        matches!(character, '+' | '-' | '*' | '/' | '^' | '=' | '<' | '>' | '(' | ')' | ',')
    }) { return Ok(None); }
    let Some(identifiers) = expression_identifier_tokens(ctx, expression)? else { return Ok(None); };
    if identifiers.iter().any(definite_parameter_reference) { return Ok(None); }
    Ok(Some(ParameterValue::String(crate::text_admission::format_retained(ctx, format_args!("{expression}"), "retain SLDPRT text parameter literal")?)))
}

fn formatted_text_dimension_literal(ctx: &DecodeContext<'_>, name: &str, expression: &str) -> Result<Option<ParameterValue>, CodecError> {
    ctx.charge_work(name.len() as u64, "parse SLDPRT formatted parameter name")?;
    ctx.charge_work(expression.len() as u64, "parse SLDPRT formatted parameter literal")?;
    formatted_text_dimension_value(name, expression).map(|value| crate::text_admission::format_retained(ctx, 
        format_args!("{value}"), "retain SLDPRT formatted parameter literal",
    ).map(ParameterValue::String)).transpose()
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
pub(super) fn apply_evaluated_parameters(ctx: &DecodeContext<'_>, histories: &mut [FeatureHistory]) -> Result<(), CodecError> {
    let evaluated = project_parameters(ctx, histories)?;
    for feature in histories.iter_mut().flat_map(|history| &mut history.features) {
        let owner = neutral_feature_id_charged(ctx, &feature.id)?;
        let mut replacements = Vec::new();
        for (name, expression) in &feature.parameters {
            ctx.charge_work(1, "scan SLDPRT evaluated parameter replacements")?;
            if parse_native_parameter_literal(feature, name.as_str(), expression).is_some() { continue; }
            ctx.charge_work(evaluated.len() as u64, "find SLDPRT evaluated parameter replacement")?;
            let value = evaluated.iter().rev().find(|parameter| {
                parameter.value.is_some() && parameter.owner.as_ref() == Some(&owner) && parameter.name == name.as_str()
            }).and_then(|parameter| parameter.value.as_ref());
            let Some(value) = value else { continue; };
            let value = match value {
                ParameterValue::String(value) => crate::text_admission::format_retained(ctx, format_args!("{value}"), "retain SLDPRT evaluated parameter text")?,
                _ => format_parameter_value(value),
            };
            let name = cadmpeg_core::text::NonBlankString::new(copy_projected_feature_text(ctx, name.as_str())?)
                .ok_or_else(|| CodecError::malformed("blank SLDPRT evaluated parameter name"))?;
            ctx.reserve_collection_vec(&mut replacements, 1, "collect SLDPRT evaluated parameter replacements")?;
            replacements.push((name, value));
        }
        for (name, value) in replacements { feature.parameters.insert(name, value); }
    }
    Ok(())
}

pub(crate) fn parse_native_parameter_literal(
    feature: &Feature,
    name: &str,
    expression: &str,
) -> Option<ParameterValue> {
    if native_parameter_is_length(feature, name, Some(expression)) {
        return parse_positive_dimension_length_mm(expression)
            .map(Length::from)
            .map(ParameterValue::Length);
    }
    parse_parameter_literal(expression)
}

pub(super) fn native_parameter_is_length(
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

fn copy_parameter_id(ctx: &DecodeContext<'_>, id: &ParameterId) -> Result<ParameterId, CodecError> {
    let copy_work = cadmpeg_core::decode::u64_from_index(id.as_str().len()).checked_mul(4)
        .ok_or_else(|| ctx.refuse_codec_limit("retain SLDPRT parameter reference", u64::MAX - 1, u64::MAX))?;
    ctx.charge_work(copy_work, "retain SLDPRT parameter reference")?;
    ParameterId::mint(crate::text_admission::format_retained(ctx, format_args!("{id}"), "retain SLDPRT parameter reference")?)
        .map_err(CodecError::malformed)
}

fn copy_parameter_value(ctx: &DecodeContext<'_>, value: &ParameterValue) -> Result<ParameterValue, CodecError> {
    match value {
        ParameterValue::String(value) => {
            let work = cadmpeg_core::decode::u64_from_index(value.len()).checked_mul(4)
                .ok_or_else(|| ctx.refuse_codec_limit("retain SLDPRT parameter value text", u64::MAX - 1, u64::MAX))?;
            ctx.charge_work(work, "retain SLDPRT parameter value text")?;
            Ok(ParameterValue::String(crate::text_admission::format_retained(ctx, 
                format_args!("{value}"), "retain SLDPRT parameter value text",
            )?))
        },
        _ => Ok(value.clone()),
    }
}

fn project_parameter_dependencies(
    ctx: &DecodeContext<'_>, parameter: &DesignParameter, aliases: ParameterAliasView<'_>,
) -> Result<cadmpeg_ir::features::DistinctMembers<ParameterId>, CodecError> {
    const OPERATION: &str = "collect SLDPRT parameter dependencies";
    let mut dependencies = cadmpeg_ir::features::DistinctMembers::default();
    let Some(tokens) = expression_identifier_tokens(ctx, &parameter.expression)? else { return Ok(dependencies); };
    for token in tokens.iter().filter(|token| !token.is_syntax()) {
        ctx.charge_work(1, OPERATION)?;
        let Some(dependency) = aliases.get(token.value()).and_then(Option::as_ref) else { continue; };
        ctx.charge_work(dependencies.as_slice().len() as u64, OPERATION)?;
        if dependency == &parameter.id || dependencies.contains(dependency) { continue; }
        ctx.charge_work(dependencies.as_slice().len() as u64, OPERATION)?;
        dependencies.try_insert_charged(copy_parameter_id(ctx, dependency)?, ctx, OPERATION)?;
    }
    Ok(dependencies)
}

fn populate_parameter_dependencies(
    ctx: &DecodeContext<'_>, parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>, global_owners: &HashSet<FeatureId>,
) -> Result<(), CodecError> {
    let aliases = ParameterAliases::new(ctx, parameters, feature_names, global_owners)?;
    for parameter in parameters.iter_mut() {
        parameter.dependencies = project_parameter_dependencies(ctx, parameter,
            aliases.for_owner(parameter.owner.as_ref()))?;
    }
    Ok(())
}

fn order_parameters_by_dependencies(ctx: &DecodeContext<'_>, parameters: &mut [DesignParameter]) -> Result<(), CodecError> {
    const OPERATION: &str = "order SLDPRT parameter dependencies";
    let mut owner_order = Vec::new();
    let mut seen_owners = HashSet::new();
    let mut parameter_owners = HashMap::new();
    for parameter in parameters.iter() {
        ctx.charge_work(1, OPERATION)?;
        let owner = parameter.owner.as_ref();
        if !seen_owners.contains(&owner) {
            ctx.charge_collection_items(1, OPERATION)?;
            seen_owners.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
            seen_owners.insert(owner);
            ctx.reserve_collection_vec(&mut owner_order, 1, OPERATION)?;
            owner_order.push(owner);
        }
        if !parameter_owners.contains_key(&parameter.id) {
            ctx.charge_collection_items(1, OPERATION)?;
            parameter_owners.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        }
        parameter_owners.insert(&parameter.id, owner);
    }
    let mut updates = Vec::new();
    for owner in owner_order {
        let mut remaining = Vec::new();
        for (index, parameter) in parameters.iter().enumerate() {
            ctx.charge_work(1, OPERATION)?;
            if parameter.owner.as_ref() == owner {
                ctx.reserve_collection_vec(&mut remaining, 1, OPERATION)?;
                remaining.push(index);
            }
        }
        let mut ordered = Vec::new();
        let mut ordered_ids = HashSet::new();
        while !remaining.is_empty() {
            let mut next = None;
            for (position, index) in remaining.iter().enumerate() {
                ctx.charge_work(1, OPERATION)?;
                ctx.charge_work(parameters[*index].dependencies.as_slice().len() as u64, OPERATION)?;
                if parameters[*index].dependencies.iter().all(|dependency| {
                    parameter_owners.get(dependency)
                        .is_none_or(|dependency_owner| dependency_owner != &owner)
                        || ordered_ids.contains(dependency)
                }) {
                    next = Some(position);
                    break;
                }
            }
            let Some(position) = next else { ordered.clear(); break; };
            ctx.charge_work(remaining.len() as u64, OPERATION)?;
            let index = remaining.remove(position);
            let id = &parameters[index].id;
            if !ordered_ids.contains(id) {
                ctx.charge_collection_items(1, OPERATION)?;
                ordered_ids.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
                ordered_ids.insert(id);
            }
            ctx.reserve_collection_vec(&mut ordered, 1, OPERATION)?;
            ordered.push(index);
        }
        for (ordinal, index) in ordered.into_iter().enumerate() {
            let ordinal = u32::try_from(ordinal).map_err(|_| ctx.refuse_codec_limit(
                "index SLDPRT ordered parameter ordinal", u64::from(u32::MAX), ordinal as u64,
            ))?;
            ctx.reserve_collection_vec(&mut updates, 1, OPERATION)?;
            updates.push((index, ordinal));
        }
    }
    for (index, ordinal) in updates { parameters[index].ordinal = ordinal; }
    Ok(())
}

#[cfg(test)]
fn parameter_aliases(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    expression_owner: Option<&FeatureId>,
) -> HashMap<String, Option<ParameterId>> {
    ParameterAliases::new(&cadmpeg_test_support::service_decode_context(), parameters, feature_names, global_owners).unwrap().materialize(expression_owner)
}

fn insert_parameter_alias(
    ctx: &DecodeContext<'_>, aliases: &mut HashMap<String, Option<ParameterId>>,
    alias: String, parameter: &ParameterId,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT parameter aliases";
    ctx.charge_work(1, OPERATION)?;
    if let Some(candidate) = aliases.get_mut(&alias) {
        if candidate.as_ref().is_some_and(|existing| existing != parameter) { *candidate = None; }
    } else {
        let parameter = copy_parameter_id(ctx, parameter)?;
        ctx.charge_collection_items(1, OPERATION)?;
        aliases.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
        aliases.insert(alias, Some(parameter));
    }
    Ok(())
}

struct ParameterAliases {
    global: HashMap<String, Option<ParameterId>>,
    exact: HashMap<String, Option<ParameterId>>,
    document_local: HashMap<String, Option<ParameterId>>,
    feature_local: HashMap<FeatureId, HashMap<String, Option<ParameterId>>>,
}

impl ParameterAliases {
    pub(super) fn new(
        ctx: &DecodeContext<'_>, parameters: &[DesignParameter],
        feature_names: &HashMap<FeatureId, String>, global_owners: &HashSet<FeatureId>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "retain SLDPRT parameter alias";
        let copy = |value: &str| crate::text_admission::format_retained(ctx, format_args!("{value}"), OPERATION);
        let mut aliases = Self {
            global: HashMap::new(), exact: HashMap::new(), document_local: HashMap::new(), feature_local: HashMap::new(),
        };
        for parameter in parameters {
            ctx.charge_work(1, "scan SLDPRT parameter aliases")?;
            insert_parameter_alias(ctx, &mut aliases.exact, copy(parameter.id.as_str())?, &parameter.id)?;
            let unqualified = [Some(parameter.name.as_str()), parameter.properties.get("EquationId")
                .filter(|equation_id| !equation_id.contains('@')).map(String::as_str)];
            if let Some(owner_name) = parameter.owner.as_ref().and_then(|owner| feature_names.get(owner)) {
                let qualified = crate::text_admission::format_retained(ctx, format_args!("{}@{owner_name}", parameter.name), OPERATION)?;
                insert_parameter_alias(ctx, &mut aliases.exact, qualified, &parameter.id)?;
                if let Some(equation_id) = parameter.properties.get("EquationId") {
                    let qualified = if equation_id.contains('@') { copy(equation_id)? }
                        else { crate::text_admission::format_retained(ctx, format_args!("{equation_id}@{owner_name}"), OPERATION)? };
                    insert_parameter_alias(ctx, &mut aliases.exact, qualified, &parameter.id)?;
                }
            }
            if parameter.owner.as_ref().is_some_and(|owner| global_owners.contains(owner)) {
                for alias in unqualified.into_iter().flatten() {
                    insert_parameter_alias(ctx, &mut aliases.global, copy(alias)?, &parameter.id)?;
                }
            }
            let local = if let Some(owner) = parameter.owner.as_ref() {
                if !aliases.feature_local.contains_key(owner) {
                    let id = copy_projected_feature_id(ctx, owner)?;
                    ctx.charge_collection_items(1, "index SLDPRT local parameter alias owners")?;
                    aliases.feature_local.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(
                        "index SLDPRT local parameter alias owners", u64::MAX - 1, u64::MAX,
                    ))?;
                    aliases.feature_local.insert(id, HashMap::new());
                }
                aliases.feature_local.get_mut(owner).ok_or_else(|| CodecError::malformed("missing SLDPRT local parameter alias owner"))?
            } else { &mut aliases.document_local };
            for alias in unqualified.into_iter().flatten() {
                insert_parameter_alias(ctx, local, copy(alias)?, &parameter.id)?;
            }
        }
        Ok(aliases)
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
    ctx: &DecodeContext<'_>, parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>, global_owners: &HashSet<FeatureId>,
) -> Result<(), CodecError> {
    let aliases = ParameterAliases::new(ctx, parameters, feature_names, global_owners)?;
    let mut values = HashMap::new();
    for parameter in parameters.iter() {
        if let Some(value) = &parameter.value { insert_parameter_value(ctx, &mut values, &parameter.id, value)?; }
    }
    loop {
        let mut changed = false;
        for parameter in parameters.iter_mut().filter(|parameter| parameter.value.is_none()) {
            ctx.charge_work(1, "evaluate SLDPRT parameter expressions")?;
            let aliases = aliases.for_owner(parameter.owner.as_ref());
            let Some(value) = ParameterExpressionParser::new(ctx, &parameter.expression, aliases, &values).parse()? else { continue; };
            insert_parameter_value(ctx, &mut values, &parameter.id, &value)?;
            parameter.value = Some(value);
            changed = true;
        }
        if !changed { break; }
    }
    Ok(())
}

fn insert_parameter_value(
    ctx: &DecodeContext<'_>, values: &mut HashMap<ParameterId, ParameterValue>, id: &ParameterId, value: &ParameterValue,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT parameter values";
    ctx.charge_work(1, OPERATION)?;
    let id = copy_parameter_id(ctx, id)?;
    let value = copy_parameter_value(ctx, value)?;
    if !values.contains_key(&id) {
        ctx.charge_collection_items(1, OPERATION)?;
        values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit(OPERATION, u64::MAX - 1, u64::MAX))?;
    }
    values.insert(id, value);
    Ok(())
}

pub(crate) fn parameters_with_unresolved_references(
    ctx: &DecodeContext<'_>, parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>, global_owners: &HashSet<FeatureId>,
) -> Result<usize, CodecError> {
    let aliases = ParameterAliases::new(ctx, parameters, feature_names, global_owners)?;
    let mut count = 0;
    for parameter in parameters {
        ctx.charge_work(1, "check SLDPRT parameter references")?;
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let unresolved = match expression_identifier_tokens(ctx, &parameter.expression)? {
            None => true,
            Some(parsed) => parsed.iter().filter(|identifier| !identifier.is_syntax())
                .filter(|identifier| definite_parameter_reference(identifier))
                .any(|identifier| aliases.get(identifier.value()).and_then(Option::as_ref)
                    .is_none_or(|dependency| dependency == &parameter.id)),
        };
        if unresolved { count += 1; }
    }
    Ok(count)
}

pub(crate) fn parameters_with_unevaluable_expressions(
    ctx: &DecodeContext<'_>, parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>, global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> Result<usize, CodecError> {
    let aliases = ParameterAliases::new(ctx, parameters, feature_names, global_owners)?;
    let mut states = parameter_value_states(ctx, parameters, configurations, false)?;
    let mut count = 0;
    for parameter in parameters {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        for values in &mut states {
            ctx.charge_work(1, "check SLDPRT parameter evaluation")?;
            let own = values.remove_entry(&parameter.id);
            let evaluated = match ParameterExpressionParser::new(ctx, &parameter.expression, aliases, values).parse()? {
                Some(value) => Some(value),
                None => text_parameter_literal(ctx, &parameter.name, &parameter.expression)?,
            };
            if let Some((id, value)) = own {
                values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("restore SLDPRT parameter evaluation value", u64::MAX - 1, u64::MAX))?;
                values.insert(id, value);
            }
            if evaluated.is_none() { count += 1; break; }
        }
    }
    Ok(count)
}

pub(crate) fn parameters_with_incoherent_dependencies(
    ctx: &DecodeContext<'_>, parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>, global_owners: &HashSet<FeatureId>,
) -> Result<usize, CodecError> {
    let aliases = ParameterAliases::new(ctx, parameters, feature_names, global_owners)?;
    let mut count = 0;
    for parameter in parameters {
        if parameter.dependencies != project_parameter_dependencies(ctx, parameter, aliases.for_owner(parameter.owner.as_ref()))? { count += 1; }
    }
    Ok(count)
}

pub(crate) fn parameters_with_incoherent_evaluated_values(
    ctx: &DecodeContext<'_>, parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>, global_owners: &HashSet<FeatureId>,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> Result<usize, CodecError> {
    let aliases = ParameterAliases::new(ctx, parameters, feature_names, global_owners)?;
    let mut states = parameter_value_states(ctx, parameters, configurations, true)?;
    let mut count = 0;
    for parameter in parameters.iter().filter(|parameter| !parameter.dependencies.is_empty()) {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        for values in &mut states {
            ctx.charge_work(1, "check SLDPRT evaluated parameter coherence")?;
            let own = values.remove_entry(&parameter.id);
            let evaluated = ParameterExpressionParser::new(ctx, &parameter.expression, aliases, values).parse()?;
            let incoherent = own.as_ref().zip(evaluated.as_ref())
                .is_some_and(|((_, actual), evaluated)| !equivalent_parameter_values(actual, evaluated));
            if let Some((id, value)) = own {
                values.try_reserve(1).map_err(|_| ctx.refuse_codec_limit("restore SLDPRT parameter coherence value", u64::MAX - 1, u64::MAX))?;
                values.insert(id, value);
            }
            if incoherent { count += 1; break; }
        }
    }
    Ok(count)
}

fn parameter_value_states(
    ctx: &DecodeContext<'_>, parameters: &[DesignParameter],
    configurations: &[cadmpeg_ir::features::DesignConfiguration], include_global: bool,
) -> Result<Vec<HashMap<ParameterId, ParameterValue>>, CodecError> {
    let make_state = |configuration: Option<&cadmpeg_ir::features::DesignConfiguration>| -> Result<_, CodecError> {
        let mut values = HashMap::new();
        for parameter in parameters {
            ctx.charge_work(1, "collect SLDPRT parameter value state")?;
            if let Some(value) = &parameter.value { insert_parameter_value(ctx, &mut values, &parameter.id, value)?; }
        }
        if let Some(configuration) = configuration {
            for (id, value) in &configuration.parameter_values { insert_parameter_value(ctx, &mut values, id, value)?; }
        }
        Ok(values)
    };
    let mut states = Vec::new();
    if include_global || configurations.is_empty() {
        let state = make_state(None)?;
        ctx.reserve_collection_vec(&mut states, 1, "collect SLDPRT parameter value states")?;
        states.push(state);
    }
    for configuration in configurations {
        let state = make_state(Some(configuration))?;
        ctx.reserve_collection_vec(&mut states, 1, "collect SLDPRT parameter value states")?;
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

pub(super) fn definite_parameter_reference(identifier: &ExpressionIdentifier<'_, '_>) -> bool {
    identifier.is_quoted()
        || identifier.value().contains('@')
        || identifier.value().strip_prefix('D').is_some_and(|ordinal| {
            !ordinal.is_empty() && ordinal.bytes().all(|byte| byte.is_ascii_digit())
        })
}

#[cfg(test)]
fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> {
    let ctx = cadmpeg_test_support::service_decode_context();
    expression_identifier_tokens(&ctx, expression).unwrap().unwrap_or_default().into_iter()
        .filter(|token| !token.is_syntax()).map(|token| token.value().to_owned()).collect::<Vec<_>>().into_iter()
}

enum ParameterTokenText<'a, 'ctx> {
    Borrowed(&'a str),
    Owned { value: String, _reservation: ScopedReservation<'ctx> },
}

impl ParameterTokenText<'_, '_> {
    fn as_str(&self) -> &str {
        match self { Self::Borrowed(value) => value, Self::Owned { value, .. } => value }
    }
}

/// One identifier token of the expression it borrows from.
pub(super) struct ExpressionIdentifier<'a, 'ctx> {
    raw: &'a str,
    following: &'a str,
    value: ParameterTokenText<'a, 'ctx>,
    quoted: bool,
}

impl<'a, 'ctx> ExpressionIdentifier<'a, 'ctx> {
    /// The token spanning `start..end`, which must be a quoted run around a nonempty name.
    fn quoted(ctx: &'ctx DecodeContext<'_>, expression: &'a str, start: usize, end: usize) -> Result<Option<Self>, CodecError> {
        let Some(raw) = expression.get(start..end) else { return Ok(None); };
        let Some(following) = expression.get(end..) else { return Ok(None); };
        let Some(inner) = raw.strip_prefix('"').and_then(|inner| inner.strip_suffix('"')).filter(|inner| !inner.is_empty()) else { return Ok(None); };
        ctx.charge_work(inner.len() as u64, "unescape SLDPRT parameter identifier")?;
        let value = if inner.contains("\"\"") {
            let (mut value, reservation) = crate::text_admission::reserve_scoped_string(ctx, inner.len(), "unescape SLDPRT parameter identifier")?;
            let mut segments = inner.split("\"\"");
            if let Some(first) = segments.next() { value.push_str(first); }
            for segment in segments { value.push('"'); value.push_str(segment); }
            ParameterTokenText::Owned { value, _reservation: reservation }
        } else { ParameterTokenText::Borrowed(inner) };
        Ok(Some(Self { raw, following, value, quoted: true }))
    }

    /// The unquoted token spanning `start..end`.
    fn plain(expression: &'a str, start: usize, end: usize) -> Option<Self> {
        let raw = expression.get(start..end)?;
        Some(Self {
            raw,
            following: expression.get(end..)?,
            value: ParameterTokenText::Borrowed(raw),
            quoted: false,
        })
    }

    /// The identifier text, with the quotes and doubled quotes resolved.
    pub(super) fn value(&self) -> &str {
        self.value.as_str()
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

    /// The expression text that follows this token.
    pub(super) fn following(&self) -> &'a str {
        self.following
    }

    /// The text of `tail` that precedes this token, where `tail` is the not-yet-consumed
    /// remainder of the expression this token was cut from.
    pub(super) fn preceding(&self, tail: &'a str) -> Option<&'a str> {
        tail.strip_suffix(self.following)
            .and_then(|head| head.strip_suffix(self.raw))
    }
}

pub(super) fn expression_identifier_tokens<'a, 'ctx>(
    ctx: &'ctx DecodeContext<'_>, expression: &'a str,
) -> Result<Option<Vec<ExpressionIdentifier<'a, 'ctx>>>, CodecError> {
    ctx.charge_work(expression.len() as u64, "scan SLDPRT parameter identifiers")?;
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
                if let Some(identifier) = ExpressionIdentifier::quoted(ctx, expression, at, cursor)? {
                    ctx.reserve_collection_vec(&mut identifiers, 1, "collect SLDPRT parameter identifiers")?;
                    identifiers.push(identifier);
                }
                at = cursor;
                continue;
            }
            return Ok(None);
        }

        if character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.') {
            let end = rest
                .find(|candidate: char| {
                    !(candidate.is_ascii_alphanumeric()
                        || matches!(candidate, '_' | '@' | '$' | '.'))
                })
                .unwrap_or(rest.len());
            if let Some(identifier) = ExpressionIdentifier::plain(expression, at, at + end) {
                ctx.reserve_collection_vec(&mut identifiers, 1, "collect SLDPRT parameter identifiers")?;
                identifiers.push(identifier);
            }
            at += end;
        } else {
            at += character.len_utf8();
        }
    }
    Ok(Some(identifiers))
}
