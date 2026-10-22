// SPDX-License-Identifier: Apache-2.0
//! Parameter records for native write.

use crate::records::FeatureContent;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{DesignParameter, FeatureId, ParameterValue};
use std::collections::HashMap;

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
    for parameter in parameters {
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
        let mut parameters = desired.remove(feature_id).unwrap_or_default();
        parameters.sort_by_key(|parameter| parameter.ordinal);
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
        let scalar_name = lane
            .names
            .iter()
            .find(|name| name.id == lane.scalars[location.1].name)
            .map(|name| name.value.as_str());
        if scalar_name != Some(parameter.name.as_str()) {
            // (P): changing a counted scalar name relocates following lane records.
            return Err(CodecError::UnsupportedContent(format!(
                "SLDPRT canonical parameter {} was renamed; input lanes require regeneration",
                parameter.id
            )));
        }
        let scalar = &mut lane.scalars[location.1];
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
            // (P): a reloaded typed lane has no physical patch position.
            let source_offset = scalar.offset.ok_or_else(|| {
                CodecError::UnsupportedContent(format!(
                    "SLDPRT scalar {} has no source patch position; regenerate its input lane",
                    scalar.id
                ))
            })?;
            let offset = usize::try_from(source_offset)
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
