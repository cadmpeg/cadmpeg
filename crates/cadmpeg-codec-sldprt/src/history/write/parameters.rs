// SPDX-License-Identifier: Apache-2.0
//! Parameter records for native write.

use crate::records::{Feature, FeatureContent};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::parameters::{DesignParameter, ParameterValue};
use cadmpeg_ir::features::{FeatureId, ParameterId};
use std::collections::{BTreeMap, HashMap};

use super::edits::SourceEdits;
use super::features::generated_feature_record_id;
use crate::history::hash::{native_parameter_hash, parameter_hash};
use crate::history::literals::{
    dimension_display, format_parameter_value, parse_neutral_parameter_literal,
    parse_parameter_literal,
};
use crate::history::parameters::eval::exact_integer_f64;
use crate::history::parameters::{
    expression_identifier_tokens, global_parameter_owners, parameters_with_incoherent_dependencies,
    parse_native_parameter_literal, project_parameters, stated_parameter_value,
};
use crate::history::project::neutral_feature_id;
use crate::resolved_features::relation_geometry::is_reference_relation_parameter;

pub(crate) fn prepare_parameters_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
    feature_parameter_changes_authorized: bool,
    edits: &mut SourceEdits<'_>,
) -> Result<(), CodecError> {
    let hash_arena = cadmpeg_core::decode::DecodeArena::new();
    let (hash_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &hash_arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;

    let neutral_hash = parameter_hash(&hash_ctx, &ir.model.parameters)?;
    let native_hash = native
        .as_ref()
        .map(|value| native_parameter_hash(&hash_ctx, &value.feature_histories))
        .transpose()?;
    let baseline_neutral = ir.source.as_ref().and_then(|source| {
        source
            .attributes
            .get("sldprt_neutral_parameter_local_sha256")
    });
    let baseline_native = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("sldprt_native_parameter_sha256"));
    let neutral_changed = baseline_neutral.is_none_or(|hash| hash != &neutral_hash);
    let native_changed = !edits.native_unedited()
        && match (&native_hash, baseline_native) {
            (Some(current), Some(baseline)) => current != baseline,
            (Some(_), None) | (None, Some(_)) => true,
            (None, None) => false,
        };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.parameters.is_empty() {
            return Ok(());
        }
        return sync_neutral_parameters(ir, native, edits);
    }
    match (neutral_changed, native_changed) {
        (false, true) if feature_parameter_changes_authorized => Ok(()),
        (false, _) => Ok(()),
        (true, true) => {
            if feature_parameter_changes_authorized {
                return sync_neutral_parameters(ir, native, edits);
            }
            let lane_bytes = native
                .as_ref()
                .into_iter()
                .flat_map(|native| &native.feature_input_lanes)
                .flat_map(|lane| lane.native_payload.iter().copied())
                .collect::<Vec<_>>();
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &lane_bytes,
                &arena,
                &cadmpeg_core::decode::DecodePolicy::service(),
            )?;
            let projected = native
                .as_ref()
                .map(|value| project_parameters(&ctx, &value.feature_histories))
                .transpose()?
                .unwrap_or_default();
            if parameter_hash(&hash_ctx, &projected)? == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT parameter edits".into(),
                ))
            }
        }
        (true, false) => sync_neutral_parameters(ir, native, edits),
    }
}

/// The dimension properties the record states for `parameter`: a
/// non-literal expression carries its value in the `Value` property.
fn written_dimension_properties(
    parameter: &DesignParameter,
) -> BTreeMap<cadmpeg_core::text::NonBlankString, String> {
    let mut properties = parameter.properties.clone();
    if parse_parameter_literal(&parameter.expression).is_none() {
        if let Some(value) = &parameter.value {
            properties.insert(
                cadmpeg_core::nonblank_literal!("Value"),
                format_parameter_value(value),
            );
        } else {
            properties.remove("Value");
        }
    }
    properties
}

fn sync_neutral_parameters(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
    edits: &mut SourceEdits<'_>,
) -> Result<(), CodecError> {
    // The decoder projects a display-only relation parameter from its
    // relation's solved display scalar; no feature record holds it.
    for parameter in ir
        .model
        .parameters
        .iter()
        .filter(|parameter| is_reference_relation_parameter(parameter))
    {
        if edits.parameter_edited(&parameter.id) {
            edits.parameter_unwritten(format!(
                "Display-only relation parameter {} ({} = {}) is not written; the stored relation keeps its solved value.",
                parameter.id.as_str(),
                parameter.name,
                parameter.expression
            ));
        }
    }
    let mut parameters = ir.model.parameters.clone();
    let feature_names = ir
        .model
        .features
        .iter()
        .filter_map(|feature| {
            feature
                .name
                .as_ref()
                .map(|name| (feature.id.clone(), name.clone()))
        })
        .collect::<HashMap<_, _>>();
    let lane_bytes = native
        .as_ref()
        .into_iter()
        .flat_map(|native| &native.feature_input_lanes)
        .flat_map(|lane| lane.native_payload.iter().copied())
        .collect::<Vec<_>>();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &lane_bytes,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )?;
    let global_owners = global_parameter_owners(&ir.model.features);
    if let Some(native) = native.as_ref() {
        let original = project_parameters(&ctx, &native.feature_histories)?;
        let original_feature_names = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .map(|feature| Ok((neutral_feature_id(&feature.id)?, feature.name.clone())))
            .collect::<Result<HashMap<_, _>, CodecError>>()?;
        rewrite_renamed_parameter_references(
            &mut parameters,
            &original,
            &original_feature_names,
            &feature_names,
        );
    }
    if parameters_with_incoherent_dependencies(&parameters, &feature_names, &global_owners) > 0 {
        return Err(CodecError::Malformed(
            "SLDPRT parameter dependencies are inconsistent with their expressions".into(),
        ));
    }
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    let records = native
        .iter()
        .flat_map(|native| &native.feature_histories)
        .flat_map(|history| &history.features)
        .map(|record| (record.id.as_str(), record))
        .collect::<HashMap<_, _>>();
    let mut desired = HashMap::<FeatureId, Vec<&DesignParameter>>::new();
    for parameter in &parameters {
        if is_reference_relation_parameter(parameter) {
            continue;
        }
        let Some(owner_id) = parameter.owner.as_ref() else {
            return Err(CodecError::NotImplemented(format!(
                "global SLDPRT parameter {} cannot be written to a feature record",
                parameter.id.as_str()
            )));
        };
        let Some(owner) = features.get(owner_id) else {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT parameter {} references a missing feature",
                parameter.id.as_str()
            )));
        };
        let edited = edits.parameter_edited(&parameter.id);
        if edited && parameter.display != dimension_display(&parameter.expression) {
            edits.parameter_unwritten(format!(
                "Parameter {} is written with expression {}; its display semantics {:?} are not written.",
                parameter.id.as_str(),
                parameter.expression,
                parameter.display
            ));
        }
        // A scalar-bound parameter carries its value in the bound scalar and
        // its expression in the record; the record alone states the value of
        // any other parameter, read as the decoder reads it.
        if edited && parameter.native_ref.is_none() {
            let record_id = owner
                .native_ref
                .clone()
                .unwrap_or_else(|| generated_feature_record_id(owner_id));
            let record = records.get(record_id.as_str()).copied();
            let stated = record.map_or_else(
                || parse_neutral_parameter_literal(owner, &parameter.name, &parameter.expression),
                |record| {
                    stated_parameter_value(
                        record,
                        &parameter.name,
                        &parameter.expression,
                        &written_dimension_properties(parameter),
                    )
                },
            );
            if let Some(stated) = stated.filter(|stated| parameter.value.as_ref() != Some(stated)) {
                edits.parameter_unwritten(format!(
                    "Parameter {} is written with expression {}, which states {}; its value {} is not written.",
                    parameter.id.as_str(),
                    parameter.expression,
                    format_parameter_value(&stated),
                    parameter.value.as_ref().map_or_else(|| "none".to_owned(), format_parameter_value)
                ));
            }
        }
        let owner_parameters = desired.entry(owner_id.clone()).or_default();
        if owner_parameters
            .iter()
            .any(|candidate| candidate.name == parameter.name)
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate SLDPRT parameter {} on feature {}",
                parameter.name, owner_id
            )));
        }
        if owner_parameters
            .iter()
            .any(|candidate| candidate.ordinal == parameter.ordinal)
        {
            return Err(CodecError::malformed(format_args!(
                "duplicate SLDPRT parameter ordinal {} on feature {}",
                parameter.ordinal, owner_id
            )));
        }
        owner_parameters.push(parameter);
    }
    let Some(native) = native.as_mut() else {
        return Err(CodecError::NotImplemented(
            "SLDPRT parameters require feature records".into(),
        ));
    };
    for feature in &ir.model.features {
        let feature_id = &feature.id;
        let record = native
            .feature_histories
            .iter_mut()
            .flat_map(|history| &mut history.features)
            .find(|record| {
                feature.native_ref.as_deref() == Some(record.id.as_str())
                    || (feature.native_ref.is_none()
                        && record.id == generated_feature_record_id(feature_id))
            })
            .ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "SLDPRT parameters for feature {feature_id} require a retained feature record"
                ))
            })?;
        let mut parameters = desired.remove(feature_id).unwrap_or_default();
        parameters.sort_by_key(|parameter| parameter.ordinal);
        record.parameters = cadmpeg_core::text::named_entries(
            format_args!("the sldprt feature {feature_id} record"),
            parameters
                .iter()
                .map(|parameter| (parameter.name.clone(), parameter.expression.clone())),
        )?;
        record.dimension_properties = parameters
            .iter()
            .map(|parameter| {
                (
                    parameter.name.clone(),
                    written_dimension_properties(parameter),
                )
            })
            .collect();
        let mut names = parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<Vec<_>>()
            .into_iter();
        let mut content = record
            .content
            .iter()
            .filter_map(|item| match item {
                FeatureContent::Dimension(_) => names.next().map(FeatureContent::Dimension),
                other => Some(other.clone()),
            })
            .collect::<Vec<_>>();
        content.extend(names.map(FeatureContent::Dimension));
        record.content = content;
    }
    // The stored scalar of an unedited parameter keeps its bytes: the value
    // the decoder derived from it does not convert back exactly.
    for parameter in &parameters {
        if !edits.parameter_edited(&parameter.id) {
            continue;
        }
        let Some(native_ref) = parameter.native_ref.as_deref() else {
            continue;
        };
        let location = native
            .feature_input_lanes
            .iter()
            .enumerate()
            .find_map(|(lane_index, lane)| {
                lane.scalars
                    .iter()
                    .position(|scalar| scalar.id == native_ref)
                    .map(|scalar_index| (lane_index, scalar_index))
            })
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT parameter {} references missing scalar {native_ref}",
                    parameter.id.as_str()
                ))
            })?;
        let lane = &mut native.feature_input_lanes[location.0];
        let scalar = &mut lane.scalars[location.1];
        if scalar.role == crate::records::FeatureInputScalarRole::Display {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT parameter {} references a display scalar",
                parameter.id.as_str()
            )));
        }
        // The decoder keeps an integer or boolean value only for a scalar
        // that equals it exactly.
        let value = match &parameter.value {
            Some(ParameterValue::Length(length)) => Some(length.get() / 1000.0),
            Some(ParameterValue::Angle(angle)) => Some(angle.get()),
            Some(ParameterValue::Real(value)) => Some(value.get()),
            Some(ParameterValue::Integer(value)) => exact_integer_f64(*value),
            Some(ParameterValue::Boolean(value)) => Some(if *value { 1.0 } else { 0.0 }),
            Some(ParameterValue::String(_)) | None => None,
        };
        let Some(value) = value else {
            edits.parameter_unwritten(format!(
                "Parameter {} has no real value for its native scalar {}; the scalar keeps its stored value.",
                parameter.id.as_str(),
                scalar.id
            ));
            continue;
        };
        let offset = usize::try_from(scalar.offset).map_err(|_| {
            CodecError::Malformed("SLDPRT scalar offset exceeds address space".into())
        })?;
        let bytes = lane
            .native_payload
            .get_mut(offset..offset + 8)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT scalar {} lies outside its payload",
                    scalar.id
                ))
            })?;
        let checked = cadmpeg_ir::scalar::FiniteReal::new(value)
            .ok_or_else(|| CodecError::malformed("SLDPRT scalar update is non-finite"))?;
        bytes.copy_from_slice(&value.to_le_bytes());
        scalar.value = checked;
    }
    Ok(())
}

fn rewrite_renamed_parameter_references(
    parameters: &mut [DesignParameter],
    original: &[DesignParameter],
    original_feature_names: &HashMap<FeatureId, String>,
    feature_names: &HashMap<FeatureId, String>,
) {
    let original = original
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let desired = parameters
        .iter()
        .map(|parameter| (&parameter.id, parameter))
        .collect::<HashMap<_, _>>();
    let mut replacements = HashMap::<ParameterId, HashMap<String, String>>::new();
    for (id, parameter) in &desired {
        let Some(previous) = original.get(id) else {
            continue;
        };
        let mut aliases = HashMap::new();
        let previous_owner_name = parameter
            .owner
            .as_ref()
            .and_then(|owner| original_feature_names.get(owner));
        let owner_name = parameter
            .owner
            .as_ref()
            .and_then(|owner| feature_names.get(owner));
        if previous.name != parameter.name {
            aliases.insert(previous.name.clone(), parameter.name.clone());
        }
        if previous.name != parameter.name || previous_owner_name != owner_name {
            if let (Some(previous_owner_name), Some(owner_name)) = (previous_owner_name, owner_name)
            {
                aliases.insert(
                    format!("{}@{previous_owner_name}", previous.name),
                    format!("{}@{owner_name}", parameter.name),
                );
            }
        }
        if let Some(previous_id) = previous.properties.get("EquationId") {
            let replacement = parameter
                .properties
                .get("EquationId")
                .unwrap_or(&parameter.name);
            if previous_id != replacement || previous_owner_name != owner_name {
                aliases.insert(previous_id.clone(), replacement.clone());
                if let (Some(previous_owner_name), Some(owner_name)) =
                    (previous_owner_name, owner_name)
                {
                    if !previous_id.contains('@') {
                        let qualified_replacement = if replacement.contains('@') {
                            replacement.clone()
                        } else {
                            format!("{replacement}@{owner_name}")
                        };
                        aliases.insert(
                            format!("{previous_id}@{previous_owner_name}"),
                            qualified_replacement,
                        );
                    }
                }
            }
        }
        if !aliases.is_empty() {
            replacements.insert((*id).clone(), aliases);
        }
    }
    for parameter in parameters {
        let aliases = parameter
            .dependencies
            .iter()
            .filter_map(|dependency| replacements.get(dependency))
            .flat_map(|aliases| aliases.iter())
            .map(|(alias, replacement)| (alias.clone(), replacement.clone()))
            .collect::<HashMap<_, _>>();
        if aliases.is_empty() {
            continue;
        }
        if let Some(rewritten) = rewrite_parameter_expression(&parameter.expression, &aliases) {
            parameter.expression = rewritten;
        }
    }
}

pub(in crate::history) fn rewrite_parameter_expression(
    expression: &str,
    aliases: &HashMap<String, String>,
) -> Option<String> {
    let tokens = expression_identifier_tokens(expression)?;
    let mut rewritten = String::with_capacity(expression.len());
    let mut tail = expression;
    let mut replaced = false;
    for token in tokens {
        if token.is_syntax() {
            continue;
        }
        let Some(replacement) = aliases.get(token.value()) else {
            continue;
        };
        let Some(preceding) = token.preceding(tail) else {
            continue;
        };
        rewritten.push_str(preceding);
        if token.is_quoted() || !unquoted_expression_identifier(replacement) {
            rewritten.push('"');
            rewritten.push_str(&replacement.replace('"', "\"\""));
            rewritten.push('"');
        } else {
            rewritten.push_str(replacement);
        }
        tail = token.following();
        replaced = true;
    }
    if !replaced {
        return None;
    }
    rewritten.push_str(tail);
    Some(rewritten)
}

pub(in crate::history) fn unquoted_expression_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(|character| {
        !character.is_ascii_digit()
            && character != '.'
            && (character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$'))
    }) && characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.')
    })
}

pub(super) fn restore_equivalent_parameter_expressions(
    feature: &Feature,
    original_parameters: &HashMap<String, BTreeMap<cadmpeg_core::text::NonBlankString, String>>,
    evaluated_parameters: &HashMap<String, BTreeMap<cadmpeg_core::text::NonBlankString, String>>,
    desired_parameters: &mut BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) {
    let Some(original) = original_parameters.get(&feature.id) else {
        return;
    };
    let Some(evaluated) = evaluated_parameters.get(&feature.id) else {
        return;
    };
    for (name, desired) in desired_parameters {
        let Some(expression) = original.get(name) else {
            continue;
        };
        if parse_native_parameter_literal(feature, name.as_str(), expression).is_some() {
            continue;
        }
        let Some(evaluated) = evaluated.get(name) else {
            continue;
        };
        let Some(desired_value) = parse_native_parameter_literal(feature, name.as_str(), desired)
        else {
            continue;
        };
        let Some(evaluated_value) =
            parse_native_parameter_literal(feature, name.as_str(), evaluated)
        else {
            continue;
        };
        if desired_value == evaluated_value {
            desired.clone_from(expression);
        }
    }
}
