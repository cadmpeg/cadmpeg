// SPDX-License-Identifier: Apache-2.0
//! Native Keywords parameter projection and equation evaluation.

use crate::classification::{classify, FeatureClass, NativeClassKind};
use crate::records::{Feature, FeatureHistory};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{
    features::{
        DesignParameter, DimensionDisplay, FeatureDefinition, FeatureId, FeatureOperation,
        FeatureTreeNodeRole, ParameterId, ParameterValue,
    },
    scalar::{Angle, FiniteReal, Length},
};
use std::collections::{HashMap, HashSet};

use crate::history::classify::{
    feature_family, feature_input_class, is_chamfer, is_extrude, is_fillet, is_offset_plane,
    HistoryIndex, EQUATION_DRIVEN_TOKEN,
};
use crate::history::literals::{
    admit_literal, dimension_display, parse_angle_rad, parse_dimension_display_length,
    parse_parameter_literal, parse_positive_dimension_length_mm, FiniteLiteral, LengthLiteral,
    ParameterLiteral,
};
use crate::history::project::pattern::{native_pattern_form, NativePatternClass};
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

pub(crate) fn project_parameters(
    ctx: &DecodeContext<'_>,
    histories: &[FeatureHistory],
) -> Result<Vec<DesignParameter>, CodecError> {
    const OPERATION: &str = "scan SLDPRT project_parameters values";
    let mut scratch = ctx.reserve_scoped(0, "index SLDPRT parameter owner names")?;
    let mut feature_names = HashMap::new();
    let mut global_owners = HashSet::new();
    let mut parameters = Vec::new();
    for history in ctx.admit_iter(histories, OPERATION)? {
        let index = HistoryIndex::new(ctx, &history.features)?;
        for feature in ctx.admit_iter(&history.features, OPERATION)? {
            if index.is_metadata(ctx, feature)? {
                continue;
            }
            let owner = scratch.with_storage(|| neutral_feature_id_charged(ctx, &feature.id))?;
            if !feature.name.is_empty() {
                scratch.with_storage(|| {
                    let id = copy_projected_feature_id(ctx, &owner)?;
                    let name = copy_projected_feature_text(ctx, &feature.name)?;
                    ctx.insert_hash_map(
                        &mut feature_names,
                        id,
                        name,
                        "index SLDPRT parameter owner names",
                    )
                })?;
            }
            if feature.kind.eq_ignore_ascii_case("EquationDriven") {
                scratch.with_storage(|| {
                    let id = copy_projected_feature_id(ctx, &owner)?;
                    ctx.insert_hash_set(
                        &mut global_owners,
                        id,
                        "index SLDPRT global parameter owners",
                    )
                })?;
            }
            for (ordinal, name) in ctx
                .admit_iter(projected_parameter_names(ctx, feature)?, OPERATION)?
                .enumerate()
            {
                let expression = ctx
                    .get_btree_map(
                        &feature.parameters,
                        name.as_str(),
                        "look up SLDPRT ordered key",
                    )?
                    .ok_or_else(|| CodecError::malformed("missing SLDPRT projected parameter"))?;
                admit_literal(ctx, expression, "parse SLDPRT parameter display")?;
                let display = dimension_display(expression);
                let properties = ctx
                    .get_btree_map(
                        &feature.dimension_properties,
                        &name,
                        "look up SLDPRT ordered key",
                    )?
                    .map(|properties| {
                        copy_projected_feature_properties(
                            ctx,
                            properties,
                            "collect SLDPRT projected parameter properties",
                        )
                    })
                    .transpose()?
                    .unwrap_or_default();
                let parse_value = |value: &str| -> Result<_, CodecError> {
                    Ok(match display {
                        Some(DimensionDisplay::Diameter | DimensionDisplay::Radius) => {
                            admit_literal(ctx, value, "parse SLDPRT parameter value")?;
                            parse_dimension_display_length(value).map(ParameterValue::Length)
                        }
                        None => parse_native_parameter_literal(ctx, feature, &name, value)?,
                    })
                };
                let stated =
                    match ctx.get_btree_map(&properties, "Value", "look up SLDPRT ordered key")? {
                        Some(value) => parse_value(value)?,
                        None => None,
                    };
                let value = match stated {
                    Some(value) => Some(value),
                    None => parse_value(expression)?,
                };
                let ordinal_u32 = u32::try_from(ordinal).map_err(|_| {
                    ctx.refuse_codec_limit(
                        "index SLDPRT parameter ordinal",
                        u64::from(u32::MAX),
                        cadmpeg_core::decode::u64_from_index(ordinal),
                    )
                })?;
                let parameter = DesignParameter {
                    id: neutral_parameter_id(ctx, feature, ordinal)?,
                    owner: Some(copy_projected_feature_id(ctx, &owner)?),
                    ordinal: ordinal_u32,
                    properties,
                    name,
                    expression: ctx
                        .copy_retained_text(expression, "retain SLDPRT parameter expression")?,
                    display,
                    value,
                    dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                    native_ref: None,
                    pmi: None,
                };
                ctx.push_vec(
                    &mut parameters,
                    parameter,
                    "collect SLDPRT projected parameters",
                )?;
            }
        }
    }
    populate_parameter_dependencies(ctx, &mut parameters, &feature_names, &global_owners)?;
    order_parameters_by_dependencies(ctx, &mut parameters)?;
    evaluate_parameter_expressions(ctx, &mut parameters, &feature_names, &global_owners)?;
    for parameter in ctx.admit_iter(&mut parameters, "scan SLDPRT text parameter literals")? {
        if parameter.value.is_none() {
            parameter.value = text_parameter_literal(ctx, &parameter.name, &parameter.expression)?;
        }
    }
    Ok(parameters)
}

fn text_parameter_literal(
    ctx: &DecodeContext<'_>,
    name: &str,
    expression: &str,
) -> Result<Option<ParameterValue>, CodecError> {
    match bare_text_parameter_literal(ctx, expression)? {
        Some(value) => Ok(Some(value)),
        None => formatted_text_dimension_literal(ctx, name, expression),
    }
}

fn bare_text_parameter_literal(
    ctx: &DecodeContext<'_>,
    expression: &str,
) -> Result<Option<ParameterValue>, CodecError> {
    const OPERATION: &str = "parse SLDPRT text parameter literal";
    let expression = ctx.trim_text(expression, OPERATION)?;
    if expression.is_empty()
        || ctx.any_by(
            expression.chars(),
            |character| {
                Ok(matches!(
                    character,
                    '+' | '-' | '*' | '/' | '^' | '=' | '<' | '>' | '(' | ')' | ','
                ))
            },
            OPERATION,
        )?
    {
        return Ok(None);
    }
    let (identifiers, _identifier_storage) = expression_identifier_tokens(ctx, expression)?;
    let Some(identifiers) = identifiers else {
        return Ok(None);
    };
    if ctx.any_by(
        &identifiers,
        |identifier| definite_parameter_reference(ctx, identifier),
        "scan SLDPRT text parameter identifiers",
    )? {
        return Ok(None);
    }
    Ok(Some(ParameterValue::String(ctx.copy_retained_text(
        expression,
        "retain SLDPRT text parameter literal",
    )?)))
}

fn formatted_text_dimension_literal(
    ctx: &DecodeContext<'_>,
    name: &str,
    expression: &str,
) -> Result<Option<ParameterValue>, CodecError> {
    const NAME: &str = "parse SLDPRT formatted parameter name";
    const LITERAL: &str = "parse SLDPRT formatted parameter literal";
    let Some(suffix) = ctx.strip_prefix(name, "TXD", NAME)? else {
        return Ok(None);
    };
    if suffix.is_empty() || !ctx.all_by(suffix.bytes(), |byte| Ok(byte.is_ascii_digit()), NAME)? {
        return Ok(None);
    }
    let expression = ctx.trim_text(expression, LITERAL)?;
    let mut in_tag = false;
    let mut nonblank = false;
    let mut tags = 0;
    let mut characters = expression.chars();
    while let Some(character) = ctx.next_charged(&mut characters, LITERAL)? {
        match character {
            '<' if in_tag => return Ok(None),
            '<' => {
                in_tag = true;
                nonblank = false;
            }
            '>' if !in_tag || !nonblank => return Ok(None),
            '>' => {
                in_tag = false;
                tags += 1;
            }
            character if in_tag => nonblank |= !character.is_whitespace(),
            _ => {}
        }
    }
    if in_tag || tags == 0 {
        return Ok(None);
    }
    Ok(Some(ParameterValue::String(ctx.copy_retained_text(
        expression,
        "retain SLDPRT formatted parameter literal",
    )?)))
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
    const OPERATION: &str = "find SLDPRT evaluated parameter replacement";
    let (evaluated, _evaluated_storage) =
        ctx.with_scoped_storage(OPERATION, || project_parameters(ctx, histories))?;
    // The last evaluated value of each owner's named parameter.
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut values = HashMap::new();
    for parameter in ctx.admit_iter(&evaluated, OPERATION)? {
        let (Some(owner), Some(value)) = (&parameter.owner, &parameter.value) else {
            continue;
        };
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut values,
                (owner, parameter.name.as_str()),
                value,
                OPERATION,
            )
        })?;
    }
    for history in ctx.admit_iter(histories, "scan SLDPRT evaluated parameter replacements")? {
        for feature in ctx.admit_iter(
            &mut history.features,
            "scan SLDPRT evaluated parameter replacements",
        )? {
            let (owner, _owner_storage) = ctx
                .with_scoped_storage(OPERATION, || neutral_feature_id_charged(ctx, &feature.id))?;
            let mut replacements = Vec::new();
            let mut replacement_storage = ctx.reserve_scoped(0, OPERATION)?;
            for (name, expression) in ctx.admit_iter(
                &feature.parameters,
                "scan SLDPRT evaluated parameter replacements",
            )? {
                if parse_native_parameter_literal(ctx, feature, name.as_str(), expression)?
                    .is_some()
                {
                    continue;
                }
                let Some(value) = ctx.get_hash_map(&values, &(&owner, name.as_str()), OPERATION)?
                else {
                    continue;
                };
                let value = match value {
                    ParameterValue::String(value) => {
                        ctx.copy_retained_text(value, "retain SLDPRT evaluated parameter text")?
                    }
                    _ => ctx.format_retained(
                        format_args!("{}", ParameterLiteral(value)),
                        "retain SLDPRT evaluated parameter text",
                    )?,
                };
                let name = replacement_storage
                    .with_storage(|| ctx.copy_retained_text(name.as_str(), OPERATION))?;
                ctx.push_scoped_vec(
                    &mut replacement_storage,
                    &mut replacements,
                    (name, value),
                    "collect SLDPRT evaluated parameter replacements",
                )?;
            }
            for (name, value) in ctx.admit_iter(replacements, OPERATION)? {
                if let Some(expression) =
                    ctx.get_mut_btree_map(&mut feature.parameters, name.as_str(), OPERATION)?
                {
                    *expression = value;
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_native_parameter_literal(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &Feature,
    name: &str,
    expression: &str,
) -> Result<Option<ParameterValue>, cadmpeg_core::CodecError> {
    let is_length = native_parameter_is_length(ctx, feature, name, Some(expression))?;
    admit_literal(ctx, expression, "parse SLDPRT native parameter literal")?;
    if is_length {
        return Ok(parse_positive_dimension_length_mm(expression)
            .map(Length::from)
            .map(ParameterValue::Length));
    }
    Ok(parse_parameter_literal(expression))
}

pub(super) fn native_parameter_is_length(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &Feature,
    name: &str,
    expression: Option<&str>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let cosmetic_thread = classify(feature) == Some(FeatureClass::CosmeticThread);
    Ok(match name {
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
                    && ctx
                        .get_btree_map(&feature.properties, "Mode", "test SLDPRT map key")?
                        .is_some_and(|mode| {
                            mode.eq_ignore_ascii_case("Offset")
                                || mode.eq_ignore_ascii_case("Translate")
                        }))
                || is_offset_plane(ctx, feature)?
                || cosmetic_thread
        }
        "D2" if cosmetic_thread => true,
        "D2" if is_chamfer(feature) => match expression {
            Some(value) => {
                admit_literal(ctx, value, "parse SLDPRT chamfer angle")?;
                parse_angle_rad(value).is_none()
            }
            None => true,
        },
        "D3" if matches!(
            native_pattern_form(ctx, feature)?,
            Some(NativePatternClass::Linear | NativePatternClass::CurveDriven)
        ) =>
        {
            true
        }
        _ => {
            is_extrude(feature)
                && matches!(
                    ctx.get_btree_map(&feature.properties, "EndCondition", "test SLDPRT map key")?
                        .map(String::as_str),
                    Some("Blind" | "Symmetric")
                )
                && feature.parameters.len() == 1
                && ctx.contains_key_btree_map(&(feature.parameters), name, "test SLDPRT map key")?
        }
    })
}

/// The expression of a native scalar `value` in metres or radians, in the
/// display form of `expression`, or `None` when the value in its unit is not
/// finite.
pub(crate) fn format_native_scalar(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &Feature,
    name: &str,
    value: f64,
    expression: Option<&str>,
) -> Result<Option<String>, cadmpeg_core::CodecError> {
    Ok(
        if let Some(display) = expression
            .map(|value| {
                admit_literal(ctx, value, "parse SLDPRT parameter display")?;
                Ok::<_, CodecError>(dimension_display(value))
            })
            .transpose()?
            .flatten()
        {
            let prefix = match display {
                DimensionDisplay::Diameter => expression
                    .filter(|value| value.trim().starts_with("&lt;MOD-DIAM&gt;"))
                    .map_or("<MOD-DIAM>", |_| "&lt;MOD-DIAM&gt;"),
                DimensionDisplay::Radius => expression
                    .filter(|value| value.trim().starts_with("&lt;MOD-RHO&gt;"))
                    .map_or("<MOD-RHO>", |_| "&lt;MOD-RHO&gt;"),
            };
            match Length::new(value * 1000.0) {
                Some(length) => Some(ctx.format_retained(
                    format_args!("{prefix}{}", FiniteLiteral(length.get())),
                    "format SLDPRT native scalar",
                )?),
                None => return Ok(None),
            }
        } else if native_parameter_is_length(ctx, feature, name, expression)? {
            Length::new(value * 1000.0)
                .map(|length| {
                    ctx.format_retained(
                        format_args!("{}", LengthLiteral(length)),
                        "format SLDPRT native scalar",
                    )
                })
                .transpose()?
        } else if expression.and_then(parse_angle_rad).is_some() {
            Angle::new(value)
                .map(|angle| {
                    ctx.format_retained(
                        format_args!("{}rad", FiniteLiteral(angle.get())),
                        "format SLDPRT native scalar",
                    )
                })
                .transpose()?
        } else {
            FiniteReal::new(value)
                .map(|real| {
                    ctx.format_retained(
                        format_args!("{}", FiniteLiteral(real.get())),
                        "format SLDPRT native scalar",
                    )
                })
                .transpose()?
        },
    )
}

fn copy_parameter_id(ctx: &DecodeContext<'_>, id: &ParameterId) -> Result<ParameterId, CodecError> {
    id.try_clone_for_decode(ctx, "retain SLDPRT parameter reference")
}

fn project_parameter_dependencies(
    ctx: &DecodeContext<'_>,
    parameter: &DesignParameter,
    aliases: ParameterAliasView<'_>,
) -> Result<cadmpeg_ir::features::DistinctMembers<ParameterId>, CodecError> {
    const OPERATION: &str = "collect SLDPRT parameter dependencies";
    let (tokens, _token_storage) = expression_identifier_tokens(ctx, &parameter.expression)?;
    let Some(tokens) = tokens else {
        return Ok(cadmpeg_ir::features::DistinctMembers::default());
    };
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut seen = HashSet::new();
    let mut dependencies = Vec::new();
    for token in ctx.admit_iter(&tokens, OPERATION)? {
        if token.is_syntax(ctx)? {
            continue;
        }
        let Some(dependency) = aliases.get(ctx, token.value())?.and_then(Option::as_ref) else {
            continue;
        };
        if ctx.equal(dependency, &parameter.id, OPERATION)?
            || ctx.contains_hash_set(&seen, dependency, OPERATION)?
        {
            continue;
        }
        scratch.with_storage(|| ctx.insert_hash_set(&mut seen, dependency, OPERATION))?;
        ctx.push_vec(
            &mut dependencies,
            copy_parameter_id(ctx, dependency)?,
            OPERATION,
        )?;
    }
    Ok(cadmpeg_ir::features::DistinctMembers::try_from(
        dependencies,
        ctx,
    )?)
}

fn populate_parameter_dependencies(
    ctx: &DecodeContext<'_>,
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> Result<(), CodecError> {
    let (aliases, _aliases_storage) =
        ParameterAliases::scoped(ctx, parameters, feature_names, global_owners)?;
    for parameter in ctx.admit_iter(parameters, "collect SLDPRT parameter dependencies")? {
        parameter.dependencies = project_parameter_dependencies(
            ctx,
            parameter,
            aliases.for_owner(parameter.owner.as_ref()),
        )?;
    }
    Ok(())
}

/// Number each owner's parameters so that a parameter follows the parameters
/// of the same owner it depends on, keeping source order among ready
/// parameters. An owner whose dependencies form a cycle keeps its ordinals.
fn order_parameters_by_dependencies(
    ctx: &DecodeContext<'_>,
    parameters: &mut [DesignParameter],
) -> Result<(), CodecError> {
    const OPERATION: &str = "order SLDPRT parameter dependencies";
    let mut scratch = ctx.reserve_scoped(0, OPERATION)?;
    let mut owner_groups = HashMap::new();
    let mut group_sizes = Vec::new();
    let mut groups = Vec::new();
    let mut positions = HashMap::new();
    for (index, parameter) in ctx.admit_iter(&*parameters, OPERATION)?.enumerate() {
        let next = group_sizes.len();
        let group = scratch.with_storage(|| {
            ctx.entry_hash_map(&mut owner_groups, parameter.owner.as_ref(), OPERATION)
                .map(|entry| *entry.or_insert(next))
        })?;
        if group == next {
            ctx.push_scoped_vec(&mut scratch, &mut group_sizes, 0_usize, OPERATION)?;
        }
        group_sizes[group] += 1;
        ctx.push_scoped_vec(&mut scratch, &mut groups, group, OPERATION)?;
        scratch.with_storage(|| {
            ctx.insert_hash_map(&mut positions, &parameter.id, index, OPERATION)
        })?;
    }
    let (mut dependents, mut waiting) = scratch.with_storage(|| {
        Ok::<_, CodecError>((
            ctx.collect_indexed_vec(parameters.len(), OPERATION, |_| Ok(Vec::<usize>::new()))?,
            ctx.alloc_filled(parameters.len(), 0_usize, OPERATION)?,
        ))
    })?;
    for (index, parameter) in ctx.admit_iter(&*parameters, OPERATION)?.enumerate() {
        for dependency in ctx.admit_iter(
            parameter.dependencies.as_slice(),
            "scan SLDPRT order_parameters_by_dependencies values",
        )? {
            let Some(&source) = ctx.get_hash_map(&positions, dependency, OPERATION)? else {
                continue;
            };
            if groups[source] != groups[index] {
                continue;
            }
            scratch.with_storage(|| ctx.push_vec(&mut dependents[source], index, OPERATION))?;
            waiting[index] += 1;
        }
    }
    let mut ready = std::collections::BTreeSet::new();
    for (index, waiting) in ctx.admit_iter(&waiting, OPERATION)?.enumerate() {
        if *waiting == 0 {
            scratch.with_storage(|| ctx.insert_btree_set(&mut ready, index, OPERATION))?;
        }
    }
    let mut ordered =
        scratch.with_storage(|| ctx.alloc_filled(group_sizes.len(), 0_usize, OPERATION))?;
    let mut updates = Vec::new();
    while let Some(index) = ready.pop_first() {
        ctx.charge_work(1, OPERATION)?;
        let group = groups[index];
        let ordinal = u32::try_from(ordered[group]).map_err(|_| {
            ctx.refuse_codec_limit(
                "index SLDPRT ordered parameter ordinal",
                u64::from(u32::MAX),
                cadmpeg_core::decode::u64_from_index(ordered[group]),
            )
        })?;
        ordered[group] += 1;
        ctx.push_scoped_vec(&mut scratch, &mut updates, (index, ordinal), OPERATION)?;
        for &dependent in ctx.admit_iter(&dependents[index], OPERATION)? {
            waiting[dependent] -= 1;
            if waiting[dependent] == 0 {
                scratch.with_storage(|| ctx.insert_btree_set(&mut ready, dependent, OPERATION))?;
            }
        }
    }
    for (index, ordinal) in ctx.admit_iter(updates, "scan SLDPRT updates values")? {
        let group = groups[index];
        if ordered[group] == group_sizes[group] {
            parameters[index].ordinal = ordinal;
        }
    }
    Ok(())
}

#[cfg(test)]
fn parameter_aliases(
    parameters: &[DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
    expression_owner: Option<&FeatureId>,
) -> HashMap<String, Option<ParameterId>> {
    ParameterAliases::new(
        &cadmpeg_test_support::service_decode_context(),
        parameters,
        feature_names,
        global_owners,
    )
    .unwrap()
    .materialize(expression_owner)
}

fn insert_parameter_alias(
    ctx: &DecodeContext<'_>,
    aliases: &mut HashMap<String, Option<ParameterId>>,
    alias: String,
    parameter: &ParameterId,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT parameter aliases";
    if let Some(candidate) =
        ctx.get_mut_hash_map(&mut *aliases, &alias, "look up mutable SLDPRT hash key")?
    {
        if let Some(existing) = candidate.as_ref() {
            if !ctx.equal(existing, parameter, OPERATION)? {
                *candidate = None;
            }
        }
    } else {
        let parameter = copy_parameter_id(ctx, parameter)?;
        ctx.insert_hash_map(aliases, alias, Some(parameter), OPERATION)?;
    }
    Ok(())
}

/// Parameter aliases by scope, built once for a parameter set.
pub(crate) struct ParameterAliases {
    global: HashMap<String, Option<ParameterId>>,
    exact: HashMap<String, Option<ParameterId>>,
    document_local: HashMap<String, Option<ParameterId>>,
    feature_local: HashMap<FeatureId, HashMap<String, Option<ParameterId>>>,
}

impl ParameterAliases {
    /// Build the alias tables as scratch storage held by the returned reservation.
    pub(crate) fn scoped<'c>(
        ctx: &'c DecodeContext<'_>,
        parameters: &[DesignParameter],
        feature_names: &HashMap<FeatureId, String>,
        global_owners: &HashSet<FeatureId>,
    ) -> Result<(Self, ScopedReservation<'c>), CodecError> {
        ctx.with_scoped_storage("retain SLDPRT parameter alias", || {
            Self::new(ctx, parameters, feature_names, global_owners)
        })
    }

    pub(super) fn new(
        ctx: &DecodeContext<'_>,
        parameters: &[DesignParameter],
        feature_names: &HashMap<FeatureId, String>,
        global_owners: &HashSet<FeatureId>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "retain SLDPRT parameter alias";
        let copy = |value: &str| ctx.copy_retained_text(value, OPERATION);
        let mut aliases = Self {
            global: HashMap::new(),
            exact: HashMap::new(),
            document_local: HashMap::new(),
            feature_local: HashMap::new(),
        };
        for parameter in ctx.admit_iter(parameters, "scan SLDPRT parameter aliases")? {
            insert_parameter_alias(
                ctx,
                &mut aliases.exact,
                copy(parameter.id.as_str())?,
                &parameter.id,
            )?;
            let equation_id = ctx
                .get_btree_map(&parameter.properties, "EquationId", OPERATION)?
                .map(String::as_str);
            let qualified_equation_id = match equation_id {
                Some(equation_id) => ctx.contains_text(equation_id, "@", OPERATION)?,
                None => false,
            };
            let unqualified = [
                Some(parameter.name.as_str()),
                equation_id.filter(|_| !qualified_equation_id),
            ];
            let owner_name = match parameter.owner.as_ref() {
                Some(owner) => ctx.get_hash_map(feature_names, owner, "look up SLDPRT hash key")?,
                None => None,
            };
            if let Some(owner_name) = owner_name {
                let qualified = ctx
                    .format_retained(format_args!("{}@{owner_name}", parameter.name), OPERATION)?;
                insert_parameter_alias(ctx, &mut aliases.exact, qualified, &parameter.id)?;
                if let Some(equation_id) = equation_id {
                    let qualified = if qualified_equation_id {
                        copy(equation_id)?
                    } else {
                        ctx.format_retained(format_args!("{equation_id}@{owner_name}"), OPERATION)?
                    };
                    insert_parameter_alias(ctx, &mut aliases.exact, qualified, &parameter.id)?;
                }
            }
            if match parameter.owner.as_ref() {
                Some(owner) => {
                    ctx.contains_hash_set(global_owners, owner, "test SLDPRT hashed identity")?
                }
                None => false,
            } {
                for alias in unqualified.into_iter().flatten() {
                    insert_parameter_alias(ctx, &mut aliases.global, copy(alias)?, &parameter.id)?;
                }
            }
            let local = if let Some(owner) = parameter.owner.as_ref() {
                if !ctx.contains_key_hash_map(
                    &aliases.feature_local,
                    owner,
                    "test SLDPRT map key",
                )? {
                    let id = copy_projected_feature_id(ctx, owner)?;
                    ctx.insert_hash_map(
                        &mut aliases.feature_local,
                        id,
                        HashMap::new(),
                        "index SLDPRT local parameter alias owners",
                    )?;
                }
                ctx.get_mut_hash_map(
                    &mut aliases.feature_local,
                    owner,
                    "look up mutable SLDPRT hash key",
                )?
                .ok_or_else(|| {
                    CodecError::malformed("missing SLDPRT local parameter alias owner")
                })?
            } else {
                &mut aliases.document_local
            };
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

impl<'a> ParameterAliasView<'a> {
    pub(super) fn get(
        &self,
        ctx: &DecodeContext<'_>,
        alias: &str,
    ) -> Result<Option<&'a Option<ParameterId>>, CodecError> {
        if let Some(value) =
            ctx.get_hash_map(&self.aliases.exact, alias, "look up SLDPRT hash key")?
        {
            return Ok(Some(value));
        }
        let local = match self.owner {
            Some(owner) => ctx.get_hash_map(
                &self.aliases.feature_local,
                owner,
                "look up SLDPRT hash key",
            )?,
            None => None,
        }
        .unwrap_or(&self.aliases.document_local);
        if let Some(value) = ctx.get_hash_map(local, alias, "look up SLDPRT hash key")? {
            return Ok(Some(value));
        }
        ctx.get_hash_map(&self.aliases.global, alias, "look up SLDPRT hash key")
    }
}

fn evaluate_parameter_expressions(
    ctx: &DecodeContext<'_>,
    parameters: &mut [DesignParameter],
    feature_names: &HashMap<FeatureId, String>,
    global_owners: &HashSet<FeatureId>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "schedule SLDPRT parameter evaluation";
    if !ctx.any_by(
        &*parameters,
        |parameter| Ok(parameter.value.is_none()),
        "find SLDPRT unevaluated parameters",
    )? {
        return Ok(());
    }
    let (aliases, _aliases_storage) =
        ParameterAliases::scoped(ctx, parameters, feature_names, global_owners)?;
    let mut values_storage = ctx.reserve_scoped(0, "index SLDPRT parameter values")?;
    let mut values = HashMap::new();
    for (index, parameter) in ctx
        .admit_iter(&*parameters, "scan SLDPRT parameter values")?
        .enumerate()
    {
        if parameter.value.is_some() {
            values_storage.with_storage(|| {
                let id = copy_parameter_id(ctx, &parameter.id)?;
                ctx.insert_hash_map(&mut values, id, index, "index SLDPRT parameter values")
            })?;
        }
    }
    let mut graph_storage = ctx.reserve_scoped(0, OPERATION)?;
    let repeated = {
        let (identities, _identity_storage) = ctx.unique_index(
            parameters.iter().map(|parameter| (&parameter.id, ())),
            OPERATION,
        )?;
        ctx.any_by(
            &*parameters,
            |parameter| {
                Ok(ctx
                    .get_hash_map(&identities, &parameter.id, OPERATION)?
                    .and_then(Option::as_ref)
                    .is_none())
            },
            OPERATION,
        )?
    };
    if repeated {
        // Repeated identities share one value slot. Preserve source-pass replacement order.
        loop {
            let mut changed = false;
            let mut indexes = 0..parameters.len();
            while let Some(index) =
                ctx.next_charged(&mut indexes, "evaluate SLDPRT parameter expressions")?
            {
                let parameter = &parameters[index];
                if parameter.value.is_some() {
                    continue;
                }
                let aliases = aliases.for_owner(parameter.owner.as_ref());
                let Some(value) = ParameterExpressionParser::new(
                    ctx,
                    &parameter.expression,
                    aliases,
                    eval::ParameterValues::Indexed {
                        parameters,
                        positions: &values,
                    },
                )
                .parse()?
                else {
                    continue;
                };
                values_storage.with_storage(|| {
                    let id = copy_parameter_id(ctx, &parameter.id)?;
                    ctx.insert_hash_map(&mut values, id, index, "index SLDPRT parameter values")
                })?;
                parameters[index].value = Some(value);
                changed = true;
            }
            if !changed {
                break;
            }
        }
        return Ok(());
    }
    // Retry a blocked expression when its first missing value is published.
    let mut blocked = HashMap::<ParameterId, Vec<usize>>::new();
    let mut ready = Vec::new();
    let mut ready_storage = ctx.reserve_scoped(0, OPERATION)?;
    for (index, parameter) in ctx.admit_iter(&*parameters, OPERATION)?.enumerate() {
        if parameter.value.is_none() {
            ctx.push_scoped_vec(&mut ready_storage, &mut ready, index, OPERATION)?;
        }
    }
    while !ready.is_empty() {
        let current = std::mem::take(&mut ready);
        let current_storage =
            std::mem::replace(&mut ready_storage, ctx.reserve_scoped(0, OPERATION)?);
        for index in ctx.admit_iter(current, "evaluate SLDPRT parameter expressions")? {
            let parameter = &parameters[index];
            if parameter.value.is_some() {
                continue;
            }
            let aliases = aliases.for_owner(parameter.owner.as_ref());
            let evaluation = ParameterExpressionParser::new(
                ctx,
                &parameter.expression,
                aliases,
                eval::ParameterValues::Indexed {
                    parameters,
                    positions: &values,
                },
            )
            .evaluate(&mut graph_storage)?;
            let value = match evaluation {
                eval::ParameterEvaluation::Value(value) => value,
                eval::ParameterEvaluation::Invalid => continue,
                eval::ParameterEvaluation::Blocked(id) => {
                    graph_storage.with_storage(|| {
                        ctx.push_hash_group(&mut blocked, id, index, OPERATION, OPERATION)
                    })?;
                    continue;
                }
            };
            values_storage.with_storage(|| {
                let id = copy_parameter_id(ctx, &parameter.id)?;
                ctx.insert_hash_map(&mut values, id, index, "index SLDPRT parameter values")
            })?;
            parameters[index].value = Some(value);
            if let Some(dependents) =
                ctx.remove_hash_map(&mut blocked, &parameters[index].id, OPERATION)?
            {
                for dependent in ctx.admit_iter(dependents, OPERATION)? {
                    ctx.push_scoped_vec(&mut ready_storage, &mut ready, dependent, OPERATION)?;
                }
            }
        }
        drop(current_storage);
        ctx.sort_unstable_by(&mut ready, |index| index, Ord::cmp, OPERATION)?;
    }
    Ok(())
}

fn insert_parameter_value(
    ctx: &DecodeContext<'_>,
    values: &mut HashMap<ParameterId, ParameterValue>,
    id: &ParameterId,
    value: &ParameterValue,
) -> Result<(), CodecError> {
    const OPERATION: &str = "index SLDPRT parameter values";
    let id = copy_parameter_id(ctx, id)?;
    let value = value.try_clone_for_decode(ctx, "retain SLDPRT parameter value text")?;
    ctx.insert_hash_map(values, id, value, OPERATION)?;
    Ok(())
}

pub(crate) fn parameters_with_unresolved_references(
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
    aliases: &ParameterAliases,
) -> Result<usize, CodecError> {
    let mut count = 0;
    for parameter in ctx.admit_iter(parameters, "check SLDPRT parameter references")? {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let (parsed, _token_storage) = expression_identifier_tokens(ctx, &parameter.expression)?;
        let unresolved = match parsed {
            None => true,
            Some(parsed) => ctx.any_by(
                &parsed,
                |identifier| {
                    Ok(!identifier.is_syntax(ctx)?
                        && definite_parameter_reference(ctx, identifier)?
                        && match aliases
                            .get(ctx, identifier.value())?
                            .and_then(Option::as_ref)
                        {
                            Some(dependency) => ctx.equal(
                                dependency,
                                &parameter.id,
                                "check SLDPRT parameter references",
                            )?,
                            None => true,
                        })
                },
                "scan SLDPRT parameters_with_unresolved_references values",
            )?,
        };
        if unresolved {
            count += 1;
        }
    }
    Ok(count)
}

pub(crate) fn parameters_with_unevaluable_expressions(
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
    aliases: &ParameterAliases,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> Result<usize, CodecError> {
    if parameters.is_empty() {
        return Ok(0);
    }
    let (mut states, _states_storage) = ctx
        .with_scoped_storage("collect SLDPRT parameter value states", || {
            parameter_value_states(ctx, parameters, configurations, false)
        })?;
    let mut count = 0;
    for parameter in ctx.admit_iter(
        parameters,
        "scan SLDPRT parameters_with_unevaluable_expressions values",
    )? {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let mut states = states.iter_mut();
        while let Some(values) =
            ctx.next_charged(&mut states, "check SLDPRT parameter evaluation")?
        {
            let own = ctx.remove_entry_hash_map(
                values,
                &parameter.id,
                "check SLDPRT parameter evaluation",
            )?;
            let (evaluated, _evaluation_storage) =
                ctx.with_scoped_storage("check SLDPRT parameter evaluation", || {
                    match ParameterExpressionParser::new(
                        ctx,
                        &parameter.expression,
                        aliases,
                        &*values,
                    )
                    .parse()?
                    {
                        Some(value) => Ok(Some(value)),
                        None => text_parameter_literal(ctx, &parameter.name, &parameter.expression),
                    }
                })?;
            if let Some((id, value)) = own {
                ctx.insert_hash_map(values, id, value, "check SLDPRT parameter evaluation")?;
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
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
    aliases: &ParameterAliases,
) -> Result<usize, CodecError> {
    let mut count = 0;
    for parameter in ctx.admit_iter(
        parameters,
        "scan SLDPRT parameters_with_incoherent_dependencies values",
    )? {
        let (projected, _projected_storage) =
            ctx.with_scoped_storage("check SLDPRT parameter dependencies", || {
                project_parameter_dependencies(
                    ctx,
                    parameter,
                    aliases.for_owner(parameter.owner.as_ref()),
                )
            })?;
        if !ctx.equal(
            &parameter.dependencies,
            &projected,
            "compare SLDPRT parameter dependencies",
        )? {
            count += 1;
        }
    }
    Ok(count)
}

pub(crate) fn parameters_with_incoherent_evaluated_values(
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
    aliases: &ParameterAliases,
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
) -> Result<usize, CodecError> {
    if parameters.is_empty() {
        return Ok(0);
    }
    let (mut states, _states_storage) = ctx
        .with_scoped_storage("collect SLDPRT parameter value states", || {
            parameter_value_states(ctx, parameters, configurations, true)
        })?;
    let mut count = 0;
    for parameter in ctx
        .admit_iter(parameters, "scan SLDPRT parameter dependencies")?
        .filter(|parameter| !parameter.dependencies.is_empty())
    {
        let aliases = aliases.for_owner(parameter.owner.as_ref());
        let mut states = states.iter_mut();
        while let Some(values) =
            ctx.next_charged(&mut states, "check SLDPRT evaluated parameter coherence")?
        {
            let own = ctx.remove_entry_hash_map(
                values,
                &parameter.id,
                "check SLDPRT parameter evaluation",
            )?;
            let (evaluated, _evaluation_storage) =
                ctx.with_scoped_storage("check SLDPRT evaluated parameter coherence", || {
                    ParameterExpressionParser::new(ctx, &parameter.expression, aliases, &*values)
                        .parse()
                })?;
            let incoherent =
                own.as_ref()
                    .zip(evaluated.as_ref())
                    .is_some_and(|((_, actual), evaluated)| {
                        !equivalent_parameter_values(actual, evaluated)
                    });
            if let Some((id, value)) = own {
                ctx.insert_hash_map(values, id, value, "check SLDPRT parameter evaluation")?;
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
    ctx: &DecodeContext<'_>,
    parameters: &[DesignParameter],
    configurations: &[cadmpeg_ir::features::DesignConfiguration],
    include_global: bool,
) -> Result<Vec<HashMap<ParameterId, ParameterValue>>, CodecError> {
    let make_state = |configuration: Option<&cadmpeg_ir::features::DesignConfiguration>| -> Result<_, CodecError> {
        let mut values = HashMap::new();
        for parameter in ctx.admit_iter(parameters, "collect SLDPRT parameter value state")? {
            if let Some(value) = &parameter.value { insert_parameter_value(ctx, &mut values, &parameter.id, value)?; }
        }
        if let Some(configuration) = configuration {
            for (id, value) in ctx.admit_iter(&configuration.parameter_values, "scan SLDPRT parameter_value_states values")? { insert_parameter_value(ctx, &mut values, id, value)?; }
        }
        Ok(values)
    };
    let mut states = Vec::new();
    if include_global || configurations.is_empty() {
        let state = make_state(None)?;
        ctx.reserve_vec(&mut states, 1, "collect SLDPRT parameter value states")?;
        states.push(state);
    }
    for configuration in
        ctx.admit_iter(configurations, "scan SLDPRT parameter_value_states values")?
    {
        let state = make_state(Some(configuration))?;
        ctx.reserve_vec(&mut states, 1, "collect SLDPRT parameter value states")?;
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

pub(super) fn definite_parameter_reference(
    ctx: &DecodeContext<'_>,
    identifier: &ExpressionIdentifier<'_, '_>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "scan SLDPRT parameter reference ordinal";
    if identifier.is_quoted() {
        return Ok(true);
    }
    let value = identifier.value();
    if ctx.any_by(value.chars(), |character| Ok(character == '@'), OPERATION)? {
        return Ok(true);
    }
    let Some(ordinal) = ctx.strip_prefix(value, "D", OPERATION)? else {
        return Ok(false);
    };
    Ok(!ordinal.is_empty()
        && ctx.all_by(ordinal.bytes(), |byte| Ok(byte.is_ascii_digit()), OPERATION)?)
}

#[cfg(test)]
fn expression_identifiers(expression: &str) -> impl Iterator<Item = String> {
    let ctx = cadmpeg_test_support::service_decode_context();
    let (tokens, _storage) = expression_identifier_tokens(&ctx, expression).unwrap();
    tokens
        .unwrap_or_default()
        .into_iter()
        .filter(|token| !token.is_syntax(&ctx).unwrap())
        .map(|token| token.value().to_owned())
        .collect::<Vec<_>>()
        .into_iter()
}

enum ParameterTokenText<'a, 'ctx> {
    Borrowed(&'a str),
    Owned {
        value: String,
        _reservation: ScopedReservation<'ctx>,
    },
}

impl ParameterTokenText<'_, '_> {
    fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned { value, .. } => value,
        }
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
    fn quoted(
        ctx: &'ctx DecodeContext<'_>,
        expression: &'a str,
        start: usize,
        end: usize,
    ) -> Result<Option<Self>, CodecError> {
        let Some(raw) = expression.get(start..end) else {
            return Ok(None);
        };
        let Some(following) = expression.get(end..) else {
            return Ok(None);
        };
        let Some(inner) = raw
            .strip_prefix('"')
            .and_then(|inner| inner.strip_suffix('"'))
            .filter(|inner| !inner.is_empty())
        else {
            return Ok(None);
        };
        let value = if ctx.contains_text(inner, "\"\"", "unescape SLDPRT parameter identifier")? {
            let mut skip_quote = false;
            let (value, reservation) = ctx.collect_scoped_text(
                ctx.admit_iter(inner, "scan SLDPRT parameter quote escapes")?
                    .filter(|character| {
                        if skip_quote {
                            skip_quote = false;
                            return false;
                        }
                        skip_quote = *character == '"';
                        true
                    }),
                "unescape SLDPRT parameter identifier",
            )?;
            ParameterTokenText::Owned {
                value,
                _reservation: reservation,
            }
        } else {
            ParameterTokenText::Borrowed(inner)
        };
        Ok(Some(Self {
            raw,
            following,
            value,
            quoted: true,
        }))
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
    pub(super) fn is_syntax(&self, ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
        if self.quoted {
            return Ok(false);
        }
        if self
            .value()
            .starts_with(|character: char| character.is_ascii_digit() || character == '.')
        {
            return Ok(true);
        }
        if self.value().eq_ignore_ascii_case("pi")
            || self.value().eq_ignore_ascii_case("true")
            || self.value().eq_ignore_ascii_case("false")
        {
            return Ok(true);
        }
        if eval::ParameterFunction::parse(self.value()).is_none() {
            return Ok(false);
        }
        Ok(ctx
            .find_map(
                self.following.chars(),
                |character| Ok((!character.is_whitespace()).then_some(character == '(')),
                "scan SLDPRT parameter function suffix",
            )?
            .unwrap_or(false))
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
    ctx: &'ctx DecodeContext<'_>,
    expression: &'a str,
) -> Result<
    (
        Option<Vec<ExpressionIdentifier<'a, 'ctx>>>,
        ScopedReservation<'ctx>,
    ),
    CodecError,
> {
    const OPERATION: &str = "scan SLDPRT parameter identifiers";
    let mut storage = ctx.reserve_scoped(0, "collect SLDPRT parameter identifiers")?;
    let mut identifiers = Vec::new();
    let mut characters = expression.char_indices();
    while let Some((start, character)) = ctx.next_charged(&mut characters, OPERATION)? {
        if character == '"' {
            let mut closed = None;
            while let Some((at, character)) = ctx.next_charged(&mut characters, OPERATION)? {
                if character != '"' {
                    continue;
                }
                if expression[at + 1..].starts_with('"') {
                    ctx.next_charged(&mut characters, OPERATION)?;
                } else {
                    closed = Some(at + 1);
                    break;
                }
            }
            let Some(end) = closed else {
                return Ok((None, storage));
            };
            if let Some(identifier) = ExpressionIdentifier::quoted(ctx, expression, start, end)? {
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut identifiers,
                    identifier,
                    "collect SLDPRT parameter identifiers",
                )?;
            }
        } else if character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.') {
            let mut end = start + character.len_utf8();
            while expression[end..].starts_with(|character: char| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.')
            }) {
                let Some((at, character)) = ctx.next_charged(&mut characters, OPERATION)? else {
                    break;
                };
                end = at + character.len_utf8();
            }
            if let Some(identifier) = ExpressionIdentifier::plain(expression, start, end) {
                ctx.push_scoped_vec(
                    &mut storage,
                    &mut identifiers,
                    identifier,
                    "collect SLDPRT parameter identifiers",
                )?;
            }
        }
    }
    Ok((Some(identifiers), storage))
}
