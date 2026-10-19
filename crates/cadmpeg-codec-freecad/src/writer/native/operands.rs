// SPDX-License-Identifier: Apache-2.0
//! Canonical operands of a neutral source-native feature override native lanes.

use super::super::xml::{self, Object};
use crate::loss::FreecadLossCode;
use crate::native::PropertyRecord;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{Dimension, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::HashMap;

pub(in crate::writer) fn write(
    ir: &CadIr,
    objects: &mut [Object],
    names: &HashMap<String, String>,
    properties: &[PropertyRecord],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    for feature in &ir.model.features {
        let FeatureOperation::Native { parameters, .. } =
            feature.evaluation.definition().operation()
        else {
            continue;
        };
        let Some(object) = names
            .get(feature.id.as_str())
            .and_then(|name| objects.iter_mut().find(|object| &object.name == name))
        else {
            continue;
        };
        for (name, operand) in parameters {
            let Some(original) = feature.native_ref.as_ref().and_then(|owner| {
                properties
                    .iter()
                    .find(|property| &property.owner == owner && property.name == name.as_str())
            }) else {
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "native feature {} operand {name} has no target property schema",
                    feature.id
                )));
                continue;
            };
            let text = match operand {
                Dimension::Literal(expression) => expression.literal_text(),
                Dimension::Parameter(id) => ir
                    .model
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == *id)
                    .and_then(|parameter| parameter.value.as_ref())
                    .map(|value| match value {
                        cadmpeg_ir::features::ParameterValue::Length(value) => {
                            value.get().to_string()
                        }
                        cadmpeg_ir::features::ParameterValue::Angle(value) => {
                            value.get().to_degrees().to_string()
                        }
                        cadmpeg_ir::features::ParameterValue::Real(value) => {
                            value.get().to_string()
                        }
                        cadmpeg_ir::features::ParameterValue::Integer(value) => value.to_string(),
                        cadmpeg_ir::features::ParameterValue::Boolean(value) => {
                            u8::from(*value).to_string()
                        }
                        cadmpeg_ir::features::ParameterValue::String(value) => value.clone(),
                    }),
            };
            let Some(text) = text else {
                object.properties.remove(name.as_str());
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "native feature {} operand {name} has no evaluated target scalar",
                    feature.id
                )));
                continue;
            };
            let mut property = original.clone();
            let Some(value) = property.values_mut().and_then(|values| {
                values.iter_mut().find(|value| {
                    value.attributes.contains_key("value") || value.attributes.contains_key("Value")
                })
            }) else {
                object.properties.remove(name.as_str());
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "native feature {} operand {name} has no writable scalar lane",
                    feature.id
                )));
                continue;
            };
            let key = if value.attributes.contains_key("value") {
                "value"
            } else {
                "Value"
            };
            value.attributes.insert(key.into(), text);
            if crate::writer::xml::validate(&property).is_err() {
                object.properties.remove(name.as_str());
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "native feature {} operand {name} cannot be encoded as {}",
                    feature.id, property.type_name
                )));
                continue;
            }
            if let Some(value) = xml::native_named(&property, names)? {
                object.set(name.as_str(), value);
            } else {
                object.properties.remove(name.as_str());
                losses.push(FreecadLossCode::ExportFeatureNotTransferred.note(format!(
                    "native feature {} operand {name} has no target encoding",
                    feature.id
                )));
            }
        }
    }
    Ok(())
}
