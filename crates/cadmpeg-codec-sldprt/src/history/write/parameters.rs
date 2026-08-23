// SPDX-License-Identifier: Apache-2.0
//! Parameter records for native write.

#![allow(unused_imports)]
use crate::classification::{
    classify, classify_type_token, classify_xml_element, native_object_class,
    principal_plane_with_siblings, FeatureClass, NativeClassKind,
};
use crate::records::{Configuration, Feature, FeatureContent, FeatureHistory, HistoryContent};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::Annotations;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::features::{
    Angle, AxisAngle, BodyRetentionMode, BodySelection, BooleanOp, ChamferForm, ChamferSpec,
    ConfigurationBodies, ConfigurationId, CosmeticThreadExtent, CurveProjectionDirection,
    CurveProjectionDirectionState, DatumPlaneReference, DesignConfiguration, DesignParameter,
    DimensionDisplay, EdgeSelection, ExtrudeExtent, ExtrudeSide, FaceMotion, FaceSelection,
    FeatureDefinition, FeatureId, FeatureSourceContent, FeatureTreeNodeRole, FlexForm, FlexMode,
    HoleBottom, HoleForm, HoleKind, Length, ParameterId, ParameterValue, PathRef, PatternForm,
    PatternKind, PatternSeed, ProfileRef, RadiusForm, RadiusSpec, RevolutionAxis,
    RevolutionConstruction, RevolveExtent, RibConstruction, RibDraft, RibSide, RuledSurfaceMode,
    ScaleCenter, ScaleFactors, SketchSpace, SplitFaceTool, SurfaceExtension, SweepMode,
    Termination, TrimRegion, VariableRadius, VertexSelection, WrapMode,
};
use cadmpeg_ir::geometry::{Curve, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::AttributeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{Body, Edge, Face};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::Exactness;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use super::features::{generated_feature_record_id, synchronize_history_content_order};
use crate::history::hash::{native_parameter_hash, parameter_hash};
use crate::history::literals::{
    dimension_display, format_parameter_value, parse_neutral_parameter_literal,
    parse_parameter_literal,
};
use crate::history::parameters::{
    expression_identifier_is_syntax, expression_identifier_tokens, global_parameter_owners,
    parameters_with_incoherent_dependencies, parse_native_parameter_literal, project_parameters,
};
use crate::history::project::neutral_feature_id;
use crate::resolved_features::relation_geometry::is_reference_relation_parameter;

pub fn prepare_parameters_for_write(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
    feature_parameter_changes_authorized: bool,
) -> Result<(), CodecError> {
    let neutral_hash = parameter_hash(&ir.model.parameters);
    let native_hash = native
        .as_ref()
        .map(|value| native_parameter_hash(&value.feature_histories));
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
    let native_changed = match (&native_hash, baseline_native) {
        (Some(current), Some(baseline)) => current != baseline,
        (Some(_), None) | (None, Some(_)) => true,
        (None, None) => false,
    };
    if baseline_neutral.is_none() && baseline_native.is_none() {
        if ir.model.parameters.is_empty() {
            return Ok(());
        }
        return sync_neutral_parameters(ir, native);
    }
    match (neutral_changed, native_changed) {
        (false, true) if feature_parameter_changes_authorized => Ok(()),
        (false, _) => Ok(()),
        (true, true) => {
            if feature_parameter_changes_authorized {
                return sync_neutral_parameters(ir, native);
            }
            let projected = native
                .as_ref()
                .map(|value| project_parameters(&value.feature_histories))
                .unwrap_or_default();
            if parameter_hash(&projected) == neutral_hash {
                Ok(())
            } else {
                Err(CodecError::Malformed(
                    "conflicting neutral and native SLDPRT parameter edits".into(),
                ))
            }
        }
        (true, false) => sync_neutral_parameters(ir, native),
    }
}

pub(crate) fn sync_neutral_parameters(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    if let Some(parameter) = ir
        .model
        .parameters
        .iter()
        .find(|parameter| is_reference_relation_parameter(parameter))
    {
        return Err(CodecError::NotImplemented(format!(
            "SLDPRT display-only relation parameter {} has no writable native scalar",
            parameter.id.0
        )));
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
    let global_owners = global_parameter_owners(&ir.model.features);
    if let Some(native) = native.as_ref() {
        let original = project_parameters(&native.feature_histories);
        let original_feature_names = native
            .feature_histories
            .iter()
            .flat_map(|history| &history.features)
            .map(|feature| (neutral_feature_id(&feature.id), feature.name.clone()))
            .collect::<HashMap<_, _>>();
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
    let mut desired = HashMap::<FeatureId, Vec<&DesignParameter>>::new();
    for parameter in &parameters {
        let Some(owner_id) = parameter.owner.as_ref() else {
            return Err(CodecError::NotImplemented(format!(
                "global SLDPRT parameter {} cannot be written to a feature record",
                parameter.id.0
            )));
        };
        let Some(owner) = features.get(owner_id) else {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} references a missing feature",
                parameter.id.0
            )));
        };
        if parameter.display != dimension_display(&parameter.expression) {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} has display semantics inconsistent with its expression",
                parameter.id.0
            )));
        }
        if parse_neutral_parameter_literal(owner, &parameter.name, &parameter.expression)
            .is_some_and(|literal| parameter.value.as_ref() != Some(&literal))
        {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} has a value inconsistent with its expression",
                parameter.id.0
            )));
        }
        let owner_parameters = desired.entry(owner_id.clone()).or_default();
        if owner_parameters
            .iter()
            .any(|candidate| candidate.name == parameter.name)
        {
            return Err(CodecError::Malformed(format!(
                "duplicate SLDPRT parameter {} on feature {}",
                parameter.name, owner_id
            )));
        }
        if owner_parameters
            .iter()
            .any(|candidate| candidate.ordinal == parameter.ordinal)
        {
            return Err(CodecError::Malformed(format!(
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
    for (feature_id, feature) in features {
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
        record.parameters = parameters
            .iter()
            .map(|parameter| (parameter.name.clone(), parameter.expression.clone()))
            .collect();
        record.dimension_properties = parameters
            .iter()
            .map(|parameter| {
                let mut properties = parameter.properties.clone();
                if parse_parameter_literal(&parameter.expression).is_none() {
                    if let Some(value) = &parameter.value {
                        properties.insert("Value".into(), format_parameter_value(value));
                    } else {
                        properties.remove("Value");
                    }
                }
                (parameter.name.clone(), properties)
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
    for parameter in &parameters {
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
                CodecError::Malformed(format!(
                    "SLDPRT parameter {} references missing scalar {native_ref}",
                    parameter.id.0
                ))
            })?;
        let lane = &mut native.feature_input_lanes[location.0];
        let scalar = &mut lane.scalars[location.1];
        if scalar.role == crate::records::FeatureInputScalarRole::Display {
            return Err(CodecError::Malformed(format!(
                "SLDPRT parameter {} references a display scalar",
                parameter.id.0
            )));
        }
        let value = match parameter.value {
            Some(ParameterValue::Length(length)) => length.0 / 1000.0,
            Some(ParameterValue::Angle(angle)) => angle.0,
            Some(ParameterValue::Real(value)) => value,
            _ => {
                return Err(CodecError::NotImplemented(format!(
                    "SLDPRT scalar {} requires a real-valued parameter",
                    scalar.id
                )));
            }
        };
        let offset = usize::try_from(scalar.offset).map_err(|_| {
            CodecError::Malformed("SLDPRT scalar offset exceeds address space".into())
        })?;
        let bytes = lane
            .native_payload
            .get_mut(offset..offset + 8)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "SLDPRT scalar {} lies outside its payload",
                    scalar.id
                ))
            })?;
        bytes.copy_from_slice(&value.to_le_bytes());
        scalar.value = value;
    }
    Ok(())
}

pub(crate) fn rewrite_renamed_parameter_references(
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

pub(crate) fn rewrite_parameter_expression(
    expression: &str,
    aliases: &HashMap<String, String>,
) -> Option<String> {
    let tokens = expression_identifier_tokens(expression).identifiers;
    let mut rewritten = String::with_capacity(expression.len());
    let mut copied = 0;
    for token in tokens {
        if expression_identifier_is_syntax(expression, &token) {
            continue;
        }
        let Some(replacement) = aliases.get(&token.value) else {
            continue;
        };
        rewritten.push_str(&expression[copied..token.start]);
        if token.quoted || !unquoted_expression_identifier(replacement) {
            rewritten.push('"');
            rewritten.push_str(&replacement.replace('"', "\"\""));
            rewritten.push('"');
        } else {
            rewritten.push_str(replacement);
        }
        copied = token.end;
    }
    if copied == 0 {
        return None;
    }
    rewritten.push_str(&expression[copied..]);
    Some(rewritten)
}

pub(crate) fn unquoted_expression_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters.next().is_some_and(|character| {
        !character.is_ascii_digit()
            && character != '.'
            && (character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$'))
    }) && characters.all(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '@' | '$' | '.')
    })
}

pub(crate) fn restore_equivalent_parameter_expressions(
    feature: &Feature,
    original_parameters: &HashMap<String, BTreeMap<String, String>>,
    evaluated_parameters: &HashMap<String, BTreeMap<String, String>>,
    desired_parameters: &mut BTreeMap<String, String>,
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
        if parse_native_parameter_literal(feature, name, expression).is_some() {
            continue;
        }
        let Some(evaluated) = evaluated.get(name) else {
            continue;
        };
        let Some(desired_value) = parse_native_parameter_literal(feature, name, desired) else {
            continue;
        };
        let Some(evaluated_value) = parse_native_parameter_literal(feature, name, evaluated) else {
            continue;
        };
        if desired_value == evaluated_value {
            desired.clone_from(expression);
        }
    }
}
