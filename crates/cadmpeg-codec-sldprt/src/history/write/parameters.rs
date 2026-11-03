// SPDX-License-Identifier: Apache-2.0
//! Parameter records for native write.

use crate::records::FeatureContent;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, FeatureId, ParameterId, ParameterValue};
use std::collections::{BTreeSet, HashMap, HashSet};

use super::features::generated_feature_record_id;
use crate::history::parameters::{
    global_parameter_owners, parameters_with_incoherent_dependencies, render_parameter_expression,
};

pub(crate) fn synchronize_parameter_records(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    if !ir.scope.design {
        return Ok(());
    }
    sync_neutral_parameters(ir, native)
}

fn sync_neutral_parameters(
    ir: &cadmpeg_ir::CadIr,
    native: &mut Option<crate::native::SldprtNative>,
) -> Result<(), CodecError> {
    let parameters = &ir.model.parameters;
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
    if parameters_with_incoherent_dependencies(parameters, &feature_names, &global_owners) > 0 {
        return Err(CodecError::malformed(
            "SLDPRT parameter expressions reference missing canonical identities",
        ));
    }
    let rendered = parameters
        .iter()
        .map(|parameter| {
            let text =
                render_parameter_expression(parameter, parameters, &feature_names, &global_owners)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "SLDPRT parameter {} references an unrenderable canonical identity",
                            parameter.id
                        ))
                    })?;
            let text = match parameter.display {
                Some(cadmpeg_ir::features::DimensionDisplay::Diameter) => {
                    format!("<MOD-DIAM>{text}")
                }
                Some(cadmpeg_ir::features::DimensionDisplay::Radius) => format!("R{text}"),
                None => text,
            };
            Ok((parameter.id.clone(), text))
        })
        .collect::<Result<HashMap<_, _>, CodecError>>()?;
    let features = ir
        .model
        .features
        .iter()
        .map(|feature| (&feature.id, feature))
        .collect::<HashMap<_, _>>();
    let mut desired = HashMap::<FeatureId, Vec<&DesignParameter>>::new();
    // Scalar-linked parameters mirror their lane scalars. When the two
    // disagree, the edit direction follows references: a parameter bound by a
    // document dimension drives its scalar, while an orphaned parameter keeps
    // the matching lane edit and stays out of the record (#237).
    let orphaned = orphaned_scalar_parameters(ir, native.as_ref())?;
    for parameter in parameters {
        if orphaned.contains(&parameter.id) {
            continue;
        }
        let Some(owner_id) = parameter.owner.as_ref() else {
            return Err(CodecError::UnsupportedContent(format!(
                "global SLDPRT parameter {} cannot be written to a feature record",
                parameter.id.as_str()
            )));
        };
        if !features.contains_key(owner_id) {
            return Err(CodecError::malformed(format_args!(
                "SLDPRT parameter {} references a missing feature",
                parameter.id
            )));
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
        return Err(CodecError::UnsupportedContent(
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
                CodecError::UnsupportedContent(format!(
                    "SLDPRT parameters for feature {feature_id} require a retained feature record"
                ))
            })?;
        let parameters = order_owner_parameters(desired.remove(feature_id).unwrap_or_default());
        let canonical = cadmpeg_core::text::named_entries(
            format_args!("the sldprt feature {feature_id} record"),
            parameters.iter().map(|parameter| {
                let native_literal = parameter.value.as_ref().and_then(|value| {
                    (parameter.expression == value.literal_expression()
                        && parameter.expression.literal_value_as(value).as_ref() == Some(value))
                    .then(|| {
                        let target = record.parameters.get(parameter.name.as_str())?;
                        (crate::history::parameters::parse_native_parameter_literal(
                            record,
                            parameter.name.as_str(),
                            target,
                        )
                        .as_ref()
                            == Some(value))
                        .then(|| target.clone())
                    })
                    .flatten()
                });
                (
                    parameter.name.clone(),
                    native_literal.unwrap_or_else(|| rendered[&parameter.id].clone()),
                )
            }),
        )?;
        record.parameters.extend(canonical);
        record.dimension_properties = parameters
            .iter()
            .map(|parameter| {
                let mut properties = parameter.properties.clone();
                properties.remove("Value");
                properties.remove("Driven");
                properties.remove("EquationId");
                if let Some(alias) = record
                    .dimension_properties
                    .get(parameter.name.as_str())
                    .and_then(|properties| properties.get("EquationId"))
                {
                    properties.insert(cadmpeg_core::nonblank_literal!("EquationId"), alias.clone());
                }
                if let Some(value) = &parameter.value {
                    properties.insert(
                        cadmpeg_core::nonblank_literal!("Value"),
                        crate::history::literals::format_parameter_cache_value(value),
                    );
                }
                properties.insert(
                    cadmpeg_core::nonblank_literal!("Driven"),
                    (!parameter.driving).to_string(),
                );
                (parameter.name.clone(), properties)
            })
            .collect();
        let mut names = parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect::<Vec<_>>();
        names.extend(
            record
                .parameters
                .keys()
                .filter(|name| {
                    !parameters
                        .iter()
                        .any(|parameter| parameter.name.as_str() == name.as_str())
                })
                .map(ToString::to_string),
        );
        let mut names = names.into_iter();
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
    for parameter in parameters {
        if orphaned.contains(&parameter.id) {
            continue;
        }
        let Some(native_ref) = parameter.native_ref.as_deref() else {
            continue;
        };
        let location =
            native
                .feature_input_lanes
                .iter()
                .enumerate()
                .find_map(|(lane_index, lane)| {
                    lane.scalars
                        .iter()
                        .position(|scalar| scalar.id == native_ref)
                        .map(|scalar_index| (lane_index, scalar_index))
                });
        let Some((lane_index, scalar_index)) = location else {
            // A parameter linked to its dimension record rather than a lane
            // scalar has no lane value to synchronize.
            if native
                .feature_histories
                .iter()
                .flat_map(|history| &history.features)
                .any(|record| record.id.as_str() == native_ref)
            {
                continue;
            }
            return Err(CodecError::malformed(format_args!(
                "SLDPRT parameter {} references missing scalar {native_ref}",
                parameter.id.as_str()
            )));
        };
        let lane = &mut native.feature_input_lanes[lane_index];
        let scalar_name = lane
            .names
            .iter()
            .find(|name| name.id == lane.scalars[scalar_index].name)
            .map(|name| name.value.as_str());
        if scalar_name != Some(parameter.name.as_str()) {
            // (P): changing a counted scalar name relocates following lane records.
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT canonical parameter {} was renamed; input lanes require regeneration",
                parameter.id
            )));
        }
        let scalar = &mut lane.scalars[scalar_index];
        let value = match parameter.value.as_ref() {
            Some(ParameterValue::Length(length)) => length.get() / 1000.0,
            Some(ParameterValue::Angle(angle)) => angle.get(),
            Some(ParameterValue::Real(value)) => value.get(),
            Some(ParameterValue::Integer(value)) => cadmpeg_core::convert::f64_from_i64(*value)
                .ok_or_else(|| {
                    CodecError::UnsupportedContent(format!(
                        "SLDPRT scalar {} cannot exactly encode integer {value}",
                        scalar.id
                    ))
                })?,
            _ => {
                return Err(CodecError::UnsupportedContent(format!(
                    "SLDPRT scalar {} requires a real-valued parameter",
                    scalar.id
                )));
            }
        };
        let checked = cadmpeg_ir::scalar::FiniteReal::new(value)
            .ok_or_else(|| CodecError::malformed("SLDPRT scalar update is non-finite"))?;
        if !lane.native_payload.is_empty() {
            let offset = usize::try_from(scalar.offset)
                .map_err(|_| CodecError::malformed("SLDPRT scalar offset exceeds address space"))?;
            let end = offset.checked_add(8).ok_or_else(|| {
                CodecError::malformed("SLDPRT scalar extent overflows address space")
            })?;
            let bytes = lane.native_payload.get_mut(offset..end).ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "SLDPRT scalar {} lies outside its payload",
                    scalar.id
                ))
            })?;
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        scalar.value = checked;
    }
    Ok(())
}

/// Scalar-linked parameters whose lane scalar disagrees with their value and
/// which nothing references. Their lane edit stands: the stale parameter
/// stays out of the record and the lane keeps its bytes.
///
/// References come from the schema-driven neutral walk, which covers
/// dimension bindings, expressions, and overrides alike.
pub(crate) fn orphaned_scalar_parameters(
    ir: &cadmpeg_ir::CadIr,
    native: Option<&crate::native::SldprtNative>,
) -> Result<HashSet<ParameterId>, CodecError> {
    let Some(native) = native else {
        return Ok(HashSet::new());
    };
    // Disagreements are edits; agreement needs no reference data.
    let mut disagreements = Vec::new();
    for (lane_index, lane) in native.feature_input_lanes.iter().enumerate() {
        for (scalar_index, scalar) in lane.scalars.iter().enumerate() {
            let Some(parameter_index) = ir
                .model
                .parameters
                .iter()
                .position(|parameter| parameter.native_ref.as_deref() == Some(scalar.id.as_str()))
            else {
                continue;
            };
            let Some(value) = ir.model.parameters[parameter_index].value.as_ref() else {
                continue;
            };
            let lane_value = match value {
                ParameterValue::Length(length) => length.get() / 1000.0,
                ParameterValue::Angle(angle) => angle.get(),
                ParameterValue::Real(value) => value.get(),
                ParameterValue::Integer(value) => {
                    match cadmpeg_core::convert::f64_from_i64(*value) {
                        Some(value) => value,
                        None => continue,
                    }
                }
                _ => continue,
            };
            if lane_value != scalar.value.get() {
                disagreements.push((parameter_index, lane_index, scalar_index));
            }
        }
    }
    if disagreements.is_empty() {
        return Ok(HashSet::new());
    }
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::desktop(),
    )?;
    let mut referenced = HashSet::new();
    ir.visit_neutral_references(&ctx, &mut |_owner, target| {
        referenced.insert(target.to_owned());
        Ok(())
    })?;
    Ok(disagreements
        .into_iter()
        .filter(|(parameter_index, lane_index, scalar_index)| {
            let parameter = &ir.model.parameters[*parameter_index];
            if referenced.contains(parameter.id.as_str()) {
                return false;
            }
            // An unreferenced parameter keeps a lane edit that matches the
            // document: the lane states what a role dimension holds. Any
            // other disagreement keeps the parameter's canonical value.
            let lane = &native.feature_input_lanes[*lane_index];
            let scalar = &lane.scalars[*scalar_index];
            lane_matches_document_dimension(ir, parameter, lane, scalar)
        })
        .map(|(parameter_index, _, _)| ir.model.parameters[parameter_index].id.clone())
        .collect())
}

/// Whether the lane scalar states what the document holds for its native
/// role: a role dimension whose literal denotes the scalar's value. Units
/// follow the linked parameter's quantity.
fn lane_matches_document_dimension(
    ir: &cadmpeg_ir::CadIr,
    parameter: &DesignParameter,
    lane: &crate::records::FeatureInputLane,
    scalar: &crate::records::FeatureInputScalar,
) -> bool {
    let divisor = match parameter.value.as_ref() {
        Some(ParameterValue::Length(_)) => 1000.0,
        _ => 1.0,
    };
    let Some(history_id) = scalar.feature_ref.as_deref() else {
        return false;
    };
    let Some(role) = lane
        .names
        .iter()
        .find(|name| name.id == scalar.name)
        .map(|name| name.value.as_str())
    else {
        return false;
    };
    let Some(feature) = ir
        .model
        .features
        .iter()
        .find(|feature| feature.native_ref.as_deref() == Some(history_id))
    else {
        return false;
    };
    let operation = feature.evaluation.definition().operation();
    let Ok(value) = serde_json::to_value(operation) else {
        return false;
    };
    let mut paths = Vec::new();
    if crate::history::encode::bindings::visit_operation_roles(operation, |roles, role_paths| {
        if roles.contains(&role) {
            paths.extend(role_paths.iter().map(ToString::to_string));
        }
        Ok(())
    })
    .is_err()
    {
        return false;
    }
    paths.iter().any(|path| {
        value
            .pointer(path)
            .filter(|dimension| {
                dimension.get("kind").and_then(serde_json::Value::as_str) == Some("literal")
            })
            .and_then(|dimension| dimension.get("value")?.as_f64())
            .is_some_and(|literal| literal / divisor == scalar.value.get())
    })
}

fn order_owner_parameters(mut parameters: Vec<&DesignParameter>) -> Vec<&DesignParameter> {
    parameters.sort_by_key(|parameter| parameter.ordinal);
    let indices = parameters
        .iter()
        .enumerate()
        .map(|(index, parameter)| (&parameter.id, index))
        .collect::<HashMap<_, _>>();
    let dependencies = parameters
        .iter()
        .map(|parameter| {
            parameter
                .expression
                .dependencies()
                .filter_map(|id| indices.get(id).copied())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut remaining = dependencies.iter().map(Vec::len).collect::<Vec<_>>();
    let mut consumers = (0..parameters.len())
        .map(|_| Vec::new())
        .collect::<Vec<_>>();
    for (consumer, dependencies) in dependencies.iter().enumerate() {
        for &dependency in dependencies {
            consumers[dependency].push(consumer);
        }
    }
    let mut ready = (0..parameters.len())
        .filter(|&index| remaining[index] == 0)
        .collect::<BTreeSet<_>>();
    let mut ordered = Vec::new();
    while let Some(index) = ready.pop_first() {
        ordered.push(parameters[index]);
        for &consumer in &consumers[index] {
            remaining[consumer] -= 1;
            if remaining[consumer] == 0 {
                ready.insert(consumer);
            }
        }
    }
    ordered.extend(
        parameters
            .into_iter()
            .enumerate()
            .filter_map(|(index, parameter)| (remaining[index] != 0).then_some(parameter)),
    );
    ordered
}

#[cfg(test)]
mod tests;
